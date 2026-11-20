//! Android importer tests: fixtures built from the harvested Room DDL
//! (v35 shape from the 34.json createSql + user_version=35; v45 shape from
//! the mirror DDL + room_master_table), then a table-by-table import with
//! the three transforms asserted end-to-end.

use std::path::PathBuf;

use rusqlite::{Connection, OpenFlags};

use mochy_db::importer::downloads::download_state;
use mochy_db::{Db, ImportOptions, TableSet, import_android_db, read_import_meta};

const ROOM_SCHEMA_V34: &str = include_str!("data/room_schema_v34.json");

const TZ_OFFSET_MINUTES: i32 = 330; // IST — a non-hour offset proves the math
const T0_WALL_CLOCK: i64 = 1_795_152_600_000; // 2026-11-20T05:30Z as stored = 00:00 true UTC

fn temp_dir(tag: &str) -> PathBuf {
    // Counter, not a clock read: macOS `clock_gettime` ticks at microsecond
    // granularity, so timestamp-derived names can collide between parallel
    // tests (same root cause as the in-memory DB name flake fixed in db.rs).
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let seq = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "mochy-db-importer-{tag}-{}-{seq}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Builds an Android-shaped v35 database by executing the Room v34 JSON's
/// own createSql statements (v35 = v34 schema: MIGRATION_35_36 adds
/// playlist_event only when moving TO 36), plus `room_master_table` and
/// `user_version = 35`.
fn build_android_v35(path: &std::path::Path) {
    let json: serde_json::Value = serde_json::from_str(ROOM_SCHEMA_V34).unwrap();
    let conn = Connection::open(path).unwrap();
    conn.execute_batch("BEGIN").unwrap();
    for entity in json["database"]["entities"].as_array().unwrap() {
        let table = entity["tableName"].as_str().unwrap();
        // createSql is the full statement with a ${TABLE_NAME} placeholder.
        conn.execute_batch(
            &entity["createSql"]
                .as_str()
                .unwrap()
                .replace("${TABLE_NAME}", table),
        )
        .unwrap();
        for index in entity["indices"].as_array().unwrap_or(&vec![]) {
            conn.execute_batch(
                &index["createSql"]
                    .as_str()
                    .unwrap()
                    .replace("${TABLE_NAME}", table),
            )
            .unwrap();
        }
    }
    for view in json["database"]["views"].as_array().unwrap() {
        // View createSql also carries a ${VIEW_NAME} placeholder.
        conn.execute_batch(
            &view["createSql"]
                .as_str()
                .unwrap()
                .replace("${VIEW_NAME}", view["viewName"].as_str().unwrap()),
        )
        .unwrap();
    }
    // Room always creates its identity table; the importer must ignore it.
    conn.execute_batch(
        "CREATE TABLE room_master_table (id INTEGER PRIMARY KEY, identity_hash TEXT);
         INSERT INTO room_master_table (identity_hash) VALUES ('fake-v35-hash');",
    )
    .unwrap();
    conn.execute_batch("COMMIT").unwrap();
    conn.pragma_update(None, "user_version", 35_i64).unwrap();

    // --- Android-shaped data (wall-clock-as-UTC timestamps) ---
    conn.execute(
        "INSERT INTO song (id, title, duration, explicit, liked, likedDate, totalPlayTime, inLibrary, \
         isLocal, lyricsOffset, romanizeLyrics, isDownloaded, isUploaded, isVideo) \
         VALUES ('s1', 'Nightcall', 180, 0, 1, ?, 300000, ?, 0, 0, 1, 0, 0, 0)",
        rusqlite::params![T0_WALL_CLOCK, T0_WALL_CLOCK],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO song (id, title, duration, explicit, liked, totalPlayTime, isLocal, lyricsOffset, \
         romanizeLyrics, isDownloaded, isUploaded, isVideo) \
         VALUES ('s2', 'Local File', 200, 0, 0, 0, 1, 0, 1, 0, 0, 0)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO artist (id, name, lastUpdateTime, isLocal) VALUES ('UCA1', 'Kavinsky', ?, 0)",
        rusqlite::params![T0_WALL_CLOCK],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO song_artist_map (songId, artistId, position) VALUES ('s1', 'UCA1', 0)",
        [],
    )
    .unwrap();
    // Note: v35 song has NO addedByLike / genre? It HAS genre (added v44? no —
    // genre came in 43->44). v34 JSON song has 23 columns; we inserted the
    // intersection columns only — the copy fills the rest.
    conn.execute(
        "INSERT INTO event (songId, timestamp, playTime) VALUES ('s1', ?, 60000)",
        rusqlite::params![T0_WALL_CLOCK],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO event (songId, timestamp, playTime) VALUES ('s1', ?, 30000)",
        rusqlite::params![T0_WALL_CLOCK + 3_600_000],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO playCount (song, year, month, count) VALUES ('s1', 2026, 11, 2)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO lyrics (id, lyrics, provider, translatedLyrics, translationLanguage, translationMode) \
         VALUES ('s1', 'LYRICS_NOT_FOUND', 'LRCLIB', '', '', '')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO format (id, itag, mimeType, codecs, bitrate, sampleRate, contentLength, loudnessDb, perceptualLoudnessDb, playbackUrl) \
         VALUES ('s1', 251, 'audio/webm', 'opus', 160000, 48000, 4000000, -3.5, -2.1, NULL)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO playlist (id, name, createdAt, lastUpdateTime, isEditable, isLocal, isAutoSync) \
         VALUES ('LP1', 'Mix', ?, ?, 1, 0, 0)",
        rusqlite::params![T0_WALL_CLOCK, T0_WALL_CLOCK],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO playlist_song_map (playlistId, songId, position, setVideoId) VALUES ('LP1', 's1', 0, NULL)",
        [],
    )
    .unwrap();
    conn.execute("INSERT INTO search_history (query) VALUES ('kavinsky')", [])
        .unwrap();
    conn.execute(
        "INSERT INTO speed_dial_item (id, title, type, explicit, createDate) VALUES ('s1', 'Nightcall', 'SONG', 0, ?)",
        rusqlite::params![T0_WALL_CLOCK], // TRUE UTC upstream — must NOT shift
    )
    .unwrap();
}

/// Builds an Android-shaped v45 database: mirror DDL + room_master_table +
/// user_version = 45 (exercises the post-34 tables too).
fn build_android_v45(path: &std::path::Path) {
    build_android_v35(path);
    let conn = Connection::open(path).unwrap();
    // The v35 build already carries the v34 tables; add the v35->v45
    // objects exactly as the migrations would have.
    conn.execute_batch(
        "ALTER TABLE song ADD COLUMN localUri TEXT DEFAULT NULL; \
         ALTER TABLE song ADD COLUMN addedByLike INTEGER NOT NULL DEFAULT 0; \
         ALTER TABLE song ADD COLUMN genre TEXT; \
         ALTER TABLE playlist ADD COLUMN description TEXT DEFAULT NULL;",
    )
    .unwrap();
    conn.execute(
        "UPDATE song SET localUri = 'file:///x.mp3', genre = 'Synthwave' WHERE id = 's2'",
        [],
    )
    .unwrap();
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS `playlist_event` (`id` INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL, \
         `playlistId` TEXT NOT NULL, `timestamp` INTEGER NOT NULL, \
         FOREIGN KEY(`playlistId`) REFERENCES `playlist`(`id`) ON UPDATE NO ACTION ON DELETE CASCADE );
         CREATE INDEX IF NOT EXISTS `index_playlist_event_playlistId` ON `playlist_event` (`playlistId`);
         INSERT INTO playlist_event (playlistId, timestamp) VALUES ('LP1', 1000);
         CREATE TABLE IF NOT EXISTS `imported_song` (`songId` TEXT NOT NULL, `source` TEXT NOT NULL, \
         `importedAt` INTEGER NOT NULL, PRIMARY KEY(`songId`, `source`));
         INSERT INTO imported_song (songId, source, importedAt) VALUES ('s1', 'spotify', 1000);
         CREATE TABLE IF NOT EXISTS `import_session` (`id` INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL, \
         `source` TEXT NOT NULL, `startedAt` INTEGER NOT NULL);
         INSERT INTO import_session (source, startedAt) VALUES ('spotify', 1000);
         CREATE TABLE IF NOT EXISTS `import_session_song` (`sessionId` INTEGER NOT NULL, `songId` TEXT NOT NULL, \
         `addedToLibrary` INTEGER NOT NULL, `liked` INTEGER NOT NULL, PRIMARY KEY(`sessionId`, `songId`));
         INSERT INTO import_session_song (sessionId, songId, addedToLibrary, liked) VALUES (1, 's1', 1, 0);
         CREATE TABLE IF NOT EXISTS `imported_playlist` (`playlistId` TEXT NOT NULL, `source` TEXT NOT NULL, \
         `sessionId` INTEGER, `importedAt` INTEGER NOT NULL, PRIMARY KEY(`playlistId`));
         ALTER TABLE imported_playlist ADD COLUMN remoteId TEXT;
         ALTER TABLE imported_playlist ADD COLUMN remoteUrl TEXT;
         ALTER TABLE imported_playlist ADD COLUMN lastSyncedAt INTEGER;
         ALTER TABLE imported_playlist ADD COLUMN authorName TEXT;
         ALTER TABLE imported_playlist ADD COLUMN authorAvatarUrl TEXT;
         ALTER TABLE imported_playlist ADD COLUMN authorUrl TEXT;
         CREATE TABLE IF NOT EXISTS `imported_playlist_track` (`playlistId` TEXT NOT NULL, `trackKey` TEXT NOT NULL, \
         PRIMARY KEY(`playlistId`, `trackKey`));
         CREATE TABLE IF NOT EXISTS `song_edit` (`songId` TEXT NOT NULL, `title` TEXT, `artists` TEXT, `album` TEXT, \
         `year` INTEGER, `cover` TEXT, `videoId` TEXT, `noVideo` INTEGER NOT NULL DEFAULT 0, PRIMARY KEY(`songId`));
         INSERT INTO song_edit (songId, title, artists) VALUES ('s1', 'Nightcall (edit)', NULL);",
    )
    .unwrap();
    conn.pragma_update(None, "user_version", 45_i64).unwrap();
}

