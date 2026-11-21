//! The two-way sync engine traits and the queue-pumping engine —
//! `utils/SyncUtils.kt:99-160, 174-199, 414-431`.
//!
//! Upstream `SyncUtils` is a Hilt singleton with a buffered channel of
//! [`SyncOperation`]s and ONE consumer coroutine; every arm first checks
//! `isLoggedIn()`, the full-sync arm additionally the hold flag and the
//! per-arm settings. Phase 22 ports the pull arms
//! (`executeSyncLikedSongs` & co.) over these same traits; Phase 07
//! ships the seams plus the one push arm the pending queue builds on:
//!
//! - [`YouTubeSyncApi`] — the `YouTube` facade surface the engine
//!   drives (Phase 22 extends it with the pulls).
//! - [`SyncDb`] — the database surface (snapshots for the undo journal,
//!   writes for the restores, the importer bookkeeping the gates read).
//! - [`Sleeper`] — the delay abstraction (`delay()` upstream) so the
//!   retry backoff and the inter-write pacing stay runtime-free and
//!   instant under tests.
//! - [`LoveSink`] — `scrobbler.love(artist, track, love)`
//!   (SyncUtils.kt:415-417: a love on Last.fm/Libre.fm runs BEFORE the
//!   YouTube login check — it needs no YouTube account). The Phase 23
//!   scrobbler implements it.
//! - [`ReplayApi`] — the pending-queue replay surface.
//!
//! Async methods return boxed futures (the Phase 06
//! `AlternateStreamEngine` precedent — the seams hold `dyn` traits).

use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use mochy_config::SettingsStore;
use mochy_vault::{Vault, VaultKey};

use super::pending::{PendingOp, PendingOpQueue};
use super::state::{SongRef, SyncOperation, SyncState};

/// The boxed-future alias used by every async seam in this module.
type BoxFut<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// `SyncUtils.kt:120-124` — the retry/backoff constants.
pub const MAX_RETRIES: u32 = 3;
pub const INITIAL_RETRY_DELAY_MS: u64 = 1_000;
/// `DB_OPERATION_DELAY_MS` — the pacing between database writes.
pub const DB_OPERATION_DELAY_MS: u64 = 50;

/// The `YouTube` facade surface the sync engine drives. Phase 07 needs
/// the like push; Phase 22 adds the pulls (`playlist("LM")`,
/// `library(...)`, `album(...)` — see SyncUtils.kt:444-1185) over the
/// same trait, so the engine never names the facade directly.
pub trait YouTubeSyncApi: Send + Sync {
    /// `YouTube.likeVideo(videoId, like)` — the push half.
    fn like_video(&self, video_id: &str, like: bool) -> BoxFut<'_, Result<(), String>>;
}

/// The database surface — the `MusicDatabase` queries and writes
/// `SyncUtils`/`SyncUndo`/`LostLikes` use, as a trait so the engine
/// stays pure and the Phase 22 adapter bridges the mochy-db actor.
/// Timestamps are epoch milliseconds (the Phase 03 entity convention).
pub trait SyncDb: Send + Sync {
    /// `likedSongsByNameAsc()` — the liked slice with dates.
    fn liked_songs(&self) -> Vec<(String, Option<i64>)>;
    /// The songs with `inLibrary != null`, with the membership date
    /// (`songsByNameAsc()` filtered).
    fn library_songs(&self) -> Vec<(String, i64)>;
    /// `playlistsByNameAsc()` filtered to `browseId != null`.
    fn playlists(&self) -> Vec<PlaylistRef>;
    /// `playlistSongs(playlistId)` ordered by position.
    fn playlist_songs(&self, playlist_id: &str) -> Vec<PlaylistMapEntry>;
    /// Whether the playlist still exists (the undo skips deleted ones).
    fn playlist_exists(&self, playlist_id: &str) -> bool;
    /// `song(id)?.artists.firstOrNull()?.name` — the like path's
    /// scrobbler lookup.
    fn song_artist(&self, video_id: &str) -> Option<String>;
    /// `getSongById(id) != null` (the undo only restores known songs).
    fn song_exists(&self, video_id: &str) -> bool;
    /// `unlikedSongEntities(ids)` — songs that SHOULD be liked but are
    /// not (the lost-likes candidates).
    fn unliked_songs(&self, ids: &[String]) -> Vec<SongRef>;
    /// `likesFromImports()` — (song id, imported-at) for likes any
    /// import left behind.
    fn likes_from_imports(&self) -> Vec<(String, Option<i64>)>;
    /// `importedSongIdsExcept(source)` — ids imported from any service
    /// other than the named one.
    fn imported_song_ids_except(&self, source: &str) -> HashSet<String>;
    /// `importSessionSongIds()` — ids that arrived through an import
    /// session (protected from library removal).
    fn import_session_song_ids(&self) -> HashSet<String>;

