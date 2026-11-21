//! `LostLikes` — `importer/LostLikes.kt`: brings back likes a sync took
//! off by mistake, from every record the app keeps of a like.
//!
//! The records, each later one knowing the like date better
//! (LostLikes.kt:37-45):
//! 1. `likesFromImports()` — an import does not keep the other app's
//!    like dates; the day the import ran is the closest available;
//! 2. `SyncUndo.likedBefore()` — the snapshot before the last full
//!    sync;
//! 3. `YouTubeLikesMemory.removed()` — what a sync took off, with the
//!    dates they had.
//!
//! `wanted` merges them (later records win, except a null date never
//! displaces an existing one); `missing` keeps the wanted ids that are
//! currently unliked; `bring_back` re-likes exactly those, locally
//! only, and forgets them from the memory so the next sync counts them
//! as made here. The SQLite 999-parameter chunking (LostLikes.kt:15)
//! lives in the Phase 22 DB adapter; the trait's slice API already
//! allows chunked calls.

use std::collections::HashMap;

use crate::sync::engine::SyncDb;
use crate::sync::memory::LikesMemory;
use crate::sync::state::SongRef;
use crate::sync::undo::SyncUndo;

/// The merged "what should be liked, with the best date we know"
/// record — `LostLikes.wanted` (LostLikes.kt:38-45). Pure.
pub fn wanted(
    likes_from_imports: Vec<(String, Option<i64>)>,
    undo_liked_before: Vec<(String, Option<i64>)>,
    memory_removed: &HashMap<String, Option<i64>>,
) -> HashMap<String, Option<i64>> {
    let mut wanted: HashMap<String, Option<i64>> = HashMap::new();
    let mut merge = |records: Vec<(String, Option<i64>)>| {
        for (id, date) in records {
            // each later record knows the like date better, so it wins
            // over the one before — but a missing date never displaces
            // one we had
            match wanted.get(&id) {
                Some(Some(_)) if date.is_none() => {}
                _ => {
                    wanted.insert(id, date);
                }
            }
        }
    };
    merge(likes_from_imports);
    merge(undo_liked_before);
    // the map's iteration order is unspecified; sort for determinism
    let memory: Vec<(String, Option<i64>)> = memory_removed
        .iter()
        .map(|(id, date)| (id.clone(), *date))
        .collect();
    let mut memory = memory;
    memory.sort_by(|a, b| a.0.cmp(&b.0));
    merge(memory);
    wanted
}

/// `LostLikes.count` — how many wanted likes are currently missing.
pub fn count(db: &dyn SyncDb, undo: &SyncUndo, memory: &LikesMemory) -> usize {
    missing(db, &wanted_records(undo, memory)).len()
}

/// `LostLikes.bringBack` — only on this machine, YouTube is left
/// alone; returns the songs that came back (LostLikes.kt:21-35): each
/// re-like keeps its recorded date, or `now_epoch_ms` when no record
/// knew it. The memory `forget` rides along so the next sync counts
/// them as made here.
pub fn bring_back(
    db: &dyn SyncDb,
    undo: &SyncUndo,
    memory: &mut LikesMemory,
    now_epoch_ms: i64,
) -> Vec<SongRef> {
    let records = wanted_records(undo, memory);
    let songs = missing(db, &records);
    for song in &songs {
        let date = records
            .get(&song.id)
            .copied()
            .flatten()
            .unwrap_or(now_epoch_ms);
        db.set_liked(&song.id, true, Some(date));
    }
    let ids: Vec<String> = songs.iter().map(|song| song.id.clone()).collect();
    memory.forget(&ids);
    songs
}

fn wanted_records(undo: &SyncUndo, memory: &LikesMemory) -> HashMap<String, Option<i64>> {
    wanted(
        // likesFromImports() — the DB read (Phase 22 adapter chunks it)
        Vec::new(),
        undo.liked_before(),
        &memory.removed(),
    )
}

