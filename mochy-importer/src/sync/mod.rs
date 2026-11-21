//! The `utils/SyncUtils.kt` foundation — state, queues, journals and the
//! engine traits Phase 22 completes.
//!
//! Upstream sources (Android, pin `bde01cb4`):
//! - `utils/SyncUtils.kt` — the operation channel, the retry policy and
//!   the state machine (lines 64-128, 174-199);
//! - `importer/PendingYouTubeLikes.kt` — songs liked by an import that
//!   YouTube does not know yet;
//! - `importer/YouTubeLikesMemory.kt` — what the liked-songs sync keeps
//!   between runs (the last full list YouTube served, and every like a
//!   sync removed, with dates);
//! - `importer/SyncUndo.kt` — the journal an undo restores from;
//! - `importer/LostLikes.kt` — bringing back likes a sync took off by
//!   mistake;
//! - `MainActivity.kt`/`MusicService.kt` — the play-report and
//!   history-pull gates.
//!
//! Module map:
//! - [`state`] — `SyncOperation`/`SyncStatus`/`SyncState`;
//! - [`pending`] — [`pending::PendingYouTubeLikes`] (upstream import
//!   pending-likes parity) and [`pending::PendingOpQueue`] (the port's
//!   ordered queue of likes/library edits made while signed out,
//!   replayed on login);
//! - [`memory`] — `YouTubeLikesMemory`;
//! - [`undo`] — `SyncUndo` over the [`engine::SyncDb`] trait;
//! - [`lostlikes`] — `LostLikes` scaffolding;
//! - [`engine`] — the two-way engine traits (`YouTubeSyncApi`,
//!   `SyncDb`, `Sleeper`, `LoveSink`) and the queue-pumping engine with
//!   the `executeLikeSong` arm and the retry policy;
//! - [`gates`] — the play-report / history-pull / auto-sync cooldown
//!   gates over the settings registry.

pub mod engine;
pub mod gates;
pub mod lostlikes;
pub mod memory;
pub mod pending;
pub mod state;
pub mod undo;

pub use engine::{LoveSink, ReplayApi, ReplayReport, Sleeper, SyncDb, SyncEngine, YouTubeSyncApi};
pub use gates::{
    SYNC_COOLDOWN_SECS, SyncHoldGate, auto_sync_allowed, history_sync_enabled,
    play_reporting_enabled,
};
pub use lostlikes::{bring_back as bring_back_lost_likes, count as lost_likes_count, wanted};
pub use memory::{LikesMemory, MAX_REMOVED, MAX_UNLIKES_AT_ONCE};
pub use pending::{PendingOp, PendingOpQueue, PendingYouTubeLikes};
pub use state::{SongRef, SyncOperation, SyncState, SyncStatus};
pub use undo::{SyncUndo, UndoEntry, UndoJournal};
