//! Table-watcher invalidation — the Room Flow replacement (blueprint ch. 7.3).
//!
//! Every repository write signals AFTER COMMIT on a broadcast channel keyed
//! by table set; queries subscribe and re-run (later GPUI entities consume
//! these through `cx.spawn`). This reproduces Room's invalidation tracker,
//! which is what the Android ViewModels' `Flow` queries re-ran on.
//!
//! Two consumption shapes are public:
//! - [`InvalidationHandle`] — the raw multi-subscriber broadcast
//!   (drop-oldest under lag), pollable from any async executor (the tokio
//!   broadcast primitive needs no runtime context to be *polled*, so GPUI's
//!   executor can await it directly);
//! - [`Db::watch`] — `std::sync::mpsc::Receiver` + [`WatchGuard`], the same
//!   shape as mochy-config's settings watchers, so application code has ONE
//!   watcher idiom across settings and database. The fan-out into these
//!   happens synchronously in [`Invalidations::send`]; no forwarding thread
//!   exists to leak or wake late.

use std::sync::mpsc;
use std::sync::{Arc, Mutex};

/// The 24 mirrored tables as an enum for watcher keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Table {
    Song,
    Artist,
    Album,
    Playlist,
    SongArtistMap,
    SongAlbumMap,
    AlbumArtistMap,
    PlaylistSongMap,
    SearchHistory,
    Format,
    Lyrics,
    Event,
    RelatedSongMap,
    SetVideoId,
    PlayCount,
    RecognitionHistory,
    SpeedDialItem,
    PlaylistEvent,
    ImportedSong,
    ImportSession,
    ImportSessionSong,
    ImportedPlaylist,
    ImportedPlaylistTrack,
    SongEdit,
}

impl Table {
    /// SQL table name (identical to Android's).
    pub const fn table_name(self) -> &'static str {
        match self {
            Table::Song => "song",
            Table::Artist => "artist",
            Table::Album => "album",
            Table::Playlist => "playlist",
            Table::SongArtistMap => "song_artist_map",
            Table::SongAlbumMap => "song_album_map",
            Table::AlbumArtistMap => "album_artist_map",
            Table::PlaylistSongMap => "playlist_song_map",
            Table::SearchHistory => "search_history",
            Table::Format => "format",
            Table::Lyrics => "lyrics",
            Table::Event => "event",
            Table::RelatedSongMap => "related_song_map",
            Table::SetVideoId => "set_video_id",
            Table::PlayCount => "playCount",
            Table::RecognitionHistory => "recognition_history",
            Table::SpeedDialItem => "speed_dial_item",
            Table::PlaylistEvent => "playlist_event",
            Table::ImportedSong => "imported_song",
            Table::ImportSession => "import_session",
            Table::ImportSessionSong => "import_session_song",
            Table::ImportedPlaylist => "imported_playlist",
            Table::ImportedPlaylistTrack => "imported_playlist_track",
            Table::SongEdit => "song_edit",
        }
    }

    /// All tables in schema order.
    pub const ALL: [Table; 24] = [
        Table::Song,
        Table::Artist,
        Table::Album,
        Table::Playlist,
        Table::SongArtistMap,
        Table::SongAlbumMap,
        Table::AlbumArtistMap,
        Table::PlaylistSongMap,
        Table::SearchHistory,
        Table::Format,
        Table::Lyrics,
        Table::Event,
        Table::RelatedSongMap,
        Table::SetVideoId,
        Table::PlayCount,
        Table::RecognitionHistory,
        Table::SpeedDialItem,
        Table::PlaylistEvent,
        Table::ImportedSong,
        Table::ImportSession,
        Table::ImportSessionSong,
        Table::ImportedPlaylist,
        Table::ImportedPlaylistTrack,
        Table::SongEdit,
    ];
}

/// A set of tables, used as the invalidation key.
///
/// Upstream analogue: Room's `InvalidationTracker` notifies each Flow query
/// with the set of tables its query depends on. A write to `song` fans out
/// to every query that reads `song`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct TableSet(u32);

impl TableSet {
    /// The empty set.
    pub const EMPTY: TableSet = TableSet(0);

    /// Builds a set from tables.
    pub const fn of(tables: &[Table]) -> TableSet {
        let mut bits = 0u32;
        let mut i = 0;
        while i < tables.len() {
            bits |= 1 << (tables[i] as u32);
            i += 1;
        }
        TableSet(bits)
    }

    /// All 24 tables.
    pub const fn all() -> TableSet {
        TableSet(0xFF_FFFF)
    }

    /// Union.
    pub const fn union(self, other: TableSet) -> TableSet {
        TableSet(self.0 | other.0)
    }

    /// The tables present in both sets.
    pub const fn intersection(self, other: TableSet) -> TableSet {
        TableSet(self.0 & other.0)
    }

    /// Whether every table in `other` is also in `self`.
    pub const fn contains(self, other: TableSet) -> bool {
        (self.0 & other.0) == other.0
    }

    /// Whether the set is empty.
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Iterates the member tables.
    pub fn iter(self) -> impl Iterator<Item = Table> {
        Table::ALL.iter().copied().filter(move |t| {
            let one = TableSet(1 << (*t as u32));
            self.contains(one)
        })
    }