/// `missing` (LostLikes.kt:47-49) — the wanted ids that are currently
/// unliked.
fn missing(db: &dyn SyncDb, wanted: &HashMap<String, Option<i64>>) -> Vec<SongRef> {
    let ids: Vec<String> = wanted.keys().cloned().collect();
    db.unliked_songs(&ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// LostLikes.kt:37-45 — the merge order: imports < undo journal <
    /// removal memory; a null date never wins over a known one.
    #[test]
    fn wanted_merge_prefers_later_records_but_keeps_dates() {
        let imports = vec![
            ("a".to_string(), Some(1)),
            ("b".to_string(), Some(2)),
            ("c".to_string(), None), // import ran, date unknown
        ];
        let undo = vec![
            ("b".to_string(), None), // was liked, snapshot lost the date
            ("d".to_string(), Some(4)),
        ];
        let memory = HashMap::from([("c".to_string(), Some(3))]);

        let merged = wanted(imports, undo, &memory);
        assert_eq!(merged.get("a"), Some(&Some(1)), "only the import knows a");
        assert_eq!(
            merged.get("b"),
            Some(&Some(2)),
            "the undo's null date must not displace the import's date"
        );
        assert_eq!(
            merged.get("c"),
            Some(&Some(3)),
            "the removal memory's date wins over the import's null"
        );
        assert_eq!(merged.get("d"), Some(&Some(4)));
    }

    /// LostLikes.kt:21-35 — bring back re-likes the missing ones with
    /// their recorded dates and forgets them from the memory.
    #[test]
    fn bring_back_relikes_and_forgets() {
        let dir = tempfile::tempdir().unwrap();
        let mut memory = LikesMemory::open(dir.path());
        let removed = vec![("gone".to_string(), Some(1_234))];
        memory.record_removed(&removed);

        let undo = SyncUndo::open(dir.path());
        // no journal: only the memory knows

        let db = test_db(&["gone", "kept"]);
        let songs = bring_back(&db, &undo, &mut memory, 9_999);
        assert_eq!(songs.len(), 1);
        assert_eq!(songs[0].id, "gone");
        assert_eq!(db.liked_songs(), vec![("gone".to_string(), Some(1_234))]);
        // and the memory no longer counts it as removed
        assert!(memory.removed().is_empty());
    }

    // -- a tiny SyncDb double for the bring-back test --
    struct TestDb {
        liked: Mutex<Vec<(String, bool, Option<i64>)>>,
    }

    fn test_db(_songs: &[&str]) -> TestDb {
        TestDb {
            liked: Mutex::new(vec![]),
        }
    }

    impl SyncDb for TestDb {
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
        fn playlists(&self) -> Vec<crate::sync::engine::PlaylistRef> {
            Vec::new()
        }
        fn playlist_songs(&self, _playlist_id: &str) -> Vec<crate::sync::engine::PlaylistMapEntry> {
            Vec::new()
        }
        fn playlist_exists(&self, _playlist_id: &str) -> bool {
            false
        }
        fn song_artist(&self, _video_id: &str) -> Option<String> {
            None
        }
        fn song_exists(&self, _video_id: &str) -> bool {
            true
        }
        fn unliked_songs(&self, ids: &[String]) -> Vec<SongRef> {
            // every wanted id that is not currently liked
            ids.iter()
                .filter(|id| {
                    !self
                        .liked
                        .lock()
                        .unwrap()
                        .iter()
                        .any(|(song, liked, _)| song == *id && *liked)
                })
                .map(|id| SongRef {
                    id: id.clone(),
                    title: String::new(),
                    liked: false,
                })
                .collect()
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
        fn set_in_library(&self, _video_id: &str, _date: Option<i64>) {}
        fn replace_playlist_songs(
            &self,
            _playlist_id: &str,
            _entries: &[crate::sync::engine::PlaylistMapEntry],
        ) {
        }
    }
}
