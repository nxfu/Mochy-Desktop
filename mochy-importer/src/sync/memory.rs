//! `YouTubeLikesMemory` — `importer/YouTubeLikesMemory.kt`.
//!
//! What the liked-songs sync keeps between runs: the ids YouTube's
//! liked list held at the last FULL read (`seen`), and every like a
//! sync took off with the date it had (`removed`) — the fuel for the
//! lost-likes recovery and the shield against a short list wiping the
//! library ("the sync used to unlike every song missing from
//! youtube's list, so one short list wiped hundreds of likes",
//! SyncUtils.kt:519-520).
//!
//! Port notes: timestamps are epoch-ms `i64` (the Phase 03 entity
//! convention; upstream serializes `LocalDateTime.toString()` — our
//! file is ours, `ignoreUnknownKeys` tolerance kept by decoding
//! defensively). The `exclusive` run mutex is the caller's job on
//! desktop (the engine holds the store behind its own lock; the
//! one-liked-sync-at-a-time rule is documented on
//! [`crate::sync::engine::SyncEngine`]).

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// `MaxRemoved` (YouTubeLikesMemory.kt:17) — enough for a whole library
/// taken off by one bad sync, oldest drop first.
pub const MAX_REMOVED: usize = 20_000;
/// `MaxUnlikesAtOnce` (YouTubeLikesMemory.kt:19) — more unlikes than
/// this in one sync, AND over a tenth of the list, is YouTube sending a
/// short list rather than real unlikes.
pub const MAX_UNLIKES_AT_ONCE: usize = 20;

/// The file name (YouTubeLikesMemory.kt:15).
const MEMORY_FILE: &str = "yt_likes_memory.json";

#[derive(Debug, Default, Serialize, Deserialize)]
struct MemoryFile {
    /// The liked ids YouTube listed at the last full read; `None`
    /// before the first one (upstream `seen: List<String>?`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    seen: Option<Vec<String>>,
    /// (song id, liked date at removal) — insertion-ordered.
    #[serde(default)]
    removed: Vec<(String, Option<i64>)>,
}

/// The memory store, file-backed with the upstream write-then-rename
/// atomicity.
#[derive(Debug, Default)]
pub struct LikesMemory {
    memory: MemoryFile,
    path: PathBuf,
}

impl LikesMemory {
    /// Open (or start) the memory under the app data dir; a missing or
    /// corrupt file reads as empty (upstream `runCatching ... ?: Memory()`).
    pub fn open(dir: &Path) -> Self {
        let path = dir.join(MEMORY_FILE);
        let memory = fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        LikesMemory { memory, path }
    }

    /// `seen(context)` — what YouTube's liked list held at the last
    /// full read; `None` before the first one.
    pub fn seen(&self) -> Option<HashSet<String>> {
        self.memory
            .seen
            .as_ref()
            .map(|ids| ids.iter().cloned().collect())
    }

    /// `rememberSeen(context, ids)`.
    pub fn remember_seen(&mut self, ids: HashSet<String>) {
        self.memory.seen = Some(ids.into_iter().collect());
        self.write();
    }

    /// `recordRemoved(context, songs)` (YouTubeLikesMemory.kt:53-60) —
    /// re-inserted entries move to the end; the journal is trimmed to
    /// the last [`MAX_REMOVED`]. Ordered input: upstream receives a
    /// `LinkedHashMap` built from an ordered drop list, and the trim
    /// keeps the NEWEST by that order.
    pub fn record_removed(&mut self, songs: &[(String, Option<i64>)]) {
        if songs.is_empty() {
            return;
        }
        let ids: HashSet<&String> = songs.iter().map(|(id, _)| id).collect();
        self.memory.removed.retain(|(id, _)| !ids.contains(id));
        for (id, date) in songs {
            self.memory.removed.push((id.clone(), *date));
        }
        let excess = self.memory.removed.len().saturating_sub(MAX_REMOVED);
        self.memory.removed.drain(..excess);
        self.write();
    }

    /// `removed(context)` — the removals with their dates.
    pub fn removed(&self) -> HashMap<String, Option<i64>> {
        self.memory.removed.iter().cloned().collect()
    }

