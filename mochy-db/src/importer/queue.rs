//! Transform (b): queue reconstruction.
//!
//! Upstream persists the player queue as Java-serialized `PersistQueue`
//! blobs (`persistent_queue.data`, `persistent_automix.data`,
//! `persistent_manual_queue.data`, `persistent_player_state.data` —
//! MusicService.kt:5630-5633, written via `ObjectOutputStream`). Java
//! serialization cannot be read from Rust, and when the Android app itself
//! fails to read them it clears the files and starts empty
//! (MusicService.kt:1246-1287). The importer therefore reconstructs a
//! PAUSED queue from the song table plus the format cache (blueprint ch.
//! 7.2): the user loses only the shuffle order — which Android restarts
//! also lose.

use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::entities::FormatEntity;
use crate::error::DbResult;

/// How many songs the reconstruction keeps. Upstream restores the whole
/// serialized queue, but a reconstruction from history is best-effort; the
/// queue machine (Phase 10) re-builds runway from radio anyway
/// (MusicService.kt QUEUE_RUNWAY_SIZE = 5).
pub const RECONSTRUCTION_LIMIT: usize = 50;

/// One reconstructed queue entry: enough of a song to start playback
/// without a refetch (id + title + duration + artists + the cached format).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReconstructedSong {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub duration: i32,
    #[serde(default)]
    pub thumbnail_url: Option<String>,
    #[serde(default)]
    pub artists: Vec<String>,
    /// The cached stream format, so the first start comes from cache
    /// (upstream `Song.format` relation).
    #[serde(default)]
    pub format: Option<FormatEntity>,
}

/// The reconstruction record handed to Phase 10's QueueMachine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueueReconstruction {
    /// Most-recently-played first; `current_song_id` is the head.
    pub songs: Vec<ReconstructedSong>,
    /// The song that was playing when the Android app last stopped.
    pub current_song_id: Option<String>,
    /// Always true: a reconstructed queue never auto-plays
    /// (upstream restores with `playWhenReady = false`,
    /// MusicService.kt:1272-1275).
    pub paused: bool,
    /// Whether the unreadable Java blob was present (informational).
    pub source_blob_present: bool,
    /// "User loses shuffle order, same as Android restarts" — recorded so
    /// the player can tell the user why the queue looks linear.
    pub shuffle_order_lost: bool,
    /// Where the songs came from ("event-history" or "library-fallback").
    #[serde(default = "default_basis")]
    pub basis: String,
}

fn default_basis() -> String {
    "event-history".to_string()
}

impl Default for QueueReconstruction {
    fn default() -> Self {
        Self {
            songs: Vec::new(),
            current_song_id: None,
            paused: true,
            source_blob_present: false,
            shuffle_order_lost: false,
            basis: default_basis(),
        }
    }
}

/// Builds the reconstruction from an Android source database: the most
/// recently played DISTINCT songs (event table, timestamp DESC), falling
/// back to the newest library songs when there is no history.
///
/// `files_dir` (when given) is the Android app's files dir; the presence
/// of `persistent_queue.data` there is recorded on the result.
pub fn reconstruct_queue(
    source: &Connection,
    files_dir: Option<&std::path::Path>,
) -> DbResult<QueueReconstruction> {
    let blob_present = files_dir
        .map(|dir| dir.join("persistent_queue.data").exists())
        .unwrap_or(false);

    // Recent distinct songs by (max) event timestamp, newest first.
    let mut recent: Vec<(String, i32, Option<String>)> = Vec::new();
    {
        let mut stmt = source.prepare(
            "SELECT song.id, song.duration, song.thumbnailUrl \
             FROM event JOIN song ON song.id = event.songId \
             GROUP BY event.songId ORDER BY MAX(event.timestamp) DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map([RECONSTRUCTION_LIMIT as i64], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i32>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })?;
        for row in rows {
            recent.push(row?);
        }
    }

    // No history at all: newest library songs (inLibrary DESC).
    if recent.is_empty() {
        let mut stmt = source.prepare(
            "SELECT id, duration, thumbnailUrl FROM song \
             WHERE inLibrary IS NOT NULL ORDER BY inLibrary DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map([RECONSTRUCTION_LIMIT as i64], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i32>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })?;
        for row in rows {
            recent.push(row?);
        }
    }

    let mut songs = Vec::with_capacity(recent.len());
    for (id, duration, thumbnail_url) in recent {
        // Artists in position order — the same consuming ORDER BY the
        // sorted_song_artist_map view encodes.
        let mut artist_stmt = source.prepare(
            "SELECT artist.name FROM song_artist_map \
             JOIN artist ON artist.id = song_artist_map.artistId \
             WHERE song_artist_map.songId = ?1 ORDER BY song_artist_map.position",
        )?;
        let artists: Vec<String> = {
            let rows = artist_stmt.query_map([&id], |r| r.get::<_, String>(0))?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        let format: Option<FormatEntity> = source
            .query_row(
                "SELECT * FROM format WHERE id = ?1",
                [&id],
                FormatEntity::from_row,
            )
            .optional()?;
        songs.push(ReconstructedSong {
            id,
            title: String::new(), // filled below from the song row
            duration,
            thumbnail_url,
            artists,
            format,
        });
    }

    // Titles in one pass (the query above avoided a per-row join).
    for song in &mut songs {
        let title: Option<String> = source
            .query_row("SELECT title FROM song WHERE id = ?1", [&song.id], |r| {
                r.get(0)
            })
            .optional()?;
        song.title = title.unwrap_or_default();
    }
    songs.retain(|s| !s.id.is_empty());

    Ok(QueueReconstruction {
        current_song_id: songs.first().map(|s| s.id.clone()),
        paused: true,
        source_blob_present: blob_present,
        // Blob present + unreadable => the shuffle order it held is gone.
        shuffle_order_lost: blob_present,
        basis: default_basis(),
        songs,
    })
}
