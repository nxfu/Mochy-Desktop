# Phase 03 Audit — mochy-db (schema, repositories, invalidation, importer)

**Audit date:** 2026-11-20
**Method:** independent schema-parity script (parses the 24 upstream
`@Entity` data classes, builds the mirror DDL from `schema.rs` in a real
SQLite DB, diffs columns/affinity/nullability/defaults/PK/FK), index
reconciliation against the v34 JSON + migration `CREATE INDEX` statements,
migration-text comparison, DAO-semantics spot checks against
`db/DatabaseDao.kt` + entity state machines, importer-transform evidence
review, and a deterministic reproduction of the macOS CI failure.

---

## 1. Schema verification

### 1.1 Upstream facts (pinned bde01cb4, re-verified)

- `MusicDatabaseVersion = 45` (MusicDatabase.kt:110); `exportSchema = false`.
- **24 entities** in `@Database(entities = …)` — list matches the mirror's
  `TABLES` order exactly.
- **3 views** (`sorted_song_artist_map`, `sorted_song_album_map`,
  `playlist_song_map_preview`) — SQL text compared verbatim against the
  `@DatabaseView` values: **identical**, including the quirky
  `WHERE position <= 3` preview filter (4 thumbnails with 0-based
  positions — semantics preserved, pinned by a test).
- `app/schemas/com.mochyy.mochymusic.db.InternalDatabase/` contains **only
  `34.json`** — exports stop at v34 (17 tables). The committed fixture
  `tests/data/room_schema_v34.json` is **byte-identical** to upstream's
  file (diff).
- **14 hand-written migrations**; only 35→45 is contiguous (MIGRATION_35_36
  … MIGRATION_44_45 = 10 steps); `.fallbackToDestructiveMigration(
  dropAllTables = true)` confirms older paths are destructive. No
  MIGRATION_34_35 exists (version bump only) — the v35 fixture is correctly
  built from the v34 JSON.
- `JournalMode.WRITE_AHEAD_LOGGING` + two `newFixedThreadPool(4)` executors
  (MusicDatabase.kt:162-164) — mirrored by WAL + the 4-connection reader
  pool.

### 1.2 Column-level parity (independent script)

All 24 tables, **167 columns total**: name, affinity, NOT NULL, DEFAULT,
primary keys (incl. composite) and foreign keys (incl. `ON DELETE CASCADE`)
**match the upstream entity declarations with zero diffs**. Special
semantics confirmed from source:

- `song.inLibrary` NULL = not in library; `isLocal`/`localUri`/`genre`
  tri-state preserved (nullable / default-annotated exactly as Kotlin).
- `album.playlistId` = the YT album playlist; `playlist.setVideoId` rides
  `playlist_song_map` for YT moves.
- `format.loudnessDb` + `perceptualLoudnessDb` (REAL).
- `lyrics` provider + `translatedLyrics` + `translationLanguage/Mode`
  columns with `'Unknown'`/`''` defaults; the `LYRICS_NOT_FOUND` sentinel
  lives in the lyrics column (documented in entities).
- import_* tables have **no FK by design** — verified against the Kotlin
  entities (no `ForeignKey` entries) and the migration DDL.
- `song_edit` overlay incl. `artists` JSON text column.
- `playlist.description` divergence (migration says `DEFAULT NULL`, fresh
  Room DDL has none) is already documented in HANDOFF Phase 03; behaviorally
  identical (nullable).

### 1.3 Index parity

Mirror declares 18 indices = the 14 v34-JSON indices ∪ the 13 migration
`CREATE INDEX` statements (9 overlap). Set reconciliation: **0 missing,
0 extra**. Names follow Room's `index_<table>_<column>` convention; the
`search_history` unique index is unique.

### 1.4 Migration DDL fidelity

The seven post-v34 table DDLs and every column addition in the mirror were
compared against the `db.execSQL` strings in MusicDatabase.kt:723-905 —
verbatim matches (playlist_event, imported_song, import_session,
import_session_song, imported_playlist (+remote/author columns),
imported_playlist_track, song_edit; song.localUri/addedByLike/genre;
playlist.description). The mirror correctly does NOT port the migration
chain (v45 DDL is the baseline, `user_version = 1`, no `room_master_table`
— asserted by tests, re-run green).