/// Builds a Media3 `exoplayer_internal.db` with the DefaultDownloadIndex
/// table shape and a few rows.
fn build_exoplayer_db(path: &std::path::Path) {
    let conn = Connection::open(path).unwrap();
    conn.execute_batch(
        "CREATE TABLE ExoPlayerDownloads (id TEXT PRIMARY KEY NOT NULL, mime_type TEXT, uri TEXT NOT NULL, \
         stream_keys TEXT NOT NULL, custom_cache_key TEXT, data BLOB NOT NULL, state INTEGER NOT NULL, \
         start_time_ms INTEGER NOT NULL, update_time_ms INTEGER NOT NULL, content_length INTEGER NOT NULL, \
         stop_reason INTEGER NOT NULL, failure_reason INTEGER NOT NULL, percent_downloaded REAL NOT NULL, \
         bytes_downloaded INTEGER NOT NULL, key_set_id BLOB NOT NULL);",
    )
    .unwrap();
    let insert = "INSERT INTO ExoPlayerDownloads (id, mime_type, uri, stream_keys, custom_cache_key, data, state, \
                  start_time_ms, update_time_ms, content_length, stop_reason, failure_reason, percent_downloaded, \
                  bytes_downloaded, key_set_id) VALUES (?1, 'audio/webm', ?1, '', ?1, ?2, ?3, 1000, ?4, 4000000, 0, 0, 1.0, 4000000, x'')";
    // COMPLETED download (title blob in `data`).
    conn.execute(
        insert,
        rusqlite::params![
            "s1",
            "Nightcall".as_bytes(),
            download_state::COMPLETED,
            1_700_000_000_000i64
        ],
    )
    .unwrap();
    // FAILED download.
    conn.execute(
        insert,
        rusqlite::params![
            "s2",
            "Local File".as_bytes(),
            download_state::FAILED,
            1_700_000_000_001i64
        ],
    )
    .unwrap();
    // A download for a song that is not in the library (a removed song) —
    // the seed keeps the entry; apply marks nothing.
    conn.execute(
        insert,
        rusqlite::params![
            "gone",
            "Removed".as_bytes(),
            download_state::COMPLETED,
            1_700_000_000_002i64
        ],
    )
    .unwrap();
}

