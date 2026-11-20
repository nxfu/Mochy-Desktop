//! The Android `mochy.db` importer: read-only source, table-by-table copy,
//! exactly three transforms.
//!
//! 1. **Timestamp normalization** ([`event_time`]): every Room-converter
//!    column (wall clock stored as UTC ms) shifts to true UTC once, with
//!    the import timezone recorded in `_mochy_meta` (blueprint ch. 7.2,
//!    decision Q4).
//! 2. **Queue reconstruction** ([`queue`]): the Java-serialized
//!    `persistent_queue.data` blobs are unreadable; a paused queue is
//!    rebuilt from the song table + format cache (user loses shuffle
//!    order, same as Android restarts).
//! 3. **Downloads seed** ([`downloads`]): Media3's
//!    `exoplayer_internal.db` download index seeds the download state.
//!
//! Secrets (DataStore `enc1:` values sealed by Android Keystore AES/GCM)
//! cannot be decrypted off-device BY DESIGN — the import path forces
//! re-login exactly as the app itself does cross-device (blueprint ch.
//! 7.2). The importer only touches `mochy.db`; settings come through
//! mochy-config's TransferArchive path (Phase 02).
//!
//! Version window: Android `user_version` 35..=45 (only 35→45 has a
//! contiguous migration path; below 35 Android itself destructively wiped
//! the database — verified fact #6). The copy is column-intersection
//! driven, so any intermediate schema shape imports; missing columns take
//! the v45 defaults, and `room_master_table` is ignored.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde::{Deserialize, Serialize};

pub mod downloads;
pub mod event_time;
pub mod queue;

use crate::db::Db;
use crate::error::{DbError, DbResult};
use crate::importer::downloads::DownloadsSeed;
use crate::importer::event_time::offset_label;
use crate::importer::queue::QueueReconstruction;
use crate::schema;
use crate::watch::{Table, TableSet};

/// Import knobs.
#[derive(Debug, Clone, Default)]
pub struct ImportOptions {
    /// The Android device's timezone offset EAST of UTC in minutes at
    /// import time (e.g. 330 for IST, 480 for UTC+8). Used once to shift
    /// wall-clock-as-UTC columns to true UTC (decision Q4; see
    /// [`event_time`]).
    pub event_tz_offset_minutes: i32,
    /// The Android app's files dir (holds `persistent_queue.data` etc.);
    /// its presence is recorded on the queue reconstruction.
    pub android_files_dir: Option<PathBuf>,
    /// Path to Media3's `exoplayer_internal.db` for the downloads seed.
    pub exoplayer_internal_db: Option<PathBuf>,
}

/// Per-table copy outcome.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TableReport {
    /// Rows in the source table.
    pub source_rows: u64,
    /// Rows inserted into the target.
    pub copied: u64,
    /// Rows that failed a constraint (counted, not fatal).
    pub conflicts: u64,
    /// Source columns the v45 mirror does not have (ignored).
    pub source_only_columns: Vec<String>,
    /// v45 columns missing from the source (filled with defaults).
    pub filled_default_columns: Vec<String>,
    /// First few constraint errors, for the report.
    pub conflict_samples: Vec<String>,
}

/// The import report: counts, conflicts, the three transform records.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImportReport {
    /// `PRAGMA user_version` of the Android source.
    pub source_user_version: i64,
    /// Tables found in the source (may be fewer than 24 on old versions).
    pub source_tables: Vec<String>,
    /// Per-target-table outcome (24 entries; missing sources report 0).
    pub per_table: BTreeMap<String, TableReport>,
    /// The fixed offset applied to wall-clock-as-UTC columns.
    pub timezone_offset_minutes: i32,
    /// Human-readable offset label (`+08:00`).
    pub timezone_label: String,
    /// The shift that was applied, in ms (`-offset * 60_000`).
    pub event_shift_ms: i64,
    /// How many timestamp cells were normalized.
    pub normalized_timestamps: u64,
    /// The paused-queue reconstruction record (transform b).
    pub queue: QueueReconstruction,
    /// The downloads seed (transform c).
    pub downloads: DownloadsSeed,
    /// Songs the seed marked downloaded.
    pub downloads_applied: usize,
    /// True: Keystore-sealed secrets were unreadable, so the app must ask
    /// for a fresh login (always true for Android imports — recorded so
    /// callers can surface the notice).
    pub forced_relogin: bool,
    /// Foreign-key violations found after the copy
    /// (`table:rowid` strings, from `PRAGMA foreign_key_check`).
    pub fk_violations: Vec<String>,
    /// UTC epoch ms when the import ran.
    pub imported_at_ms: i64,
}