    // -- writes (SyncUndo.restoreLocal, LostLikes.bringBack, the arms) --
    /// `update(song.copy(liked = .., likedDate = ..))`.
    fn set_liked(&self, video_id: &str, liked: bool, liked_date: Option<i64>);
    /// `inLibrary(id, date)`.
    fn set_in_library(&self, video_id: &str, date: Option<i64>);
    /// `clearPlaylist` + re-insert the given map.
    fn replace_playlist_songs(&self, playlist_id: &str, entries: &[PlaylistMapEntry]);
}

/// A playlist row as the sync sees it (`playlistsByNameAsc` filtered).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistRef {
    pub id: String,
    pub name: String,
    pub browse_id: String,
}

/// One `PlaylistSongMap` row (`map.position`, `map.songId`,
/// `map.setVideoId`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistMapEntry {
    pub song_id: String,
    pub position: i32,
    pub set_video_id: Option<String>,
}

/// `delay(ms)` — the runtime-free sleep seam. The production impl
/// (Phase 22/app shell) bridges the tokio timer; tests run it instantly.
pub trait Sleeper: Send + Sync {
    fn sleep_ms(&self, ms: u64) -> BoxFut<'static, ()>;
}

/// The no-op sleeper (tests).
#[derive(Debug, Default, Clone, Copy)]
pub struct InstantSleeper;

impl Sleeper for InstantSleeper {
    fn sleep_ms(&self, _ms: u64) -> BoxFut<'static, ()> {
        Box::pin(std::future::ready(()))
    }
}

/// `scrobbler.love(artist, track, love)` (SyncUtils.kt:415-417) — the
/// fire-and-forget scrobbler queue behind the trait.
pub trait LoveSink: Send + Sync {
    fn love(&self, artist: &str, track: &str, love: bool);
}

/// The pending queue's replay surface — implemented over
/// [`YouTubeSyncApi`] by the engine, or by the Phase 22 adapter.
pub trait ReplayApi: Send + Sync {
    /// Push `YouTube.likeVideo`.
    fn like_song(&self, video_id: &str, liked: bool) -> BoxFut<'_, Result<(), String>>;
    /// Push a library add/remove (the feedback-token path).
    fn edit_library_song(&self, video_id: &str, in_library: bool)
    -> BoxFut<'_, Result<(), String>>;
}

/// The result of one replay pass ([`PendingOpQueue::replay`] report).
pub use super::pending::ReplayReport;

/// `SyncUtils.isLoggedIn()` (SyncUtils.kt:162-172) — the cookie from
/// the vault (the desktop home of `InnerTubeCookieKey`) counts as
/// signed-in only when it carries a `SAPISID`.
pub fn is_logged_in(vault: &Vault) -> bool {
    match vault.get(VaultKey::InnerTubeCookie) {
        Some(cookie) => mochy_innertube::parse_cookie_string(&cookie).contains_key("SAPISID"),
        None => false,
    }
}

/// The engine — one operation queue, one consumer ([`SyncEngine::run`]),
/// the shared state the UI reads. Ports the SyncUtils constructor +
/// `startProcessingQueue` (SyncUtils.kt:99-160) and `executeLikeSong`
/// (SyncUtils.kt:414-431); the pull arms are Phase 22 (they return
/// [`SyncEngineError::Phase22Scope`] rather than half-behaviors).
pub struct SyncEngine {
    queue: Mutex<Vec<SyncOperation>>,
    state: Arc<Mutex<SyncState>>,
    vault: Vault,
    store: SettingsStore,
    api: Arc<dyn YouTubeSyncApi>,
    db: Arc<dyn SyncDb>,
    sleeper: Arc<dyn Sleeper>,
    love: Option<Arc<dyn LoveSink>>,
}