    /// The member tables' SQL names.
    pub fn table_names(self) -> Vec<&'static str> {
        self.iter().map(Table::table_name).collect()
    }
}

impl std::fmt::Display for TableSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let names = self.table_names();
        write!(f, "[{}]", names.join(", "))
    }
}

/// Errors surfaced by watcher consumption.
#[derive(Debug, thiserror::Error, PartialEq, Eq, Clone, Copy)]
pub enum WatchError {
    /// Every sender was dropped (the [`crate::Db`] is gone).
    #[error("invalidation source closed")]
    Closed,
}

/// Handle over the raw invalidation broadcast.
///
/// `recv()` is pollable from any executor — the underlying tokio broadcast
/// receiver needs no runtime context to be polled, matching GPUI's executor
/// (PORT_SPEC §3.1 rule 4: the DB actor bridges through channels).
pub struct InvalidationHandle {
    rx: tokio::sync::broadcast::Receiver<TableSet>,
}

impl InvalidationHandle {
    /// Waits for the next invalidation covering at least `tables`.
    ///
    /// Invalidations for other tables are skipped. Broadcast lag is
    /// coalesced: a `Lagged` receive drains to the oldest retained event
    /// instead of failing (Room also just re-runs queries).
    pub async fn recv(&mut self, tables: TableSet) -> Result<TableSet, WatchError> {
        loop {
            let event = match self.rx.recv().await {
                Ok(event) => event,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    match self.rx.try_recv() {
                        Ok(event) => event,
                        Err(_) => continue,
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    return Err(WatchError::Closed);
                }
            };
            if event.contains(tables) || tables.is_empty() {
                return Ok(event);
            }
        }
    }
}

/// Guard that keeps a `Db::watch` subscription alive; dropping it
/// unsubscribes (same shape as mochy-config's `WatchGuard`).
pub struct WatchGuard {
    id: u64,
    registry: Arc<WatcherRegistry>,
}

impl Drop for WatchGuard {
    fn drop(&mut self) {
        self.registry
            .0
            .lock()
            .expect("watch registry poisoned")
            .remove(&self.id);
    }
}

impl std::fmt::Debug for WatchGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WatchGuard").field("id", &self.id).finish()
    }
}

struct WatcherRegistry(Mutex<std::collections::HashMap<u64, (TableSet, mpsc::Sender<TableSet>)>>);

impl WatcherRegistry {
    fn next_id(&self) -> u64 {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(1);
        NEXT.fetch_add(1, Ordering::Relaxed)
    }
}

/// The invalidation bus owned by the Db.
///
/// `send` is called from inside the writer closure after commit
/// (synchronously — tokio broadcast's send is a plain function; the mpsc
/// fan-out uses `try_send` so a stuck consumer can never block the writer).
///
/// Cheaply cloneable (both halves are Arc'd) because every write closure
/// needs an owned copy to fire post-commit on the actor thread.
#[derive(Clone)]
pub(crate) struct Invalidations {
    tx: Arc<tokio::sync::broadcast::Sender<TableSet>>,
    watchers: Arc<WatcherRegistry>,
}

impl Default for Invalidations {
    fn default() -> Self {
        Self::new()
    }
}

impl Invalidations {
    pub(crate) fn new() -> Self {
        let (tx, _) = tokio::sync::broadcast::channel(256);
        Self {
            tx: Arc::new(tx),
            watchers: Arc::new(WatcherRegistry(
                Mutex::new(std::collections::HashMap::new()),
            )),
        }
    }

    /// Broadcasts an invalidation for `tables` (no-op when empty).
    pub(crate) fn send(&self, tables: TableSet) {
        if tables.is_empty() {
            return;
        }
        // A send with zero subscribers errors; that is the normal state
        // before anyone subscribes — ignore.
        let _ = self.tx.send(tables);

        let mut watchers = self.watchers.0.lock().expect("watch registry poisoned");
        let mut dead = Vec::new();
        for (id, (filter, sender)) in watchers.iter() {
            let hit = tables.intersection(*filter);
            if hit.is_empty() {
                continue;
            }
            if sender.send(hit).is_err() {
                dead.push(*id);
            }
        }
        for id in dead {
            watchers.remove(&id);
        }
    }

    /// Raw broadcast subscription.
    pub(crate) fn subscribe(&self) -> InvalidationHandle {
        InvalidationHandle {
            rx: self.tx.subscribe(),
        }
    }

    /// std-mpsc watcher over `tables`: the receiver yields the intersecting
    /// table subset of every write that touches any of them.
    pub(crate) fn watch(&self, tables: TableSet) -> (mpsc::Receiver<TableSet>, WatchGuard) {
        let (tx, rx) = mpsc::channel();
        let id = self.watchers.next_id();
        self.watchers
            .0
            .lock()
            .expect("watch registry poisoned")
            .insert(id, (tables, tx));
        let guard = WatchGuard {
            id,
            registry: Arc::clone(&self.watchers),
        };
        (rx, guard)
    }
}
