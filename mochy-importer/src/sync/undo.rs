//! `SyncUndo` — `importer/SyncUndo.kt`.
//!
//! Saves what the phone holds right before a full sync touches it, so
//! an undo can put it back: the liked songs with dates, the library
//! memberships with dates, the playlist contents, and the likes the
//! sync pushed UP to YouTube (an undo with `withYouTube = true` takes
//! those off again). After an undo, `YtSyncHoldKey` holds further
//! automatic pulls (the [`crate::sync::gates::SyncHoldGate`] consumer).
//!
//! The restore runs over the [`crate::sync::engine::SyncDb`] trait:
//! snapshots in, diffed writes out, exactly `restoreLocal`
//! (SyncUndo.kt:90-130). Timestamps are epoch-ms.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The file name (SyncUndo.kt:23).
const UNDO_FILE: &str = "yt_sync_undo.json";

/// `Entry(songId, setVideoId)` (SyncUndo.kt:37).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UndoEntry {
    pub song_id: String,
    #[serde(default)]
    pub set_video_id: Option<String>,
}

/// `Journal` (SyncUndo.kt:29-34). Maps became ordered vectors
/// (insertion order = the upstream LinkedHashMap iteration the restore
/// relies on); dates are epoch-ms.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UndoJournal {
    /// (song id, liked date) for every liked song at snapshot time.
    pub liked_songs: Vec<(String, Option<i64>)>,
    /// (song id, in-library date) for every library member.
    pub library_songs: Vec<(String, i64)>,
    /// (playlist id, ordered entries) for every browsable playlist.
    pub playlists: Vec<(String, Vec<UndoEntry>)>,
    /// Likes the sync sent up, appended as they happen.
    #[serde(default)]
    pub pushed_likes: Vec<String>,
}

/// The undo journal, file-backed (`writeText` upstream; the port adds
/// the write-then-rename pattern the sibling stores use).
#[derive(Debug, Default)]
pub struct SyncUndo {
    journal: Option<UndoJournal>,
    path: PathBuf,
}

impl SyncUndo {
    /// Open (or start) the journal under the app data dir.
    pub fn open(dir: &Path) -> Self {
        let path = dir.join(UNDO_FILE);
        let journal = fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok());
        SyncUndo { journal, path }
    }

    /// `canUndo` (SyncUndo.kt:39).
    pub fn can_undo(&self) -> bool {
        self.journal.is_some()
    }

    /// `begin(context, database)` (SyncUndo.kt:42-57) — replace the
    /// saved state with what the database holds right now, before a
    /// sync touches it.
    pub fn begin(&mut self, db: &dyn crate::sync::engine::SyncDb) {
        let journal = UndoJournal {
            liked_songs: db.liked_songs(),
            library_songs: db.library_songs(),
            playlists: db
                .playlists()
                .into_iter()
                .map(|playlist| {
                    let entries: Vec<UndoEntry> = db
                        .playlist_songs(&playlist.id)
                        .into_iter()
                        .map(|entry| UndoEntry {
                            song_id: entry.song_id,
                            set_video_id: entry.set_video_id,
                        })
                        .collect();
                    (playlist.id, entries)
                })
                .collect(),
            pushed_likes: Vec::new(),
        };
        self.journal = Some(journal);
        self.write();
    }

    /// `likedBefore` (SyncUndo.kt:60-63) — the likes the app held
    /// right before the last full sync, with their dates (the
    /// lost-likes recovery input).
    pub fn liked_before(&self) -> Vec<(String, Option<i64>)> {
        self.journal
            .as_ref()
            .map(|journal| journal.liked_songs.clone())
            .unwrap_or_default()
    }

    /// `recordPushedLikes` (SyncUndo.kt:66-72) — likes the sync sent
    /// up, so an undo can take them off again.
    pub fn record_pushed_likes(&mut self, ids: &[String]) {
        if ids.is_empty() {
            return;
        }
        if let Some(journal) = &mut self.journal {
            journal.pushed_likes.extend(ids.iter().cloned());
            self.write();
        }
    }

    /// `undo` (SyncUndo.kt:75-88) — put likes, library and playlist
    /// songs back; return the count of YouTube unlikes that failed
    /// (`unlike_on_youtube` is the `YouTube.likeVideo(id, false)`
    /// callback; the hold-flag write is the caller's — the settings
    /// store is not reachable from here on purpose, see the module
    /// docs of `gates`).
    pub fn undo(
        &mut self,
        db: &dyn crate::sync::engine::SyncDb,
        mut unlike_on_youtube: impl FnMut(&str) -> bool,
    ) -> UndoOutcome {
        let Some(journal) = self.journal.take() else {
            return UndoOutcome::default();
        };
        self.delete();
        let failed_unlikes = journal
            .pushed_likes
            .iter()
            .filter(|id| !unlike_on_youtube(id))
            .count();
        let restored = restore_local(db, &journal);
        UndoOutcome {
            failed_unlikes,
            restored,
        }
    }

    fn write(&self) {
        let Some(journal) = &self.journal else {
            return;
        };
        let Ok(text) = serde_json::to_string(journal) else {
            return;
        };
        let temp = self.path.with_extension("json.tmp");
        if fs::write(&temp, &text).is_ok() {
            let _ = fs::rename(&temp, &self.path);
        }
    }

    fn delete(&self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// What one undo did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct UndoOutcome {
    /// Pushed likes that could not be unliked on YouTube.
    pub failed_unlikes: usize,
    /// Rows the restore wrote (likes + library + playlists).
    pub restored: usize,
}

