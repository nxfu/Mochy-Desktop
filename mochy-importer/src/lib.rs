//! Account/link/CSV/local importers, TrackMatcher, TransferArchive, YT sync engine
//!
//! Upstream absorption: importer/ (42 files: Spotify/Apple/Tidal accounts, LinkImporter, CsvImport, LocalImportService, transfer/) + utils/SyncUtils (YT two-way sync).
//!
//! Phase 07 fills in the `sync` module — the `utils/SyncUtils.kt` port's
//! foundation: the operation/state machine (SyncUtils.kt:64-128), the
//! pending-op queue that records likes/library edits while signed out and
//! replays them on login, the likes memory
//! (`importer/YouTubeLikesMemory.kt`), the undo journal
//! (`importer/SyncUndo.kt`), the lost-likes recovery scaffolding
//! (`importer/LostLikes.kt`), the two-way engine traits and the
//! history/play-reporting gates. The full pull/push arms
//! (`executeSyncLikedSongs` & co.) are Phase 22's scope over these same
//! seams (PROGRESS M6).
//!
//! Layering: domain crate — no GPUI, no tokio in the public API; the
//! async seams return boxed futures (Phase 06 `AlternateStreamEngine`
//! precedent).

pub mod sync;

#[cfg(test)]
mod tests {
    #[test]
    fn crate_skeleton_presents() {
        assert_eq!(env!("CARGO_PKG_NAME"), "mochy-importer");
    }
}