    /// `forget(context, ids)` (YouTubeLikesMemory.kt:66-74) — after a
    /// like comes back by hand it counts as made here, so the next sync
    /// cannot count it as one YouTube dropped.
    pub fn forget(&mut self, ids: &[String]) {
        if ids.is_empty() {
            return;
        }
        let gone: HashSet<&String> = ids.iter().collect();
        if let Some(seen) = &mut self.memory.seen {
            seen.retain(|id| !gone.contains(id));
        }
        self.memory.removed.retain(|(id, _)| !gone.contains(id));
        self.write();
    }

    /// `droppedOnYouTube` (YouTubeLikesMemory.kt:31-40) — the likes
    /// YouTube SURELY dropped, or `None` when this list cannot tell
    /// (cut short, or a mass drop that smells like a YouTube hiccup).
    /// Pure: Phase 22's pull arm and the tests drive it directly.
    pub fn dropped_on_youtube(
        liked_here: &[String],
        seen: Option<&HashSet<String>>,
        remote: &HashSet<String>,
        whole_list: bool,
        keep: &HashSet<String>,
    ) -> Option<HashSet<String>> {
        // a short list says nothing (the caller checked `wholeList`)
        if !whole_list {
            return None;
        }
        // before the first full read nothing counts as listed, so the
        // first sync after an update takes nothing off (an EMPTY set —
        // "this list can tell, and nothing was dropped", not null)
        let seen = match seen {
            Some(seen) => seen,
            None => return Some(HashSet::new()),
        };
        // a song YouTube never listed was liked here (an import, a
        // file, a like that never went up), so it stays
        let dropped: HashSet<String> = liked_here
            .iter()
            .filter(|id| seen.contains(*id) && !remote.contains(*id) && !keep.contains(*id))
            .cloned()
            .collect();
        if !dropped.is_empty() && remote.is_empty() {
            return None;
        }
        if dropped.len() > MAX_UNLIKES_AT_ONCE && dropped.len() * 10 > seen.len() {
            return None;
        }
        Some(dropped)
    }