impl SyncEngine {
    /// Wire the engine. `love` is optional — the scrobbler phase
    /// provides it; without it the love call is skipped (upstream
    /// injects it always, but the scrobbler itself no-ops when
    /// unsigned-in, so absence is behaviorally equal).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        vault: Vault,
        store: SettingsStore,
        api: Arc<dyn YouTubeSyncApi>,
        db: Arc<dyn SyncDb>,
        sleeper: Arc<dyn Sleeper>,
        love: Option<Arc<dyn LoveSink>>,
    ) -> Self {
        SyncEngine {
            queue: Mutex::new(Vec::new()),
            state: Arc::new(Mutex::new(SyncState::default())),
            vault,
            store,
            api,
            db,
            sleeper,
            love,
        }
    }

    /// The live state ([`SyncUtils.syncState`]).
    pub fn state(&self) -> SyncState {
        self.state.lock().expect("sync state lock").clone()
    }

    /// The queue sender — every public `syncX()` upstream became this.
    /// Pending likes/library edits made while signed out are recorded
    /// into `pending` instead of queued (the Phase 07 queue; see
    /// `pending` module docs) — pass the same dir the app opened it
    /// with; `None` skips recording (no app dir yet).
    pub fn enqueue(&self, operation: SyncOperation, pending: Option<&mut PendingOpQueue>) {
        if !is_logged_in(&self.vault) {
            if let Some(pending) = pending {
                let recorded = match &operation {
                    SyncOperation::LikeSong { song } => Some(PendingOp::LikeSong {
                        video_id: song.id.clone(),
                        liked: song.liked,
                    }),
                    // no library-op enqueue exists upstream (the library
                    // pull owns membership); the pending queue accepts
                    // library edits from the local library UI in Phase 10+
                    _ => None,
                };
                if let Some(op) = recorded
                    && let Err(error) = pending.record(op.clone())
                {
                    tracing::warn!(%error, op = ?op, "could not persist pending sync op");
                }
            }
            tracing::debug!("skipping {} - user not logged in", operation);
            return;
        }
        self.queue.lock().expect("sync queue lock").push(operation);
    }

    /// Replay the signed-out queue through the engine's API — the
    /// "login" moment. Returns the replay report; failures stay queued.
    pub async fn replay_pending(&self, pending: &mut PendingOpQueue) -> ReplayReport {
        let api = ReplayThroughApi {
            api: self.api.clone(),
        };
        pending.replay(&api).await
    }

    /// `startProcessingQueue` (SyncUtils.kt:130-160) — drain every
    /// queued operation in order; one failing operation logs and moves
    /// to the next (upstream catches per-operation).
    pub async fn run(&self) {
        loop {
            let operation = {
                let mut queue = self.queue.lock().expect("sync queue lock");
                match queue.first() {
                    Some(_) => queue.remove(0),
                    None => return,
                }
            };
            if let Err(error) = self.process(&operation).await {
                tracing::error!(%error, operation = %operation, "error processing sync operation");
            }
        }
    }

    /// `processOperation` (SyncUtils.kt:144-160) — the dispatch. The
    /// like arm is fully ported; the pulls are Phase 22's arms.
    async fn process(&self, operation: &SyncOperation) -> Result<(), SyncEngineError> {
        match operation {
            SyncOperation::LikeSong { song } => {
                self.execute_like_song(song).await;
                Ok(())
            }
            other => {
                // every remaining arm is a pull/maintenance arm whose
                // body lives in SyncUtils.kt:433-1185 — Phase 22 ports
                // them over these same traits
                tracing::debug!(operation = %other, "sync arm deferred to Phase 22");
                Err(SyncEngineError::Phase22Scope(other.clone()))
            }
        }
    }

    /// `executeLikeSong` (SyncUtils.kt:414-431).
    async fn execute_like_song(&self, song: &SongRef) {
        // before the YouTube check: a love on Last.fm or Libre.fm
        // doesn't need a YouTube account
        if let Some(love) = &self.love
            && let Some(artist) = self.db.song_artist(&song.id)
        {
            love.love(&artist, &song.title, song.liked);
        }

        if !is_logged_in(&self.vault) {
            tracing::warn!("skipping likeSong - user not logged in");
            return;
        }
        if self.excluded_from_youtube().contains(&song.id) {
            return;
        }

        if let Err(error) = self
            .with_retry(|| self.api.like_video(&song.id, song.liked))
            .await
        {
            tracing::error!(%error, video = %song.id, "failed to like song on YouTube");
        }
    }

    /// `excludedFromYouTube()` (SyncUtils.kt:401-405) — ids imported
    /// from other services when `YtExcludeImportsKey` is on.
    fn excluded_from_youtube(&self) -> HashSet<String> {
        if !self.store.snapshot().YtExcludeImports() {
            return HashSet::new();
        }
        self.db
            .imported_song_ids_except(IMPORT_SOURCE_YOUTUBE_MUSIC)
    }

    /// `withRetry` (SyncUtils.kt:174-195) — up to [`MAX_RETRIES`]
    /// attempts, doubling from [`INITIAL_RETRY_DELAY_MS`]; the last
    /// failure propagates.
    async fn with_retry<T, F>(&self, mut operation: impl FnMut() -> F) -> Result<T, String>
    where
        F: Future<Output = Result<T, String>> + Send,
    {
        let mut delay_ms = INITIAL_RETRY_DELAY_MS;
        let mut attempt = 0;
        loop {
            match operation().await {
                Ok(value) => return Ok(value),
                Err(error) => {
                    attempt += 1;
                    if attempt >= MAX_RETRIES {
                        return Err(error);
                    }
                    tracing::warn!(%error, attempt, max = MAX_RETRIES, "sync attempt failed");
                    self.sleeper.sleep_ms(delay_ms).await;
                    delay_ms *= 2;
                }
            }
        }
    }
}

