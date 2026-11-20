//! # mochy-db — rusqlite schema v45 mirror, repositories, invalidation
//! events, Android-DB importer
//!
//! Upstream absorption (Android, pin `bde01cb4`):
//! - `db/MusicDatabase.kt` (905 lines; `MusicDatabaseVersion = 45` at line
//!   110, entity + view lists, the 14 hand-written migrations, the onOpen
//!   PRAGMAs) — becomes [`schema`] (v45 baseline DDL at
//!   `PRAGMA user_version = 1`; Room's migration chain is NOT ported,
//!   only 35→45 was contiguous) and [`importer`] (the Android import).
//! - `db/entities/*.kt` (24 entities + relation wrappers) — becomes
//!   [`entities`].
//! - `db/DatabaseDao.kt` (2,131 lines, the mega-DAO) + `db/daos/
//!   SpeedDialDao.kt` — becomes [`queries`] (sync SQL with per-query
//!   citations) and [`repo`] (per-aggregate repositories over the actor).
//! - `app/schemas/…/34.json` (last exported Room schema JSON) — the
//!   column/type/default source for everything ≤ v34; v35→v45 additions
//!   are reconstructed from the migration files (embedded as a test
//!   fixture; see `tests/schema_report.rs`).
//! - Media3 `exoplayer_internal.db` (`DefaultDownloadIndex`) — the
//!   downloads seed ([`importer::downloads`]).
//!
//! Architecture (blueprint ch. 7.3 / 9.2):
//! - [`Db`] — single-writer actor (`tokio_rusqlite` command channel) +
//!   4-connection reader pool; every write commits, then fires an
//!   invalidation on the table-keyed broadcast ([`watch`]).
//! - Persistence is PURE: the network side-effects upstream embedded in
//!   entity methods (`SongEntity.toggleLike` firing `YouTube.likeVideo`)
//!   are deliberately absent — Phase 14's application SongService owns
//!   them. Every repository method that upstream pairs with a YouTube call
//!   documents the hook.
//! - The 3 view-sorted tables (`sorted_song_artist_map`,
//!   `sorted_song_album_map`, `playlist_song_map_preview`) keep their
//!   views for schema parity, but the consuming queries inline the ORDER BY
//!   (SQLite does not guarantee view ordering).
//!
//! Layering (PORT_SPEC §3.1): domain-adjacent infrastructure crate. The
//! actor's async API is runtime-agnostic (no tokio context is needed to
//! poll the write futures — GPUI's executor can await them); no GPUI, no
//! platform-specific code.

pub mod db;
pub mod entities;
pub mod error;
pub mod queries;
pub mod repo;
pub mod schema;
pub mod util;
pub mod watch;

pub use db::{Db, ReaderPool};
pub use entities::*;
pub use error::{DbError, DbResult};
pub use importer::downloads::{DownloadSeedEntry, DownloadsSeed, download_state};
pub use importer::event_time::{WALL_CLOCK_AS_UTC_COLUMNS, offset_label};
pub use importer::queue::{QueueReconstruction, RECONSTRUCTION_LIMIT, ReconstructedSong};
pub use importer::{ImportOptions, ImportReport, TableReport, import_android_db, read_import_meta};
pub use queries::{AlbumSort, ArtistSort, MAX_SPEED_DIAL_ITEMS, PlaylistSort, SongSort};
pub use repo::{
    AlbumRepository, ArtistRepository, EventRepository, FormatRepository,
    ImportBookkeepingRepository, LyricsRepository, PlaylistRepository, RadioRepository,
    RecognitionRepository, Repositories, SearchHistoryRepository, SongEditRepository,
    SongRepository, SpeedDialRepository,
};
pub use watch::{InvalidationHandle, Table, TableSet, WatchError, WatchGuard};