/// The full import round-trip on a v35 Android database.
#[tokio::test]
async fn imports_android_v35_database_with_all_three_transforms() {
    let dir = temp_dir("v35");
    let source = dir.join("mochy.db");
    build_android_v35(&source);

    // The Java-serialized queue blob (unreadable by design).
    let files_dir = dir.join("files");
    std::fs::create_dir_all(&files_dir).unwrap();
    std::fs::write(
        files_dir.join("persistent_queue.data"),
        b"\xac\xed\x00\x05 not-really-java",
    )
    .unwrap();

    // Media3 download index.
    let exo = dir.join("exoplayer_internal.db");
    build_exoplayer_db(&exo);

    let target_path = dir.join("desktop.db");
    let db = Db::open(&target_path).await.unwrap();
    let report = import_android_db(
        &db,
        &source,
        ImportOptions {
            event_tz_offset_minutes: TZ_OFFSET_MINUTES,
            android_files_dir: Some(files_dir),
            exoplayer_internal_db: Some(exo),
        },
    )
    .await
    .unwrap();

    // --- The report ---
    println!(
        "import report: v{} source, {} tables",
        report.source_user_version,
        report.source_tables.len()
    );
    for (table, r) in &report.per_table {
        println!(
            "  {table:<24} src={:<3} copied={:<3} conflicts={} filled_defaults={}",
            r.source_rows,
            r.copied,
            r.conflicts,
            r.filled_default_columns.len()
        );
    }
    assert_eq!(report.source_user_version, 35);
    assert_eq!(report.timezone_label, "+05:30");
    assert!(report.forced_relogin, "Keystore secrets force re-login");
    assert!(
        report.fk_violations.is_empty(),
        "no FK violations: {:?}",
        report.fk_violations
    );

    // v35 source lacks the post-35 tables -> zero rows, defaults filled.
    let song_report = &report.per_table["song"];
    assert_eq!(song_report.source_rows, 2);
    assert_eq!(song_report.copied, 2);
    assert_eq!(song_report.conflicts, 0);
    // v35 song has no addedByLike (v42) nor genre (v44).
    assert!(
        song_report
            .filled_default_columns
            .contains(&"addedByLike".to_string())
    );
    assert!(
        song_report
            .filled_default_columns
            .contains(&"genre".to_string())
    );
    // No columns exist in the source that the mirror lacks.
    assert!(song_report.source_only_columns.is_empty());

    // --- Transform (a): normalized event timestamps ---
    // Wall clock 05:30 IST stored as 05:30Z; true UTC is 00:00Z.
    let events: Vec<i64> = db
        .read(|conn| {
            let mut stmt = conn.prepare("SELECT timestamp FROM event ORDER BY timestamp")?;
            let rows = stmt.query_map([], |r| r.get::<_, i64>(0))?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
        .unwrap();
    let shift = -(TZ_OFFSET_MINUTES as i64) * 60_000;
    assert_eq!(
        events,
        vec![T0_WALL_CLOCK + shift, T0_WALL_CLOCK + 3_600_000 + shift],
        "event.timestamp normalized to true UTC"
    );
    assert_eq!(report.event_shift_ms, shift);
    assert!(report.normalized_timestamps > 0);

    // Other wall-clock columns shifted too (likedDate, inLibrary,
    // artist.lastUpdateTime, playlist dates).
    let liked_date: Option<i64> = db
        .read(|conn| {
            Ok(
                conn.query_row("SELECT likedDate FROM song WHERE id = 's1'", [], |r| {
                    r.get(0)
                })?,
            )
        })
        .unwrap();
    assert_eq!(liked_date, Some(T0_WALL_CLOCK + shift));

    // speed_dial_item.createDate stays TRUE UTC (no shift).
    let create_date: i64 = db
        .read(|conn| {
            Ok(conn.query_row(
                "SELECT createDate FROM speed_dial_item WHERE id = 's1'",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(create_date, T0_WALL_CLOCK, "true-UTC column untouched");

    // The timezone record landed in _mochy_meta.
    let meta = db
        .read(read_import_meta)
        .unwrap()
        .expect("import meta recorded");
    assert_eq!(meta["timezone_label"], "+05:30");
    assert_eq!(meta["source_user_version"], 35);
    assert_eq!(
        meta["event_timestamp_convention"],
        "true-utc (normalized from android wall-clock-as-utc)"
    );

    // --- Transform (b): queue reconstruction ---
    let queue = &report.queue;
    assert!(queue.paused);
    assert!(queue.source_blob_present, "the unreadable blob was seen");
    assert!(queue.shuffle_order_lost);
    assert_eq!(
        queue.current_song_id.as_deref(),
        Some("s1"),
        "most recent event's song"
    );
    assert_eq!(queue.songs.len(), 1, "one distinct song in history");
    assert_eq!(queue.songs[0].title, "Nightcall");
    assert_eq!(queue.songs[0].artists, vec!["Kavinsky".to_string()]);
    assert!(queue.songs[0].format.is_some(), "format cache rides along");
    // The record serializes for Phase 10.
    let json = serde_json::to_string(queue).unwrap();
    assert!(json.contains("\"paused\":true"));

    // --- Transform (c): downloads seed ---
    assert_eq!(report.downloads.entries.len(), 3);
    assert_eq!(report.downloads.completed, 2);
    assert_eq!(
        report.downloads_applied, 2,
        "s1 + gone marked; s2 FAILED clears"
    );
    let downloaded: Option<(i64, Option<i64>)> = db
        .read(|conn| {
            Ok(conn
                .query_row(
                    "SELECT isDownloaded, dateDownload FROM song WHERE id = 's1'",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?)
        })
        .unwrap();
    let (is_downloaded, date_download) = downloaded.unwrap();
    assert_eq!(is_downloaded, 1);
    assert_eq!(
        date_download,
        Some(1_700_000_000_000),
        "dateDownload from the index's true-UTC update_time_ms"
    );
    let title_in_seed = report
        .downloads
        .entries
        .iter()
        .find(|e| e.song_id == "s1")
        .unwrap();
    assert_eq!(
        title_in_seed.title.as_deref(),
        Some("Nightcall"),
        "request data blob = UTF-8 title"
    );
    assert_eq!(
        title_in_seed.custom_cache_key.as_deref(),
        Some("s1"),
        "cache key re-links exoplayer files"
    );

    // --- The imported database is queryable through the repositories ---
    let repos = mochy_db::Repositories::new(std::sync::Arc::new(db));
    let song = repos.songs.song("s1").unwrap().expect("imported song");
    assert_eq!(song.song.title, "Nightcall");
    assert_eq!(song.artists[0].name, "Kavinsky");
    assert_eq!(song.format.as_ref().unwrap().itag, 251);
    let events = repos
        .events
        .events_in_range(
            T0_WALL_CLOCK + shift - 1,
            T0_WALL_CLOCK + 3_600_000 + shift + 1,
        )
        .unwrap();
    assert_eq!(events.len(), 2);
    let playlists = repos.playlists.playlist("LP1").unwrap().expect("playlist");
    assert_eq!(playlists.song_count, 1);
    let lyrics = repos.lyrics.lyrics("s1").unwrap().unwrap();
    assert_eq!(lyrics.lyrics, mochy_db::LyricsEntity::LYRICS_NOT_FOUND);

    std::fs::remove_dir_all(&dir).ok();
}

/// The v45 round-trip: post-34 tables import too, user_version still 1.
#[tokio::test]
async fn imports_android_v45_database_with_post_35_tables() {
    let dir = temp_dir("v45");
    let source = dir.join("mochy.db");
    build_android_v45(&source);

    let target_path = dir.join("desktop.db");
    let db = Db::open(&target_path).await.unwrap();
    let report = import_android_db(
        &db,
        &source,
        ImportOptions {
            event_tz_offset_minutes: 0,
            android_files_dir: None,
            exoplayer_internal_db: None,
        },
    )
    .await
    .unwrap();

    assert_eq!(report.source_user_version, 45);
    assert_eq!(report.per_table["playlist_event"].copied, 1);
    assert_eq!(report.per_table["imported_song"].copied, 1);
    assert_eq!(report.per_table["import_session"].copied, 1);
    assert_eq!(report.per_table["import_session_song"].copied, 1);
    assert_eq!(report.per_table["song_edit"].copied, 1);

    // The desktop user_version stays at the v45-baseline 1.
    let version: i64 = db
        .read(|conn| Ok(conn.query_row("PRAGMA user_version", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(version, 1);

    // room_master_table never exists on the desktop side.
    assert!(
        !mochy_db::schema::table_exists(
            &Connection::open_with_flags(&target_path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap(),
            "room_master_table"
        )
        .unwrap()
    );

    // Orphan tolerance: imported_song rows survive a song deletion by
    // design (ImportedSong.kt: "Deliberately no foreign key").
    let orphans: i64 = db
        .read(|conn| {
            Ok(conn.query_row(
                "SELECT COUNT(*) FROM imported_song i LEFT JOIN song s ON s.id = i.songId WHERE s.id IS NULL",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(orphans, 0, "song still present here");

    std::fs::remove_dir_all(&dir).ok();
}

/// Version gate: below 35 the import refuses (Android wiped those).
#[tokio::test]
async fn rejects_versions_below_the_contiguous_range() {
    let dir = temp_dir("v34");
    let source = dir.join("mochy.db");
    build_android_v35(&source);
    let conn = Connection::open(&source).unwrap();
    conn.pragma_update(None, "user_version", 34_i64).unwrap();
    drop(conn);

    let db = Db::open(dir.join("desktop.db")).await.unwrap();
    let err = import_android_db(&db, &source, ImportOptions::default())
        .await
        .unwrap_err();
    match err {
        mochy_db::DbError::Import { reason, .. } => {
            assert!(
                reason.contains("outside the importable range 35..=45"),
                "{reason}"
            );
        }
        other => panic!("expected Import error, got {other:?}"),
    }
    std::fs::remove_dir_all(&dir).ok();
}

/// A watcher subscribed before the import sees the all-tables event after
/// the copy commits (one wake-up, not 24).
#[tokio::test]
async fn import_fires_one_all_tables_invalidation() {
    use mochy_db::watch::Table;

    let dir = temp_dir("watch");
    let source = dir.join("mochy.db");
    build_android_v35(&source);
    let files_dir = dir.join("files");
    std::fs::create_dir_all(&files_dir).unwrap();

    let db = Db::open(dir.join("desktop.db")).await.unwrap();
    let (rx, _guard) = db.watch(TableSet::of(&[Table::Song, Table::Event, Table::Artist]));

    import_android_db(
        &db,
        &source,
        ImportOptions {
            event_tz_offset_minutes: 0,
            android_files_dir: Some(files_dir),
            exoplayer_internal_db: None,
        },
    )
    .await
    .unwrap();

    let event = rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("import invalidation");
    assert!(event.contains(TableSet::of(&[Table::Song])));
    assert!(event.contains(TableSet::of(&[Table::Event])));
    // Exactly one coalesced event.
    assert!(
        rx.recv_timeout(std::time::Duration::from_millis(100))
            .is_err()
    );
    std::fs::remove_dir_all(&dir).ok();
}

use rusqlite::OptionalExtension;