/// `ImportSource.YOUTUBE_MUSIC.key` (importer/ImportSource.kt).
pub const IMPORT_SOURCE_YOUTUBE_MUSIC: &str = "YOUTUBE_MUSIC";

/// The replay adapter: the pending queue pushes through the engine's
/// facade handle (Phase 22's adapter does the same over the real one).
struct ReplayThroughApi {
    api: Arc<dyn YouTubeSyncApi>,
}

impl ReplayApi for ReplayThroughApi {
    fn like_song(&self, video_id: &str, liked: bool) -> BoxFut<'_, Result<(), String>> {
        self.api.like_video(video_id, liked)
    }

    fn edit_library_song(
        &self,
        _video_id: &str,
        _in_library: bool,
    ) -> BoxFut<'_, Result<(), String>> {
        // the library push rides the feedback-token path
        // (addSongToLibrary/removeSongFromLibrary), Phase 22's adapter
        Box::pin(async { Err("library edit push lands with the Phase 22 adapter".into()) })
    }
}

/// Engine failures. `Phase22Scope` is a typed marker, not an error
/// condition: the arm exists upstream and its port is scheduled.
#[derive(Debug, thiserror::Error)]
pub enum SyncEngineError {
    #[error("sync arm deferred to Phase 22: {0}")]
    Phase22Scope(SyncOperation),
}

#[cfg(test)]
mod tests {
    use super::*;
    use mochy_config::{SettingId, SettingValue, SettingsStore};
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A facade double that counts calls and fails selected ids.
    struct Api {
        calls: Mutex<Vec<String>>,
        fail_first: AtomicUsize,
    }

    impl Api {
        fn new() -> Self {
            Api {
                calls: Mutex::new(Vec::new()),
                fail_first: AtomicUsize::new(0),
            }
        }
    }

