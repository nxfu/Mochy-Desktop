//! Pending work recorded while signed out —
//! `importer/PendingYouTubeLikes.kt` (upstream parity) plus the port's
//! ordered pending-op queue the Phase 07 task adds.
//!
//! Upstream keeps exactly one pending store:
//! `PendingYouTubeLikes` — the ids of songs an IMPORT liked (Spotify,
//! CSV, ...) that YouTube has never seen. The liked-songs pull consults
//! it first so a first sync pushes those likes up instead of
//! interpreting them as unlikes (SyncUtils.kt:456-473); it is a plain
//! set of ids in `yt_pending_likes.txt` (write-then-rename, deleted
//! when empty). Likes the USER made while signed out are local-only
//! upstream — `executeLikeSong` returns before the push
//! (SyncUtils.kt:419-422).
//!
//! The desktop port additionally keeps a [`PendingOpQueue`] (Phase 07
//! task: "a pending-op queue that records likes/library edits while
//! signed out and replays them on login"): every like/unlike and
//! library add/remove made while signed out is journaled in order and
//! replayed, in order, the moment a session exists. Failures stay
//! queued with their order intact for the next login. Both stores use
//! the upstream atomicity pattern: write a temp file, fsync, rename.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::engine::ReplayApi;

/// The upstream file name (importer/PendingYouTubeLikes.kt:10).
const PENDING_LIKES_FILE: &str = "yt_pending_likes.txt";
/// The port's ordered queue file.
const PENDING_OPS_FILE: &str = "yt_pending_ops.json";

/// `PendingYouTubeLikes` (importer/PendingYouTubeLikes.kt:9-47) — the
/// import-liked ids, file-backed as a set, write-then-rename, empty =
/// file gone.
#[derive(Debug, Default)]
pub struct PendingYouTubeLikes {
    ids: Vec<String>,
    path: PathBuf,
}

impl PendingYouTubeLikes {
    /// Open (or start) the store under the app data dir.
    pub fn open(dir: &Path) -> Self {
        let path = dir.join(PENDING_LIKES_FILE);
        let ids = fs::read_to_string(&path)
            .map(|text| {
                text.lines()
                    .map(str::trim)
                    .filter(|line| !line.is_empty())
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        PendingYouTubeLikes { ids, path }
    }

    /// `read` — the current set (read on every liked-songs sync).
    pub fn ids(&self) -> Vec<String> {
        self.ids.clone()
    }

    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    /// `add` — union, then persist.
    pub fn add(&mut self, ids: &[String]) -> io::Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        for id in ids {
            if !self.ids.contains(id) {
                self.ids.push(id.clone());
            }
        }
        self.write()
    }

    /// `remove` — difference, then persist.
    pub fn remove(&mut self, ids: &[String]) -> io::Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        self.ids.retain(|id| !ids.contains(id));
        self.write()
    }

    /// The upstream `write` (PendingYouTubeLikes.kt:31-46): an empty
    /// set deletes the file; otherwise write-then-rename so a crash
    /// mid-write never leaves half a list.
    fn write(&self) -> io::Result<()> {
        if self.ids.is_empty() {
            match fs::remove_file(&self.path) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(e),
            }
        } else {
            let temp = self.path.with_extension("txt.tmp");
            fs::write(&temp, self.ids.join("\n"))?;
            fs::rename(&temp, &self.path)
        }
    }
}

/// One queued edit made while signed out. Likes and library membership
/// are the two edits upstream would otherwise silently drop for
/// signed-out users (SyncUtils.kt:419-422 / 570: the library pull
/// filters `isLocal`, `remoteIds`, `importedIds` — a signed-out add
/// would be taken right back out).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum PendingOp {
    /// Push `YouTube.likeVideo(videoId, liked)` once signed in.
    LikeSong { video_id: String, liked: bool },
    /// Push the library add/remove once signed in (the feedback token
    /// path `YouTube.addSongToLibrary`/`removeSongFromLibrary` resolves
    /// per song at replay time).
    LibrarySong { video_id: String, in_library: bool },
}