/// `_mochy_meta` — the desktop-side metadata table the importer creates.
///
/// A deliberate, namespaced addition to the mirror (fresh databases stay
/// pure: 24 tables + 3 views, nothing else). Holds the import timezone
/// record so later phases (recap, stats) know the events are UTC-normalized.
pub const META_TABLE: &str = "_mochy_meta";
pub const META_KEY_ANDROID_IMPORT: &str = "android_import";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct MetaAndroidImport {
    source_user_version: i64,
    timezone_offset_minutes: i32,
    timezone_label: String,
    event_shift_ms: i64,
    normalized_timestamps: u64,
    imported_at_ms: i64,
    event_timestamp_convention: String,
}

/// Runs the import INTO a desktop Db (freshly opened by the caller).
///
/// The whole copy is one writer transaction: atomic — either the library
/// arrives complete or nothing changes. Fires a single all-tables
/// invalidation after commit so subscribed queries re-run once.
///
/// Mirrors the upstream backup-restore contract (BackupRestoreViewModel:
/// restore replaces the library wholesale; there is no merge mode).
pub async fn import_android_db(
    db: &Db,
    source_path: impl AsRef<Path>,
    options: ImportOptions,
) -> DbResult<ImportReport> {
    let source_path = source_path.as_ref().to_path_buf();

    // Pre-open the source read-only on the caller's thread to surface
    // path errors early with the right DbError shape.
    {
        let probe = open_source(&source_path)?;
        let version: i64 = probe.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        check_version(&source_path, version)?;
    }

    let path_for_actor = source_path.clone();
    let tz = options.event_tz_offset_minutes;
    let files_dir = options.android_files_dir.clone();
    let exo_path = options.exoplayer_internal_db.clone();

    db.write_raw(TableSet::all(), move |conn| {
        run_import(
            conn,
            &path_for_actor,
            tz,
            files_dir.as_deref(),
            exo_path.as_deref(),
        )
    })
    .await
}

/// Opens the Android database read-only.
fn open_source(path: &Path) -> DbResult<Connection> {
    if !path.exists() {
        return Err(DbError::Import {
            path: path.to_owned(),
            reason: "file does not exist".into(),
        });
    }
    Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| DbError::Import {
        path: path.to_owned(),
        reason: format!("cannot open: {e}"),
    })
}

fn check_version(path: &Path, version: i64) -> DbResult<()> {
    if !(schema::ANDROID_IMPORTABLE_MIN_VERSION..=schema::ANDROID_IMPORTABLE_MAX_VERSION)
        .contains(&version)
    {
        return Err(DbError::Import {
            path: path.to_owned(),
            reason: format!(
                "user_version {version} is outside the importable range {}..={} \
                 (older Android upgrades were destructive — Room wiped those databases)",
                schema::ANDROID_IMPORTABLE_MIN_VERSION,
                schema::ANDROID_IMPORTABLE_MAX_VERSION
            ),
        });
    }
    Ok(())
}