## 2. Database initialization

rusqlite bundled SQLite; `apply_pragmas` sets WAL, `busy_timeout` 60 s,
`cache_size` -16 000, `wal_autocheckpoint` 1 000, `synchronous` NORMAL,
`foreign_keys` ON — mirrors `InternalDatabase.newInstance`'s onOpen block.
Fresh DBs stamp `user_version = 1`; re-open verifies the mirror is intact
(`BadSchema` on truncation); apply is idempotent (test). Repeatable and
safe.

## 3. Repositories

- Per-aggregate split (13 repositories over the actor). Writes go through
  `Db::write` (BEGIN IMMEDIATE … COMMIT + post-commit invalidation);
  `write_raw` is the documented importer escape hatch (own transaction +
  out-of-transaction pragmas).
- Parameter binding via `rusqlite::params!` everywhere — no string
  interpolation of values; the only formatted SQL uses table/column names
  from the compile-time mirror constants (quoted). **No injection paths.**
- The three view-sorted tables: consuming queries inline their ORDER BY
  (`song_artist_map.position`, `song_album_map.\`index\` … rowid`,
  `playlist_song_map.position`) — verified by grep + read; the views exist
  only for schema parity. Kotlin-side `Collator` re-sorts are correctly
  deferred to the UI layer (documented decision).
- Entity state machines: `toggle_like` / `local_toggle_like` verified
  against SongEntity.kt (likedDate flip, inLibrary backfill, addedByLike).
  Bookmark toggles match ArtistEntity/AlbumEntity/PlaylistEntity. **One
  real defect found and fixed** (F-002 below).

## 4. Concurrency and reactivity

- Single-writer actor over `tokio_rusqlite::Connection`; write futures are
  runtime-agnostic (no timers/IO in the future itself) — GPUI's executor
  can await them. Reader pool = 4 connections (upstream pool size).
- Invalidation: `Invalidations::send` runs **inside the writer closure
  after COMMIT and before the reply resolves** — a caller that awaits
  `write()` has a strict happens-before guarantee that the event is already
  in the broadcast buffer. Aborted transactions fire nothing (FK-violation
  test). Table-set filtering by intersection; dead std-mpsc receivers are
  reaped on send; `try_send`-style non-blocking fan-out (std unbounded
  channel) can never block the writer; broadcast lag coalesces to the
  oldest retained event.
- **The invalidation system was NOT the cause of the macOS CI failure**
  (see F-001) — its ordering is sound.
- Persistence purity: no network IO anywhere in the crate; the YouTube
  side-effects of upstream entity methods are documented as Phase 14
  SongService hooks on every affected method (verified by grep + read).

## 5. Android importer

- Source opened `SQLITE_OPEN_READ_ONLY`; version gate `user_version`
  35..=45 (below 35 = destructive wipe upstream — typed refusal with the
  reason).
- Table-by-table copy driven by column intersection in TARGET order;
  per-table `TableReport` (source rows, copied, conflicts + samples,
  source-only columns, filled-default columns) — the acceptance output was
  re-run and shows the v35 fixture importing 17 tables with correct
  filled-default lists.
- Whole import is one `BEGIN IMMEDIATE` transaction with `foreign_keys`
  toggled OFF outside it, rollback on any failure, and a post-commit
  `PRAGMA foreign_key_check` audit reported (not fatal — import_* orphans
  are by design).