    /// The upstream write (YouTubeLikesMemory.kt:82-92):
    /// write-then-rename so a crash mid-write never leaves half a list.
    fn write(&self) {
        let Ok(text) = serde_json::to_string(&self.memory) else {
            return;
        };
        let temp = self.path.with_extension("json.tmp");
        if fs::write(&temp, &text).is_ok() {
            let _ = fs::rename(&temp, &self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(values: &[&str]) -> HashSet<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    /// YouTubeLikesMemory.kt:31-40 — the whole droppedOnYouTube truth
    /// table, upstream comment for upstream comment.
    #[test]
    fn dropped_on_youtube_matches_upstream() {
        let liked_here = vec!["a".into(), "b".into(), "c".into(), "d".into()];

        // a cut-short list cannot say anything
        assert_eq!(
            LikesMemory::dropped_on_youtube(
                &liked_here,
                Some(&ids(&["a", "b", "c", "d"])),
                &ids(&["a"]),
                false,
                &ids(&[])
            ),
            None,
            "!wholeList -> null"
        );

        // no memory yet (first sync): nothing counts as dropped — an
        // EMPTY set, not null (YouTubeLikesMemory.kt:34)
        assert_eq!(
            LikesMemory::dropped_on_youtube(&liked_here, None, &ids(&["a"]), true, &ids(&[])),
            Some(ids(&[])),
            "seen == null -> empty set, not null"
        );

        // seen a+b+c+d, remote holds a only, keep empty:
        // b, c, d were listed and are gone -> dropped
        assert_eq!(
            LikesMemory::dropped_on_youtube(
                &liked_here,
                Some(&ids(&["a", "b", "c", "d"])),
                &ids(&["a"]),
                true,
                &ids(&[])
            ),
            Some(ids(&["b", "c", "d"]))
        );

        // a mass drop with an EMPTY remote is a YouTube hiccup, not real
        assert_eq!(
            LikesMemory::dropped_on_youtube(
                &liked_here,
                Some(&ids(&["a", "b", "c", "d"])),
                &ids(&[]),
                true,
                &ids(&[])
            ),
            None,
            "dropped non-empty && remote empty -> null"
        );

        // keep shields an id (the pushed/kept likes)
        assert_eq!(
            LikesMemory::dropped_on_youtube(
                &liked_here,
                Some(&ids(&["a", "b", "c", "d"])),
                &ids(&["a"]),
                true,
                &ids(&["b"])
            ),
            Some(ids(&["c", "d"])),
            "keep set shields"
        );

        // a song never listed by YouTube stays (liked here: an import)
        assert_eq!(
            LikesMemory::dropped_on_youtube(
                &liked_here,
                Some(&ids(&["a", "b"])),
                &ids(&["a"]),
                true,
                &ids(&[])
            ),
            Some(ids(&["b"])),
            "unlisted likes are not drops"
        );

        // 21 drops out of 30 seen (over a tenth, over 20): short list
        let many: Vec<String> = (0..30).map(|index| format!("s{index}")).collect();
        let seen: HashSet<String> = many.iter().cloned().collect();
        let remote: HashSet<String> = many[..9].iter().cloned().collect();
        assert_eq!(
            LikesMemory::dropped_on_youtube(&many, Some(&seen), &remote, true, &ids(&[])),
            None,
            "mass drop over a tenth of the list -> null"
        );

        // 21 drops out of 300 seen (over 20 but under a tenth): real
        let many: Vec<String> = (0..300).map(|index| format!("s{index}")).collect();
        let seen: HashSet<String> = many.iter().cloned().collect();
        let remote: HashSet<String> = many[..279].iter().cloned().collect();
        assert_eq!(
            LikesMemory::dropped_on_youtube(&many, Some(&seen), &remote, true, &ids(&[]))
                .expect("over 20 but under a tenth is real drops")
                .len(),
            21
        );
    }

    /// The store round-trips and reloads (seen / removed / forget /
    /// trim).
    #[test]
    fn memory_store_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let mut memory = LikesMemory::open(dir.path());
        assert_eq!(memory.seen(), None, "nothing before the first full read");

        memory.remember_seen(ids(&["a", "b"]));
        let removed = vec![("b".to_string(), Some(1_700_000_000_000))];
        memory.record_removed(&removed);
        memory.forget(&["a".into()]);

        let reloaded = LikesMemory::open(dir.path());
        assert_eq!(
            reloaded.seen(),
            Some(ids(&["b"])),
            "forget removed a from seen"
        );
        assert_eq!(
            reloaded.removed(),
            HashMap::from([("b".to_string(), Some(1_700_000_000_000))])
        );

        // re-recording moves the entry to the end and updates the date
        let mut memory = reloaded;
        let again = vec![("b".to_string(), Some(1_800_000_000_000))];
        memory.record_removed(&again);
        assert_eq!(memory.removed()["b"], Some(1_800_000_000_000));
    }

    /// The removal journal trims to the last MAX_REMOVED, oldest first.
    #[test]
    fn removed_journal_trims_to_max() {
        let dir = tempfile::tempdir().unwrap();
        let mut memory = LikesMemory::open(dir.path());
        let all: Vec<(String, Option<i64>)> = (0..MAX_REMOVED + 50)
            .map(|index| (format!("s{index}"), Some(index as i64)))
            .collect();
        memory.record_removed(&all);
        assert_eq!(memory.removed().len(), MAX_REMOVED);
        // the oldest 50 are gone, the newest MAX_REMOVED stay
        assert!(!memory.removed().contains_key("s0"));
        assert!(
            memory
                .removed()
                .contains_key(&format!("s{}", MAX_REMOVED + 49))
        );
    }

    /// A corrupt file reads as empty memory (upstream tolerance).
    #[test]
    fn corrupt_memory_reads_empty() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(MEMORY_FILE), "{oops").unwrap();
        let memory = LikesMemory::open(dir.path());
        assert_eq!(memory.seen(), None);
        assert!(memory.removed().is_empty());
    }
}