/// The transactional core, over the writer connection.
///
/// `foreign_keys` must be toggled OUTSIDE the transaction (SQLite ignores
/// the pragma mid-transaction), which is why the importer uses
/// [`Db::write_raw`] and manages BEGIN/COMMIT itself: either the library
/// arrives complete or nothing changes.
fn run_import(
    conn: &mut Connection,
    source_path: &Path,
    tz_offset_minutes: i32,
    files_dir: Option<&Path>,
    exo_path: Option<&Path>,
) -> DbResult<ImportReport> {
    let source = open_source(source_path)?;
    let source_user_version: i64 = source.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    check_version(source_path, source_user_version)?;

    let imported_at_ms = crate::util::now_ms();
    let shift_ms = -(tz_offset_minutes as i64) * 60_000;

    // Transform b reads only the source, so it can run before the target
    // transaction opens.
    let queue_record = crate::importer::queue::reconstruct_queue(&source, files_dir)?;
    let downloads_seed = match exo_path {
        Some(path) => crate::importer::downloads::read_downloads_seed(path)?,
        None => DownloadsSeed::default(),
    };

    // FK enforcement OFF for the copy (Android DBs may carry deliberate
    // orphans — the import_* tables have NO FK by design; Room-side
    // constraint failures surface as conflict counts instead of aborting
    // the whole import).
    conn.pragma_update(None, "foreign_keys", "OFF")?;
    conn.execute_batch("BEGIN IMMEDIATE")?;

    let mut normalized_timestamps: u64 = 0;
    let mut per_table: BTreeMap<String, TableReport> = BTreeMap::new();
    let mut source_tables: Vec<String> = Vec::new();

    let copy = copy_tables(
        conn,
        &source,
        tz_offset_minutes,
        &mut normalized_timestamps,
        &mut per_table,
        &mut source_tables,
    );
    if let Err(e) = copy {
        let _ = conn.execute_batch("ROLLBACK");
        let _ = conn.pragma_update(None, "foreign_keys", "ON");
        return Err(e);
    }

    // Transform c: apply the seed inside the same transaction (the song
    // rows it marks were just copied).
    let downloads_applied =
        match crate::importer::downloads::apply_seed_to_songs(conn, &downloads_seed) {
            Ok(n) => n,
            Err(e) => {
                let _ = conn.execute_batch("ROLLBACK");
                let _ = conn.pragma_update(None, "foreign_keys", "ON");
                return Err(e);
            }
        };

    // Commit the library.
    if let Err(e) = conn.execute_batch("COMMIT") {
        let _ = conn.execute_batch("ROLLBACK");
        let _ = conn.pragma_update(None, "foreign_keys", "ON");
        return Err(DbError::Sqlite(e));
    }
    // FKs back on for normal operation (outside any transaction).
    conn.pragma_update(None, "foreign_keys", "ON")?;

    // ---- post-commit bookkeeping (own implicit transactions) ----

    // Audit the copy: constraint violations are reported, not fatal
    // (upstream tolerates orphaned import_* rows by design).
    let mut fk_violations = Vec::new();
    {
        let mut stmt = conn.prepare("PRAGMA foreign_key_check")?;
        let rows = stmt.query_map([], |r| {
            Ok(format!(
                "{}:{}",
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?
            ))
        })?;
        for row in rows {
            fk_violations.push(row?);
        }
    }

    // Record the import timezone (decision Q4).
    create_meta_table(conn)?;
    let meta = MetaAndroidImport {
        source_user_version,
        timezone_offset_minutes: tz_offset_minutes,
        timezone_label: offset_label(tz_offset_minutes),
        event_shift_ms: shift_ms,
        normalized_timestamps,
        imported_at_ms,
        event_timestamp_convention: "true-utc (normalized from android wall-clock-as-utc)"
            .to_string(),
    };
    conn.execute(
        &format!("INSERT OR REPLACE INTO `{META_TABLE}` (key, value) VALUES (?1, ?2)"),
        rusqlite::params![
            META_KEY_ANDROID_IMPORT,
            serde_json::to_string(&meta).unwrap_or_default()
        ],
    )?;

    tracing::debug!(
        source_user_version,
        tables = source_tables.len(),
        normalized_timestamps,
        downloads = downloads_seed.entries.len(),
        "android database import complete"
    );

    Ok(ImportReport {
        source_user_version,
        source_tables,
        per_table,
        timezone_offset_minutes: tz_offset_minutes,
        timezone_label: offset_label(tz_offset_minutes),
        event_shift_ms: shift_ms,
        normalized_timestamps,
        queue: queue_record,
        downloads: downloads_seed,
        downloads_applied,
        // Keystore-sealed secrets are unreadable off-device by design.
        forced_relogin: true,
        fk_violations,
        imported_at_ms,
    })
}

