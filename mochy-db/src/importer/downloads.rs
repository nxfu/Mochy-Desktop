//! Transform (c): the Media3 download index seed.
//!
//! Upstream's `DownloadManager` persists its index in
//! `exoplayer_internal.db` (table `ExoPlayerDownloads`) via
//! `StandaloneDatabaseProvider` (di/AppModule.kt provides it; DownloadUtil
//! builds the manager on it). The port reads that table read-only to seed
//! its download state: completed rows mark `song.isDownloaded` (mirroring
//! the `onDownloadChanged` listener, DownloadUtil.kt:258-291) and the full
//! row set becomes a seed record for the mochy-downloads job queue
//! (Phase 15).
//!
//! Table DDL (Media3 1.7.1 `DefaultDownloadIndex`, reconstructed):
//! `ExoPlayerDownloads(id, mime_type, uri, stream_keys, custom_cache_key,
//! data, state, start_time_ms, update_time_ms, content_length,
//! stop_reason, failure_reason, percent_downloaded, bytes_downloaded,
//! key_set_id)`.
//!
//! The app's `DownloadRequest`s are built as
//! `DownloadRequest.Builder(songId, songId.toUri())
//! .setCustomCacheKey(songId).setData(title.toByteArray())`
//! (DownloadUtil.kt:477-480), so `id` is the song id, `data` is the UTF-8
//! title, and `custom_cache_key` re-links the ExoPlayer cache files.

use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};

use crate::error::{DbError, DbResult};

/// Media3 `Download` state constants (androidx.media3.common.Download).
pub mod download_state {
    pub const QUEUED: i64 = 0;
    pub const STOPPED: i64 = 1;
    pub const DOWNLOADING: i64 = 2;
    pub const COMPLETED: i64 = 3;
    pub const FAILED: i64 = 4;
    pub const REMOVING: i64 = 5;
    pub const RESTARTING: i64 = 7;
}

/// One download row, as the desktop needs it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DownloadSeedEntry {
    /// The song id (the download request id).
    pub song_id: String,
    /// Media3 state constant (see [`download_state`]).
    pub state: i64,
    /// Song title (the request's `data` blob is the UTF-8 title).
    pub title: Option<String>,
    /// The ExoPlayer cache key (same as the song id upstream).
    pub custom_cache_key: Option<String>,
    pub bytes_downloaded: i64,
    pub content_length: i64,
    /// True UTC epoch ms (Media3 uses `System.currentTimeMillis`).
    pub update_time_ms: i64,
}

/// The downloads seed produced by an import.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DownloadsSeed {
    pub entries: Vec<DownloadSeedEntry>,
    /// How many entries were completed (and thus marked on the song table).
    pub completed: usize,
}

/// Reads `exoplayer_internal.db` (read-only) and builds the seed.
///
/// Returns an empty seed when the file or table is missing (a device that
/// never downloaded anything) — matching how upstream's DownloadUtil
/// `init` block tolerates an empty index.
pub fn read_downloads_seed(exoplayer_db: &std::path::Path) -> DbResult<DownloadsSeed> {
    if !exoplayer_db.exists() {
        return Ok(DownloadsSeed::default());
    }
    let conn = Connection::open_with_flags(
        exoplayer_db,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| DbError::Import {
        path: exoplayer_db.to_owned(),
        reason: format!("cannot open exoplayer_internal.db: {e}"),
    })?;

    let table_exists: bool = conn
        .query_row(
            "SELECT COUNT(1) FROM sqlite_master WHERE type = 'table' AND name = 'ExoPlayerDownloads'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .map(|n| n > 0)
        .unwrap_or(false);
    if !table_exists {
        return Ok(DownloadsSeed::default());
    }

    let mut stmt = conn.prepare(
        "SELECT id, state, data, custom_cache_key, bytes_downloaded, content_length, update_time_ms \
         FROM ExoPlayerDownloads",
    )?;
    let rows = stmt.query_map([], |row| {
        let data: Option<Vec<u8>> = row.get(2)?;
        Ok(DownloadSeedEntry {
            song_id: row.get(0)?,
            state: row.get(1)?,
            title: data
                .filter(|d| !d.is_empty())
                .and_then(|d| String::from_utf8(d).ok()),
            custom_cache_key: row.get(3)?,
            bytes_downloaded: row.get(4)?,
            content_length: row.get(5)?,
            update_time_ms: row.get(6)?,
        })
    })?;

    let mut seed = DownloadsSeed::default();
    for row in rows {
        let entry = row.map_err(|e| DbError::Import {
            path: exoplayer_db.to_owned(),
            reason: format!("bad ExoPlayerDownloads row: {e}"),
        })?;
        if entry.state == download_state::COMPLETED {
            seed.completed += 1;
        }
        seed.entries.push(entry);
    }
    Ok(seed)
}

/// Applies the seed to the freshly imported `song` table, mirroring the
/// upstream `onDownloadChanged` listener (DownloadUtil.kt:258-291):
/// COMPLETED -> isDownloaded = 1 + dateDownload; STOPPED / REMOVING /
/// FAILED -> isDownloaded = 0, dateDownload = NULL; in-flight states are
/// left alone (the Phase 15 job queue re-enqueues them).
///
/// `dateDownload` comes from the index's `update_time_ms` (true UTC —
/// upstream stamps `LocalDateTime.now()` at completion, which the importer
/// cannot recover; the closest true-UTC source is the index row).
pub fn apply_seed_to_songs(conn: &Connection, seed: &DownloadsSeed) -> DbResult<usize> {
    let mut applied = 0usize;
    for entry in &seed.entries {
        let result = match entry.state {
            download_state::COMPLETED => conn.execute(
                "UPDATE song SET isDownloaded = 1, dateDownload = ?1 WHERE id = ?2",
                rusqlite::params![entry.update_time_ms, entry.song_id],
            ),
            download_state::STOPPED | download_state::REMOVING | download_state::FAILED => conn
                .execute(
                    "UPDATE song SET isDownloaded = 0, dateDownload = NULL WHERE id = ?1",
                    rusqlite::params![entry.song_id],
                ),
            _ => Ok(0),
        };
        applied += result.unwrap_or(0);
    }
    Ok(applied)
}