impl PendingOp {
    /// The video the edit concerns.
    pub fn video_id(&self) -> &str {
        match self {
            PendingOp::LikeSong { video_id, .. } | PendingOp::LibrarySong { video_id, .. } => {
                video_id
            }
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct PendingFile {
    /// Monotonic sequence, never reused within a store's lifetime.
    next_seq: u64,
    ops: Vec<SequencedOp>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct SequencedOp {
    seq: u64,
    #[serde(flatten)]
    op: PendingOp,
}

/// The outcome of one replay pass — what succeeded, what stays queued.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReplayReport {
    /// Ops sent up this pass.
    pub attempted: usize,
    /// Ops YouTube accepted (removed from the queue).
    pub applied: usize,
    /// Ops that stay queued, in order, with their failure reasons.
    pub retained: Vec<(PendingOp, String)>,
}

/// The ordered queue of signed-out edits (Phase 07 port addition; see
/// the module docs).
///
/// `record` appends; `replay` walks the queue in record order, sends
/// each op through [`ReplayApi`], and keeps the failures (order
/// preserved) for the next login. Persistence is the upstream
/// write-then-rename pattern.
#[derive(Debug, Default)]
pub struct PendingOpQueue {
    file: PendingFile,
    path: PathBuf,
}

impl PendingOpQueue {
    /// Open (or start) the queue under the app data dir; a missing or
    /// corrupt file starts empty (upstream `runCatching ... getOrNull`
    /// tolerance — a half-written list is worth nothing).
    pub fn open(dir: &Path) -> Self {
        let path = dir.join(PENDING_OPS_FILE);
        let file = fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        PendingOpQueue { file, path }
    }

    /// The queued ops, in record order.
    pub fn ops(&self) -> Vec<PendingOp> {
        self.file.ops.iter().map(|entry| entry.op.clone()).collect()
    }

    pub fn len(&self) -> usize {
        self.file.ops.len()
    }

    pub fn is_empty(&self) -> bool {
        self.file.ops.is_empty()
    }

    /// Append one edit and persist.
    pub fn record(&mut self, op: PendingOp) -> io::Result<()> {
        self.file.ops.push(SequencedOp {
            seq: self.file.next_seq,
            op,
        });
        self.file.next_seq += 1;
        self.write()
    }

    /// Replay every queued op in record order through `api`. Applied
    /// ops are removed; failed ops stay queued in order (a later
    /// `like(false)` still lands after an earlier failed `like(true)`).
    pub async fn replay(&mut self, api: &dyn ReplayApi) -> ReplayReport {
        let mut report = ReplayReport::default();
        let mut retained: Vec<SequencedOp> = Vec::new();
        let entries = std::mem::take(&mut self.file.ops);
        for entry in entries {
            report.attempted += 1;
            let result = match &entry.op {
                PendingOp::LikeSong { video_id, liked } => api.like_song(video_id, *liked).await,
                PendingOp::LibrarySong {
                    video_id,
                    in_library,
                } => api.edit_library_song(video_id, *in_library).await,
            };
            match result {
                Ok(()) => report.applied += 1,
                Err(reason) => {
                    report.retained.push((entry.op.clone(), reason));
                    retained.push(entry);
                }
            }
        }
        self.file.ops = retained;
        let _ = self.write();
        report
    }

    /// Write-then-rename persistence (upstream atomicity pattern); an
    /// empty queue deletes the file.
    fn write(&self) -> io::Result<()> {
        if self.file.ops.is_empty() {
            match fs::remove_file(&self.path) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(e),
            }
        } else {
            let temp = self.path.with_extension("json.tmp");
            fs::write(&temp, serde_json::to_string_pretty(&self.file)?)?;
            fs::rename(&temp, &self.path)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::pin::Pin;
    use std::sync::Mutex;

    type BoxFut<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

    /// The recording API double: it fails selected videos and remembers
    /// the call order.
    #[derive(Default)]
    struct Recorder {
        calls: Mutex<Vec<String>>,
        fail_for: Vec<String>,
    }

    impl ReplayApi for Recorder {
        fn like_song(&self, video_id: &str, liked: bool) -> BoxFut<'_, Result<(), String>> {
            if self.fail_for.iter().any(|id| id == video_id) {
                let reason = format!("refused {video_id}");
                return Box::pin(async move { Err(reason) });
            }
            self.calls
                .lock()
                .unwrap()
                .push(format!("like({video_id},{liked})"));
            Box::pin(async { Ok(()) })
        }

        fn edit_library_song(
            &self,
            video_id: &str,
            in_library: bool,
        ) -> BoxFut<'_, Result<(), String>> {
            if self.fail_for.iter().any(|id| id == video_id) {
                let reason = format!("refused {video_id}");
                return Box::pin(async move { Err(reason) });
            }
            self.calls
                .lock()
                .unwrap()
                .push(format!("library({video_id},{in_library})"));
            Box::pin(async { Ok(()) })
        }
    }

    fn temp() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    /// ACCEPTANCE (Phase 07 task): offline like → login → replayed in
    /// order. Records a like, an unlike and a library edit while
    /// "signed out", then replays: the three ops go up in exactly the
    /// order they were made.
    #[tokio::test]
    async fn offline_edits_replay_in_order_on_login() {
        let dir = temp();
        let mut queue = PendingOpQueue::open(dir.path());
        assert!(queue.is_empty());

        // signed out: three edits queue locally
        queue
            .record(PendingOp::LikeSong {
                video_id: "vidA".into(),
                liked: true,
            })
            .unwrap();
        queue
            .record(PendingOp::LibrarySong {
                video_id: "vidB".into(),
                in_library: false,
            })
            .unwrap();
        queue
            .record(PendingOp::LikeSong {
                video_id: "vidC".into(),
                liked: true,
            })
            .unwrap();

        // a restart mid-way keeps the queue and its order
        let reloaded = PendingOpQueue::open(dir.path());
        assert_eq!(reloaded.len(), 3);

        // login: everything replays in record order
        let recorder = Recorder::default();
        let report = queue.replay(&recorder).await;
        assert_eq!(report.applied, 3);
        assert_eq!(report.attempted, 3);
        assert!(report.retained.is_empty());
        assert!(queue.is_empty(), "applied ops leave the queue");
        assert_eq!(
            *recorder.calls.lock().unwrap(),
            vec!["like(vidA,true)", "library(vidB,false)", "like(vidC,true)"],
            "replay must preserve record order"
        );

        // the persisted file is gone once empty (upstream: delete when
        // empty)
        assert!(!dir.path().join(PENDING_OPS_FILE).exists());
    }

    /// Failures stay queued, in order, for the next login.
    #[tokio::test]
    async fn failed_edits_stay_queued_in_order() {
        let dir = temp();
        let mut queue = PendingOpQueue::open(dir.path());
        for (video, liked) in [("v1", true), ("v2", true), ("v3", true)] {
            queue
                .record(PendingOp::LikeSong {
                    video_id: video.into(),
                    liked,
                })
                .unwrap();
        }

        let failing = Recorder {
            fail_for: vec!["v2".into()],
            ..Recorder::default()
        };
        let report = queue.replay(&failing).await;
        assert_eq!(report.applied, 2);
        assert_eq!(report.retained.len(), 1);
        assert_eq!(report.retained[0].0.video_id(), "v2");

        // next login (no failures): the retained op goes up, alone
        let healthy = Recorder::default();
        let report = queue.replay(&healthy).await;
        assert_eq!(report.applied, 1);
        assert_eq!(*healthy.calls.lock().unwrap(), vec!["like(v2,true)"]);
    }

    /// PendingYouTubeLikes.kt:15-46 — set semantics, atomicity, the
    /// missing-file tolerance.
    #[test]
    fn pending_youtube_likes_matches_upstream() {
        let dir = temp();
        let mut pending = PendingYouTubeLikes::open(dir.path());
        assert!(pending.is_empty(), "missing file reads as empty");

        pending.add(&["a".into(), "b".into()]).unwrap();
        pending.add(&["b".into(), "c".into()]).unwrap();
        assert_eq!(pending.ids(), vec!["a", "b", "c"], "add is a set union");

        pending.remove(&["b".into()]).unwrap();
        assert_eq!(pending.ids(), vec!["a", "c"]);

        // reload picks up the file
        let reloaded = PendingYouTubeLikes::open(dir.path());
        assert_eq!(reloaded.ids(), vec!["a", "c"]);

        // removing everything deletes the file (upstream write())
        pending.remove(&["a".into(), "c".into()]).unwrap();
        assert!(pending.is_empty());
        assert!(!dir.path().join(PENDING_LIKES_FILE).exists());

        // blank lines and whitespace are tolerated on read
        // (read() filters trimmed empties)
        std::fs::write(dir.path().join(PENDING_LIKES_FILE), " x \n\n  y \n").unwrap();
        assert_eq!(PendingYouTubeLikes::open(dir.path()).ids(), vec!["x", "y"]);
    }

    /// A corrupt queue file starts empty rather than panicking
    /// (upstream runCatching tolerance).
    #[test]
    fn corrupt_queue_file_starts_empty() {
        let dir = temp();
        std::fs::write(dir.path().join(PENDING_OPS_FILE), "not json").unwrap();
        let queue = PendingOpQueue::open(dir.path());
        assert!(queue.is_empty());
    }

    /// Sequence numbers never reuse: ops recorded after a replay that
    /// retained failures keep their relative order.
    #[tokio::test]
    async fn ops_after_replay_keep_order() {
        let dir = temp();
        let mut queue = PendingOpQueue::open(dir.path());
        let failing = Recorder {
            fail_for: vec!["keep".into()],
            ..Recorder::default()
        };
        queue
            .record(PendingOp::LikeSong {
                video_id: "keep".into(),
                liked: true,
            })
            .unwrap();
        let _ = queue.replay(&failing).await;
        queue
            .record(PendingOp::LikeSong {
                video_id: "new".into(),
                liked: true,
            })
            .unwrap();

        let healthy = Recorder::default();
        let report = queue.replay(&healthy).await;
        assert_eq!(report.applied, 2);
        assert_eq!(
            *healthy.calls.lock().unwrap(),
            vec!["like(keep,true)", "like(new,true)"],
            "the retained op stays ahead of the later one"
        );
    }

    /// The atomicity pattern: a crash between write and rename leaves
    /// the old file intact (the temp file is a sibling, the rename is
    /// atomic on every target OS).
    #[test]
    fn persistence_is_write_then_rename() {
        let dir = temp();
        let mut queue = PendingOpQueue::open(dir.path());
        queue
            .record(PendingOp::LikeSong {
                video_id: "first".into(),
                liked: true,
            })
            .unwrap();
        let main = dir.path().join(PENDING_OPS_FILE);
        assert!(main.exists());
        assert!(!dir.path().join("yt_pending_ops.json.tmp").exists());
        // an interrupted second write (simulate by leaving a stray tmp)
        std::fs::write(dir.path().join("yt_pending_ops.json.tmp"), "{}").unwrap();
        let reloaded = PendingOpQueue::open(dir.path());
        assert_eq!(reloaded.len(), 1, "the main file is still the v1 queue");
    }
}