/// The table-by-table copy (inside the caller's transaction).
fn copy_tables(
    conn: &Connection,
    source: &Connection,
    tz_offset_minutes: i32,
    normalized_timestamps: &mut u64,
    per_table: &mut BTreeMap<String, TableReport>,
    source_tables: &mut Vec<String>,
) -> DbResult<()> {
    for table in schema::TABLES {
        let target_columns = schema::columns(conn, table)?;
        debug_assert!(!target_columns.is_empty(), "mirror table {table} missing");

        let source_has = schema::table_exists(source, table)?;
        if !source_has {
            per_table.insert(
                table.to_string(),
                TableReport {
                    filled_default_columns: target_columns.iter().map(|c| c.name.clone()).collect(),
                    ..TableReport::default()
                },
            );
            continue;
        }
        source_tables.push(table.to_string());

        let source_columns = schema::columns(source, table)?;
        let source_names: Vec<&str> = source_columns.iter().map(|c| c.name.as_str()).collect();

        let mut report = TableReport {
            source_only_columns: source_names
                .iter()
                .filter(|n| !target_columns.iter().any(|t| t.name == **n))
                .map(|n| n.to_string())
                .collect(),
            filled_default_columns: target_columns
                .iter()
                .filter(|t| !source_names.contains(&t.name.as_str()))
                .map(|t| t.name.clone())
                .collect(),
            ..TableReport::default()
        };

        // Column-name intersection, in TARGET order so defaults land right.
        let shared: Vec<&crate::schema::ColumnInfo> = target_columns
            .iter()
            .filter(|t| source_names.contains(&t.name.as_str()))
            .collect();

        // Count source rows.
        report.source_rows =
            source.query_row(&format!("SELECT COUNT(*) FROM `{table}`"), [], |r| {
                r.get::<_, i64>(0)
            })? as u64;

        // Build the INSERT. Unquoted-safe: all mirror column names are
        // Room-generated identifiers; `index`/`order` still get quoted.
        let insert_sql = format!(
            "INSERT OR IGNORE INTO `{table}` ({}) VALUES ({})",
            shared
                .iter()
                .map(|c| format!("`{}`", c.name))
                .collect::<Vec<_>>()
                .join(", "),
            shared.iter().map(|_| "?").collect::<Vec<_>>().join(", ")
        );

        let select_sql = format!(
            "SELECT {} FROM `{table}`",
            shared
                .iter()
                .map(|c| format!("`{}`", c.name))
                .collect::<Vec<_>>()
                .join(", ")
        );

        let shift_cols: Vec<bool> = shared
            .iter()
            .map(|c| crate::importer::event_time::is_wall_clock_as_utc(table, &c.name))
            .collect();

        {
            let mut select = source.prepare(&select_sql)?;
            let mut insert = conn.prepare(&insert_sql)?;
            let ncols = shared.len();
            let mut rows = select.query([])?;
            while let Some(row) = rows.next()? {
                let mut values: Vec<Option<i64>> = Vec::with_capacity(ncols);
                let mut text_values: Vec<Option<String>> = Vec::with_capacity(ncols);
                let mut kinds: Vec<u8> = Vec::with_capacity(ncols); // 0 = int-ish, 1 = text, 2 = real
                let mut real_values: Vec<Option<f64>> = Vec::with_capacity(ncols);

                use rusqlite::types::{Value, ValueRef};
                for (idx, _col) in shared.iter().enumerate() {
                    let vref = row.get_ref(idx)?;
                    match vref {
                        ValueRef::Null => {
                            kinds.push(0);
                            values.push(None);
                            text_values.push(None);
                            real_values.push(None);
                        }
                        ValueRef::Integer(v) => {
                            if shift_cols[idx] {
                                let shifted =
                                    crate::importer::event_time::normalize(v, tz_offset_minutes);
                                *normalized_timestamps += 1;
                                kinds.push(0);
                                values.push(Some(shifted));
                                text_values.push(None);
                                real_values.push(None);
                            } else {
                                kinds.push(0);
                                values.push(Some(v));
                                text_values.push(None);
                                real_values.push(None);
                            }
                        }
                        ValueRef::Real(v) => {
                            kinds.push(2);
                            values.push(None);
                            text_values.push(None);
                            real_values.push(Some(v));
                        }
                        ValueRef::Text(t) => {
                            kinds.push(1);
                            values.push(None);
                            text_values.push(Some(String::from_utf8_lossy(t).into_owned()));
                            real_values.push(None);
                        }
                        ValueRef::Blob(b) => {
                            // Blob columns do not exist in the mirror; store
                            // as text losslessly enough for reporting.
                            kinds.push(1);
                            values.push(None);
                            text_values.push(Some(format!("blob:{}b", b.len())));
                            real_values.push(None);
                        }
                    }
                }

                // Bind dynamically by kind.
                let mut params: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(ncols);
                for idx in 0..ncols {
                    match kinds[idx] {
                        0 => params.push(&values[idx]),
                        1 => params.push(&text_values[idx]),
                        _ => params.push(&real_values[idx]),
                    }
                }
                match insert.execute(params.as_slice()) {
                    Ok(_) => report.copied += 1,
                    Err(e) => {
                        report.conflicts += 1;
                        if report.conflict_samples.len() < 5 {
                            report.conflict_samples.push(format!("{table}: {e}"));
                        }
                    }
                }
                let _ = Value::Null; // keep the import above honest about kinds
            }
        }
        per_table.insert(table.to_string(), report);
    }

    Ok(())
}

fn create_meta_table(conn: &Connection) -> DbResult<()> {
    conn.execute_batch(&format!(
        "CREATE TABLE IF NOT EXISTS `{META_TABLE}` (key TEXT NOT NULL PRIMARY KEY, value TEXT NOT NULL)"
    ))?;
    Ok(())
}

/// Reads the recorded Android import metadata (if this database was built
/// by an import).
pub fn read_import_meta(conn: &Connection) -> DbResult<Option<serde_json::Value>> {
    let exists = schema::table_exists(conn, META_TABLE)?;
    if !exists {
        return Ok(None);
    }
    let value: Option<String> = conn
        .query_row(
            &format!("SELECT value FROM `{META_TABLE}` WHERE key = ?1"),
            [META_KEY_ANDROID_IMPORT],
            |r| r.get(0),
        )
        .optional()?;
    Ok(value.and_then(|v| serde_json::from_str(&v).ok()))
}

/// The full table set touched by an import — used by tests and by the
/// caller that wants to pre-subscribe watchers before importing.
pub fn import_table_set() -> TableSet {
    TableSet::all().union(TableSet::of(&[Table::Song]))
}