    impl YouTubeSyncApi for Api {
        fn like_video(&self, video_id: &str, like: bool) -> BoxFut<'_, Result<(), String>> {
            let call = format!("like({video_id},{like})");
            // fail the first N attempts to exercise with_retry
            let fails = self.fail_first.load(Ordering::SeqCst);
            if fails > 0 {
                self.fail_first.fetch_sub(1, Ordering::SeqCst);
                return Box::pin(async { Err("transient".into()) });
            }
            self.calls.lock().unwrap().push(call.clone());
            Box::pin(async move { Ok(()) })
        }
    }

    /// A minimal DB double: songs with artists and likes.
    struct Db {
        artists: Vec<(String, String)>, // (video id, artist)
        liked: Mutex<Vec<(String, bool, Option<i64>)>>,
        excluded: HashSet<String>,
    }

    impl SyncDb for Db {
        fn liked_songs(&self) -> Vec<(String, Option<i64>)> {
            self.liked
                .lock()
                .unwrap()
                .iter()
                .filter(|(_, liked, _)| *liked)
                .map(|(id, _, date)| (id.clone(), *date))
                .collect()
        }
        fn library_songs(&self) -> Vec<(String, i64)> {
            Vec::new()
        }
        fn playlists(&self) -> Vec<PlaylistRef> {
            Vec::new()
        }
        fn playlist_songs(&self, _playlist_id: &str) -> Vec<PlaylistMapEntry> {
            Vec::new()
        }
        fn playlist_exists(&self, _playlist_id: &str) -> bool {
            false
        }
        fn song_artist(&self, video_id: &str) -> Option<String> {
            self.artists
                .iter()
                .find(|(id, _)| id == video_id)
                .map(|(_, artist)| artist.clone())
        }
        fn song_exists(&self, _video_id: &str) -> bool {
            true
        }
        fn unliked_songs(&self, ids: &[String]) -> Vec<SongRef> {
            self.liked
                .lock()
                .unwrap()
                .iter()
                .filter(|(id, liked, _)| !*liked && ids.contains(id))
                .map(|(id, _, _)| SongRef {
                    id: id.clone(),
                    title: String::new(),
                    liked: false,
                })
                .collect()
        }
        fn likes_from_imports(&self) -> Vec<(String, Option<i64>)> {
            Vec::new()
        }
        fn imported_song_ids_except(&self, _source: &str) -> HashSet<String> {
            self.excluded.clone()
        }
        fn import_session_song_ids(&self) -> HashSet<String> {
            HashSet::new()
        }
        fn set_liked(&self, video_id: &str, liked: bool, liked_date: Option<i64>) {
            let mut all = self.liked.lock().unwrap();
            if let Some(entry) = all.iter_mut().find(|(id, _, _)| id == video_id) {
                entry.1 = liked;
                entry.2 = liked_date;
            }
        }
        fn set_in_library(&self, _video_id: &str, _date: Option<i64>) {}
        fn replace_playlist_songs(&self, _playlist_id: &str, _entries: &[PlaylistMapEntry]) {}
    }

    fn temp_store() -> SettingsStore {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::mem::forget(dir);
        SettingsStore::load(path)
    }

    fn song(id: &str, liked: bool) -> SongRef {
        SongRef {
            id: id.into(),
            title: format!("Song {id}"),
            liked,
        }
    }

    /// SyncUtils.kt:414-431 — the like path in full: scrobbler love
    /// first (no YouTube account needed), login gate, import exclusion,
    /// then the retrying push.
    #[tokio::test]
    async fn execute_like_song_ports_the_upstream_order() {
        let vault = Vault::open_in_memory();
        vault
            .set(VaultKey::InnerTubeCookie, "SAPISID=abc; SID=xyz")
            .unwrap();
        let store = temp_store();
        let api = Arc::new(Api::new());
        let db = Arc::new(Db {
            artists: vec![("v1".into(), "Artist One".into())],
            liked: Mutex::new(vec![("v1".into(), false, None)]),
            excluded: HashSet::new(),
        });

        #[derive(Default)]
        struct LoveCounter(Mutex<Vec<String>>);
        impl LoveSink for LoveCounter {
            fn love(&self, artist: &str, track: &str, love: bool) {
                self.0
                    .lock()
                    .unwrap()
                    .push(format!("{artist}|{track}|{love}"));
            }
        }
        let love = Arc::new(LoveCounter::default());

        let engine = SyncEngine::new(
            vault,
            store,
            api.clone(),
            db,
            Arc::new(InstantSleeper),
            Some(love.clone()),
        );
        engine.enqueue(
            SyncOperation::LikeSong {
                song: song("v1", true),
            },
            None,
        );
        engine.run().await;

        assert_eq!(
            *api.calls.lock().unwrap(),
            vec!["like(v1,true)"],
            "the push went up"
        );
        assert_eq!(
            love.0.lock().unwrap().len(),
            1,
            "the scrobbler love fired exactly once"
        );
    }

    /// SyncUtils.kt:419-422 — signed out, the like is NOT pushed
    /// (upstream drops it; the port records it into the pending queue).
    #[tokio::test]
    async fn signed_out_likes_go_to_the_pending_queue_not_youtube() {
        let vault = Vault::open_in_memory();
        let store = temp_store();
        let api = Arc::new(Api::new());
        let db = Arc::new(Db {
            artists: vec![],
            liked: Mutex::new(vec![]),
            excluded: HashSet::new(),
        });
        let engine = SyncEngine::new(
            vault,
            store,
            api.clone(),
            db,
            Arc::new(InstantSleeper),
            None,
        );

        let dir = tempfile::tempdir().unwrap();
        let mut pending = PendingOpQueue::open(dir.path());
        engine.enqueue(
            SyncOperation::LikeSong {
                song: song("v9", true),
            },
            Some(&mut pending),
        );
        engine.run().await;
        assert!(
            api.calls.lock().unwrap().is_empty(),
            "nothing goes up while signed out"
        );
        assert_eq!(pending.len(), 1, "the like is recorded instead");

        // and after sign-in it replays through the engine's facade
        let report = engine.replay_pending(&mut pending).await;
        assert_eq!(report.applied, 1);
        assert_eq!(*api.calls.lock().unwrap(), vec!["like(v9,true)"]);
    }

    /// SyncUtils.kt:401-405 — YtExcludeImports keeps imports off
    /// YouTube: excluded ids never push.
    #[tokio::test]
    async fn excluded_imports_do_not_push() {
        let vault = Vault::open_in_memory();
        vault.set(VaultKey::InnerTubeCookie, "SAPISID=1").unwrap();
        let store = temp_store();
        store
            .set_by_id(SettingId::YtExcludeImports, SettingValue::Bool(true))
            .unwrap();
        let api = Arc::new(Api::new());
        let db = Arc::new(Db {
            artists: vec![],
            liked: Mutex::new(vec![]),
            excluded: HashSet::from(["imported".into()]),
        });
        let engine = SyncEngine::new(
            vault,
            store,
            api.clone(),
            db,
            Arc::new(InstantSleeper),
            None,
        );
        engine.enqueue(
            SyncOperation::LikeSong {
                song: song("imported", true),
            },
            None,
        );
        engine.run().await;
        assert!(api.calls.lock().unwrap().is_empty());
    }

    /// SyncUtils.kt:174-195 — 3 attempts, then the failure is logged
    /// and swallowed (executeLikeSong onFailure), NOT propagated.
    #[tokio::test]
    async fn retry_policy_gives_up_after_three_attempts() {
        let vault = Vault::open_in_memory();
        vault.set(VaultKey::InnerTubeCookie, "SAPISID=1").unwrap();
        let store = temp_store();
        let api = Arc::new(Api::new());
        api.fail_first.store(5, Ordering::SeqCst); // more failures than attempts
        let db = Arc::new(Db {
            artists: vec![],
            liked: Mutex::new(vec![]),
            excluded: HashSet::new(),
        });
        let engine = SyncEngine::new(
            vault,
            store,
            Arc::clone(&api) as Arc<dyn YouTubeSyncApi>,
            db,
            Arc::new(InstantSleeper),
            None,
        );
        engine.enqueue(
            SyncOperation::LikeSong {
                song: song("v2", true),
            },
            None,
        );
        engine.run().await; // must not panic, must not push
        assert!(api.calls.lock().unwrap().is_empty());
        assert_eq!(
            api.fail_first.load(Ordering::SeqCst),
            2,
            "3 attempts consumed"
        );
    }

    /// The login check is the SAPISID one (SyncUtils.kt:162-172): a
    /// cookie without SAPISID is not signed in.
    #[test]
    fn login_gate_requires_sapisid() {
        let vault = Vault::open_in_memory();
        assert!(!is_logged_in(&vault), "no cookie = not logged in");
        vault
            .set(VaultKey::InnerTubeCookie, "SID=xyz; HSID=abc")
            .unwrap();
        assert!(!is_logged_in(&vault), "no SAPISID = not logged in");
        vault
            .set(VaultKey::InnerTubeCookie, "SID=xyz; SAPISID=abc")
            .unwrap();
        assert!(is_logged_in(&vault));
    }
}
