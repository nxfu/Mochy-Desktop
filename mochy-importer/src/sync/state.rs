//! The sync operation vocabulary and state machine —
//! `utils/SyncUtils.kt:64-128`.
//!
//! Upstream models the operations as a sealed class carried by a
//! buffered channel (one consumer, serialized), the per-surface status
//! as another sealed class, and the aggregate as a data class the UI
//! collects. The port keeps the same three shapes; the channel becomes
//! the engine's in-memory queue ([`crate::sync::engine::SyncEngine`]).

use std::fmt;

/// `SyncOperation` (SyncUtils.kt:64-78) — one queued unit of sync work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncOperation {
    /// `FullSync` — every arm in order (SyncUtils.kt:349-399).
    FullSync,
    /// `LikedSongs` — pull `LM` (`executeSyncLikedSongs`).
    LikedSongs,
    /// `LibrarySongs` — pull `FEmusic_liked_videos`.
    LibrarySongs,
    /// `UploadedSongs` — pull `FEmusic_library_privately_owned_tracks`.
    UploadedSongs,
    /// `LikedAlbums` — pull `FEmusic_liked_albums`.
    LikedAlbums,
    /// `UploadedAlbums` — pull `FEmusic_library_privately_owned_releases`.
    UploadedAlbums,
    /// `ArtistsSubscriptions` — pull `FEmusic_library_corpus_artists`.
    ArtistsSubscriptions,
    /// `SavedPlaylists` — pull `FEmusic_liked_playlists`.
    SavedPlaylists,
    /// `AutoSyncPlaylists` — re-sync every playlist flagged auto-sync.
    AutoSyncPlaylists,
    /// `SinglePlaylist(browseId, playlistId)` (SyncUtils.kt:74).
    SinglePlaylist {
        browse_id: String,
        playlist_id: String,
    },
    /// `LikeSong(song)` (SyncUtils.kt:75) — push one like/unlike.
    LikeSong { song: SongRef },
    /// `CleanupDuplicates` — de-duplicate playlists sharing a browseId.
    CleanupDuplicates,
    /// `ClearAllSynced` — remove everything a sync brought in.
    ClearAllSynced,
}

/// The song slice `LikeSong` carries — the `SongEntity` fields
/// `executeLikeSong` reads (SyncUtils.kt:414-431: `s.id`, `s.liked`,
/// plus `s.title` for the scrobbler love).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SongRef {
    pub id: String,
    pub title: String,
    pub liked: bool,
}

/// `SyncStatus` (SyncUtils.kt:80-85).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum SyncStatus {
    /// `Idle`.
    #[default]
    Idle,
    /// `Syncing`.
    Syncing,
    /// `Error(message)`.
    Error(String),
    /// `Completed`.
    Completed,
}

/// `SyncState` (SyncUtils.kt:87-97) — the aggregate the UI collects;
/// `currentOperation` carries the same literals upstream feeds it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SyncState {
    pub overall_status: SyncStatus,
    pub liked_songs: SyncStatus,
    pub library_songs: SyncStatus,
    pub uploaded_songs: SyncStatus,
    pub liked_albums: SyncStatus,
    pub uploaded_albums: SyncStatus,
    pub artists: SyncStatus,
    pub playlists: SyncStatus,
    pub current_operation: String,
}

impl SyncState {
    /// The upstream `updateState { copy(...) }` builder shape.
    pub fn with(mut self, update: impl FnOnce(Self) -> Self) -> Self {
        self = update(self);
        self
    }
}

impl fmt::Display for SyncOperation {
    /// Used for tracing, exactly the shape upstream logs.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SyncOperation::FullSync => write!(f, "FullSync"),
            SyncOperation::LikedSongs => write!(f, "LikedSongs"),
            SyncOperation::LibrarySongs => write!(f, "LibrarySongs"),
            SyncOperation::UploadedSongs => write!(f, "UploadedSongs"),
            SyncOperation::LikedAlbums => write!(f, "LikedAlbums"),
            SyncOperation::UploadedAlbums => write!(f, "UploadedAlbums"),
            SyncOperation::ArtistsSubscriptions => write!(f, "ArtistsSubscriptions"),
            SyncOperation::SavedPlaylists => write!(f, "SavedPlaylists"),
            SyncOperation::AutoSyncPlaylists => write!(f, "AutoSyncPlaylists"),
            SyncOperation::SinglePlaylist {
                browse_id,
                playlist_id,
            } => write!(
                f,
                "SinglePlaylist(browseId={browse_id}, playlistId={playlist_id})"
            ),
            SyncOperation::LikeSong { song } => write!(f, "LikeSong({})", song.id),
            SyncOperation::CleanupDuplicates => write!(f, "CleanupDuplicates"),
            SyncOperation::ClearAllSynced => write!(f, "ClearAllSynced"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SyncUtils.kt:87-97 — every surface starts idle, the aggregate is
    /// updatable exactly like upstream's `updateState { copy(...) }`.
    #[test]
    fn state_defaults_and_updates_like_upstream() {
        let initial = SyncState::default();
        assert_eq!(initial.overall_status, SyncStatus::Idle);
        assert_eq!(initial.liked_songs, SyncStatus::Idle);
        assert_eq!(initial.current_operation, "");

        let updated = initial.with(|state| SyncState {
            liked_songs: SyncStatus::Syncing,
            current_operation: "Syncing liked songs".into(),
            ..state
        });
        assert_eq!(updated.liked_songs, SyncStatus::Syncing);
        assert_eq!(updated.current_operation, "Syncing liked songs");
        // untouched surfaces keep their value
        assert_eq!(updated.library_songs, SyncStatus::Idle);
    }

    /// SyncUtils.kt:64-78 — all 13 operation variants exist.
    #[test]
    fn operation_vocabulary_is_complete() {
        let all = vec![
            SyncOperation::FullSync,
            SyncOperation::LikedSongs,
            SyncOperation::LibrarySongs,
            SyncOperation::UploadedSongs,
            SyncOperation::LikedAlbums,
            SyncOperation::UploadedAlbums,
            SyncOperation::ArtistsSubscriptions,
            SyncOperation::SavedPlaylists,
            SyncOperation::AutoSyncPlaylists,
            SyncOperation::SinglePlaylist {
                browse_id: "VL123".into(),
                playlist_id: "local-1".into(),
            },
            SyncOperation::LikeSong {
                song: SongRef {
                    id: "vid".into(),
                    title: "Song".into(),
                    liked: true,
                },
            },
            SyncOperation::CleanupDuplicates,
            SyncOperation::ClearAllSynced,
        ];
        assert_eq!(all.len(), 13);
        // stable Debug/Display, useful in the engine's tracing
        assert_eq!(
            all[9].to_string(),
            "SinglePlaylist(browseId=VL123, playlistId=local-1)"
        );
        assert_eq!(all[10].to_string(), "LikeSong(vid)");
    }
}