- **Transform A (timestamps)**: `WALL_CLOCK_AS_UTC_COLUMNS` — independent
  recount of every `LocalDateTime` field across the 24 upstream entities
  gives exactly the same 21 columns (song 5, artist 2, album 4, playlist 3,
  event 1, playlist_event 1, recognition_history 1, imported_song 1,
  import_session 1, imported_playlist 2); `speed_dial_item.createDate`
  correctly excluded (`System.currentTimeMillis()` = true UTC). The
  converter analysis (`ZoneOffset.UTC` both ways + `LocalDateTime.now()`
  write sites) is correct. Offset label, shift ms and normalized-cell
  count are recorded in `_mochy_meta` + the report. (HANDOFF says "22
  columns" — actual list is 21; F-006.)
- **Transform B (queue)**: detects `persistent_queue.data`, reconstructs a
  paused queue from most-recent DISTINCT event songs (cap 50) with artists
  (position order) + cached format; library fallback; records
  shuffle-order loss + blob presence on the serde record. Matches the
  documented decision (Android itself loses the serialized queue on
  unreadable blobs).
- **Transform C (downloads)**: reads `ExoPlayerDownloads` read-only;
  completed rows → `isDownloaded=1` + `dateDownload` from the index's
  true-UTC `update_time_ms`; STOPPED/REMOVING/FAILED clear; in-flight left
  for Phase 15. Verified against DownloadUtil.kt:477-479 (request shape)
  and the `onDownloadChanged` listener (COMPLETED/FAILED/STOPPED/REMOVING
  branches) — exact match.
- **Secrets**: importer never touches DataStore files; `forced_relogin:
  true` is reported (Keystore-sealed `enc1:` values are undecryptable
  off-device by design).

## 6. Tests

33 → **39 tests** after this audit's additions (7 unit + 3 schema + 15
repos + 10 watcher + 4 importer), all green. The schema report prints 24
tables + 3 views with column counts (167 total) and the v34→v45 delta;
the v34 exact-match test asserts name+affinity+notNull+default for every
JSON-covered column and the delta set. Importer tests produce the
normalized event table (cell-by-cell wall-clock→UTC assertions incl. the
IST non-hour offset), the queue-reconstruction record (JSON round-trip),
and the downloads seed — on both a v35 and a v45 Android-shaped fixture.

## 7. Findings

| ID | Severity | Finding | Status |
|---|---|---|---|
| F-001 | **CRITICAL** (CI-blocking) | **macOS CI failure** `write_fires_broadcast_invalidation … assertion failed: inserted` (watcher.rs:32). **Root cause (reproduced deterministically)**: `Db::open_in_memory` derived the shared-cache URI name from `SystemTime::now()` nanos; macOS `clock_gettime` ticks at microsecond granularity, so two parallel test threads drew identical timestamps, opened the SAME in-memory database, and the second `INSERT OR IGNORE` of song id "song1" silently returned `Ok(false)` — which is exactly the observed assertion. The broadcast/invalidation code was never at fault (send-after-commit ordering verified). Fix: name = `pid` + process-global `AtomicU64` counter (collision-free by construction on every OS); same hardening applied to the importer tests' temp dirs. Reproduction: forcing a fixed URI name reproduced the CI failure byte-for-byte (4 passed, 1 failed, same line/message); after the fix, 10/10 watcher tests pass and 50/50 stress runs (8 threads) pass. | **Fixed** + 5 regression tests |
| F-002 | HIGH | `queries::toggle_library` set `liked = true` when ADDING a song to the library; upstream `SongEntity.toggleLibrary` leaves `liked` unchanged on add (`liked = if (inLibrary == null) liked else false`, SongEntity.kt:76-78) and only clears it on remove. Impact: adding an unliked song would pollute the liked-songs list. Fixed to match upstream; regression test `song_repo_toggle_library_keeps_liked_on_add` covers add (liked stays false, not in liked list), remove (cleared), and the `byLike` variant. | **Fixed** + test |
| F-006 | LOW | HANDOFF Phase 03 Decision 1 says "22 converter-stamped columns"; the implemented (and correct) list has 21 entries. | Documented |
| F-009 | LOW | In-memory test DBs use `cache=shared` (required so the writer + reader pool see one DB). Shared-cache mode has table-level locks: a reader holding an open statement on a table while the writer writes could surface `SQLITE_LOCKED` (not covered by `busy_timeout`). Current code always fully collects statement rows before returning, so no live path can hit it; production file DBs use WAL and are unaffected. Recorded as a test-path caveat. | Documented |

## 8. Verdict

**Phase 03: PASS with fixes applied.** The schema mirror is verified at
column level against the pinned source (167/167 columns, 18/18 indices,
3/3 views verbatim, migrations verbatim); repositories and the invalidation
bus behave correctly; the importer implements exactly the three transforms
with evidence; the macOS CI failure was an in-memory-DB naming collision,
now fixed with regression coverage.