/// `restoreLocal` (SyncUndo.kt:90-130) — diff the journal against the
/// database and write back only what differs; playlists the undo knows
/// but the database no longer has are skipped, and so are entries
/// whose song no longer exists.
fn restore_local(db: &dyn crate::sync::engine::SyncDb, journal: &UndoJournal) -> usize {
    let mut writes = 0usize;

    // current state as maps (likedSongsByNameAsc / songsByNameAsc reads)
    let current_likes: std::collections::HashMap<String, Option<i64>> =
        db.liked_songs().into_iter().collect();
    let current_library: std::collections::HashMap<String, i64> =
        db.library_songs().into_iter().collect();

    // likes: un-like what the snapshot did not have (SyncUndo.kt:92-94)
    for id in current_likes.keys() {
        if !journal
            .liked_songs
            .iter()
            .any(|(journal_id, _)| journal_id == id)
        {
            db.set_liked(id, false, None);
            writes += 1;
        }
    }
    // ...and re-like what it did, with the snapshotted date
    // (SyncUndo.kt:95-99)
    for (id, date) in &journal.liked_songs {
        if !db.song_exists(id) {
            continue;
        }
        if current_likes.get(id) != Some(date) {
            db.set_liked(id, true, *date);
            writes += 1;
        }
    }

    // library: take out what the snapshot did not have, put back what
    // it did, with the date (SyncUndo.kt:101-109)
    for id in current_library.keys() {
        if !journal
            .library_songs
            .iter()
            .any(|(journal_id, _)| journal_id == id)
        {
            db.set_in_library(id, None);
            writes += 1;
        }
    }
    for (id, date) in &journal.library_songs {
        if !db.song_exists(id) {
            continue;
        }
        if current_library.get(id) != Some(date) {
            db.set_in_library(id, Some(*date));
            writes += 1;
        }
    }

    // playlists: rewrite only where the ordered contents differ, and
    // only with entries whose song still exists (SyncUndo.kt:111-129)
    for (playlist_id, entries) in &journal.playlists {
        if !db.playlist_exists(playlist_id) {
            continue;
        }
        let usable: Vec<crate::sync::engine::PlaylistMapEntry> = entries
            .iter()
            .filter(|entry| db.song_exists(&entry.song_id))
            .map(|entry| crate::sync::engine::PlaylistMapEntry {
                song_id: entry.song_id.clone(),
                position: 0,
                set_video_id: entry.set_video_id.clone(),
            })
            .collect();
        let current = db.playlist_songs(playlist_id);
        let differs = current.len() != usable.len()
            || current.iter().zip(&usable).any(|(now, want)| {
                now.song_id != want.song_id || now.set_video_id != want.set_video_id
            });
        if differs {
            db.replace_playlist_songs(playlist_id, &usable);
            writes += 1;
        }
    }

    writes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::engine::{PlaylistMapEntry, PlaylistRef, SyncDb};
    use std::sync::Mutex;

    /// An in-memory database double with the exact fields the restore
    /// touches.
    #[derive(Default)]
    struct Db {
        songs: Vec<String>,
        liked: Mutex<Vec<(String, bool, Option<i64>)>>,
        library: Mutex<Vec<(String, Option<i64>)>>,
        playlists: Mutex<Vec<PlaylistRef>>,
        playlist_songs: Mutex<Vec<(String, Vec<PlaylistMapEntry>)>>,
    }

    impl Db {
        fn with_songs(songs: &[&str]) -> Self {
            Db {
                songs: songs.iter().map(|id| id.to_string()).collect(),
                ..Db::default()
            }
        }

        fn like(&self, id: &str, date: Option<i64>) {
            self.liked.lock().unwrap().push((id.into(), true, date));
        }

        fn add_playlist(&self, id: &str, browse: &str, entries: &[(&str, &str)]) {
            self.playlists.lock().unwrap().push(PlaylistRef {
                id: id.into(),
                name: id.into(),
                browse_id: browse.into(),
            });
            let entries: Vec<PlaylistMapEntry> = entries
                .iter()
                .enumerate()
                .map(|(position, (song, set_video))| PlaylistMapEntry {
                    song_id: song.to_string(),
                    position: position as i32,
                    set_video_id: if set_video.is_empty() {
                        None
                    } else {
                        Some(set_video.to_string())
                    },
                })
                .collect();
            self.playlist_songs
                .lock()
                .unwrap()
                .push((id.into(), entries));
        }
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
            self.library
                .lock()
                .unwrap()
                .iter()
                .filter_map(|(id, date)| date.map(|date| (id.clone(), date)))
                .collect()
        }
        fn playlists(&self) -> Vec<PlaylistRef> {
            self.playlists.lock().unwrap().clone()
        }
        fn playlist_songs(&self, playlist_id: &str) -> Vec<PlaylistMapEntry> {
            self.playlist_songs
                .lock()
                .unwrap()
                .iter()
                .find(|(id, _)| id == playlist_id)
                .map(|(_, entries)| entries.clone())
                .unwrap_or_default()
        }
        fn playlist_exists(&self, playlist_id: &str) -> bool {
            self.playlists
                .lock()
                .unwrap()
                .iter()
                .any(|playlist| playlist.id == playlist_id)
        }
        fn song_artist(&self, _video_id: &str) -> Option<String> {
            None
        }
        fn song_exists(&self, video_id: &str) -> bool {
            self.songs.iter().any(|id| id == video_id)
        }
        fn unliked_songs(&self, _ids: &[String]) -> Vec<crate::sync::state::SongRef> {
            Vec::new()
        }
        fn likes_from_imports(&self) -> Vec<(String, Option<i64>)> {
            Vec::new()
        }
        fn imported_song_ids_except(&self, _source: &str) -> std::collections::HashSet<String> {
            Default::default()
        }
        fn import_session_song_ids(&self) -> std::collections::HashSet<String> {
            Default::default()
        }
        fn set_liked(&self, video_id: &str, liked: bool, liked_date: Option<i64>) {
            let mut all = self.liked.lock().unwrap();
            match all.iter_mut().find(|(id, _, _)| id == video_id) {
                Some(entry) => {
                    entry.1 = liked;
                    entry.2 = liked_date;
                }
                None => all.push((video_id.to_owned(), liked, liked_date)),
            }
        }
        fn set_in_library(&self, video_id: &str, date: Option<i64>) {
            let mut all = self.library.lock().unwrap();
            match all.iter_mut().find(|(id, _)| id == video_id) {
                Some(entry) => entry.1 = date,
                None => all.push((video_id.to_owned(), date)),
            }
        }
        fn replace_playlist_songs(&self, playlist_id: &str, entries: &[PlaylistMapEntry]) {
            let mut all = self.playlist_songs.lock().unwrap();
            if let Some(entry) = all.iter_mut().find(|(id, _)| id == playlist_id) {
                entry.1 = entries
                    .iter()
                    .enumerate()
                    .map(|(position, entry)| PlaylistMapEntry {
                        song_id: entry.song_id.clone(),
                        position: position as i32,
                        set_video_id: entry.set_video_id.clone(),
                    })
                    .collect();
            }
        }
    }

    /// SyncUndo.kt:42-57 + 90-130 — begin snapshots, the sync mutates,
    /// undo restores likes/library/playlists.
    #[test]
    fn begin_then_undo_restores_the_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let mut undo = SyncUndo::open(dir.path());
        assert!(!undo.can_undo());

        let db = Db::with_songs(&["s1", "s2", "s3", "s4", "s5"]);
        db.like("s1", Some(100));
        db.like("s2", Some(200));
        db.set_in_library("s1", Some(300));
        db.set_in_library("s3", Some(310));
        db.add_playlist("p1", "VL1", &[("s1", "sv1"), ("s2", "")]);
        // the sync "runs": snapshot, then mutate everything
        undo.begin(&db);
        let snapshot = undo.liked_before();
        assert_eq!(snapshot.len(), 2);

        db.set_liked("s2", false, None); // a sync unliked s2
        db.set_liked("s4", true, Some(999)); // a sync liked s4
        db.set_in_library("s3", None); // left the library
        db.set_in_library("s5", Some(999)); // joined the library
        db.replace_playlist_songs("p1", &[]); // the playlist was emptied

        // undo (no YouTube side): everything above goes back
        let outcome = undo.undo(&db, |_| true);
        assert_eq!(outcome.failed_unlikes, 0);
        assert!(outcome.restored >= 4, "at least the four row writes");
        assert_eq!(
            db.liked_songs(),
            vec![("s1".into(), Some(100)), ("s2".into(), Some(200))],
            "s4's sync-time like is undone, s1/s2 keep their snapshotted dates"
        );
        assert_eq!(
            db.library_songs(),
            vec![("s1".into(), 300), ("s3".into(), 310)]
        );
        let songs = db.playlist_songs("p1");
        assert_eq!(
            songs
                .iter()
                .map(|entry| (entry.song_id.as_str(), entry.set_video_id.clone()))
                .collect::<Vec<_>>(),
            vec![("s1", Some("sv1".into())), ("s2", None)]
        );
        assert!(!undo.can_undo(), "undo consumes the journal");
        // a second undo is a no-op
        assert_eq!(undo.undo(&db, |_| true), UndoOutcome::default());
    }

    /// SyncUndo.kt:80-84 — the withYouTube leg unlikes the pushed likes
    /// and counts the failures.
    #[test]
    fn undo_counts_failed_youtube_unlikes() {
        let dir = tempfile::tempdir().unwrap();
        let mut undo = SyncUndo::open(dir.path());
        let db = Db::with_songs(&["s1"]);
        undo.begin(&db);
        undo.record_pushed_likes(&["p1".into(), "p2".into(), "p3".into()]);
        let mut unliked = Vec::new();
        let outcome = undo.undo(&db, |id| {
            unliked.push(id.to_string());
            id != "p2" // p2 fails
        });
        assert_eq!(unliked, vec!["p1", "p2", "p3"]);
        assert_eq!(outcome.failed_unlikes, 1);
    }

    /// The journal survives a restart (write-then-rename).
    #[test]
    fn journal_persists_and_reloads() {
        let dir = tempfile::tempdir().unwrap();
        let mut undo = SyncUndo::open(dir.path());
        let db = Db::with_songs(&["s1"]);
        db.like("s1", Some(5));
        undo.begin(&db);
        undo.record_pushed_likes(&["pushed".into()]);

        let reloaded = SyncUndo::open(dir.path());
        assert!(reloaded.can_undo());
        assert_eq!(reloaded.liked_before(), vec![("s1".into(), Some(5))]);
    }
}
