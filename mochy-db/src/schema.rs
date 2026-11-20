//! The v45 schema mirror: 24 tables + 3 views, DDL text kept byte-compatible
//! with Room's generated SQL so `sqlite_master.sql` of a desktop database
//! matches what Android builds from the same entities.
//!
//! Sources of truth (upstream, pin `bde01cb4`):
//! - `db/MusicDatabase.kt:110` — `MusicDatabaseVersion = 45`, entity list
//!   (24 entities) and view list (3 views); `InternalDatabase.newInstance`
//!   onOpen PRAGMAs (busy_timeout/cache_size/wal_autocheckpoint/synchronous)
//!   and `JournalMode.WRITE_AHEAD_LOGGING`.
//! - `db/entities/*.kt` — column names, types, nullability, defaults, indices.
//! - `app/schemas/com.mochyy.mochymusic.db.InternalDatabase/34.json` — the
//!   last exported Room schema JSON (verified: types + defaults + index DDL).
//! - `db/MusicDatabase.kt` hand-written migrations `MIGRATION_35_36` …
//!   `MIGRATION_44_45` — the v35→v45 additions reconstructed below
//!   (playlist_event, song.localUri, playlist.description, imported_song,
//!   import_session, import_session_song, imported_playlist,
//!   imported_playlist.remoteId/remoteUrl/lastSyncedAt,
//!   imported_playlist_track, song.addedByLike, song_edit, song.genre,
//!   imported_playlist.authorName/authorAvatarUrl/authorUrl).
//!
//! Versioning: the desktop database uses `PRAGMA user_version = 1` — the v45
//! DDL is the baseline (blueprint ch. 7.1). Room's `room_master_table` is
//! dropped and NOT created.

use std::path::Path;

use rusqlite::Connection;

use crate::error::{DbError, DbResult};

/// The 24 mirrored tables, in the order Android's `@Database(entities = […])`
/// declares them (MusicDatabase.kt:113-138).
pub const TABLES: &[&str] = &[
    "song",
    "artist",
    "album",
    "playlist",
    "song_artist_map",
    "song_album_map",
    "album_artist_map",
    "playlist_song_map",
    "search_history",
    "format",
    "lyrics",
    "event",
    "related_song_map",
    "set_video_id",
    "playCount",
    "recognition_history",
    "speed_dial_item",
    "playlist_event",
    "imported_song",
    "import_session",
    "import_session_song",
    "imported_playlist",
    "imported_playlist_track",
    "song_edit",
];

/// The 3 mirrored views (MusicDatabase.kt:139-143). Their SQL embeds the
/// ORDER BY the Android relation queries relied on; SQLite does not
/// guarantee view ordering, so consuming queries in [`crate::queries`]
/// re-state the ORDER BY themselves (blueprint ch. 7.1).
pub const VIEWS: &[&str] = &[
    "sorted_song_artist_map",
    "sorted_song_album_map",
    "playlist_song_map_preview",
];

/// Table DDL. Text mirrors Room's `CREATE TABLE IF NOT EXISTS` output
/// (compare the migration `db.execSQL` strings in MusicDatabase.kt:315-347
/// and the `createSql` fields of room_schema_v34.json).
const TABLE_DDL: &[&str] = &[
    // SongEntity.kt — 26 columns at v45 (23 at v34 + localUri + addedByLike + genre).
    // Index on albumId from the @Entity(indices = [Index(value = ["albumId"])]) — v34.json.
    "CREATE TABLE IF NOT EXISTS `song` (`id` TEXT NOT NULL, `title` TEXT NOT NULL, `duration` INTEGER NOT NULL, `thumbnailUrl` TEXT, `albumId` TEXT, `albumName` TEXT, `explicit` INTEGER NOT NULL DEFAULT 0, `year` INTEGER, `date` INTEGER, `dateModified` INTEGER, `liked` INTEGER NOT NULL, `likedDate` INTEGER, `totalPlayTime` INTEGER NOT NULL, `inLibrary` INTEGER, `dateDownload` INTEGER, `isLocal` INTEGER NOT NULL DEFAULT false, `libraryAddToken` TEXT, `libraryRemoveToken` TEXT, `lyricsOffset` INTEGER NOT NULL DEFAULT 0, `romanizeLyrics` INTEGER NOT NULL DEFAULT true, `isDownloaded` INTEGER NOT NULL DEFAULT 0, `isUploaded` INTEGER NOT NULL DEFAULT false, `isVideo` INTEGER NOT NULL DEFAULT false, `localUri` TEXT DEFAULT NULL, `addedByLike` INTEGER NOT NULL DEFAULT 0, `genre` TEXT, PRIMARY KEY(`id`))",
    // ArtistEntity.kt.
    "CREATE TABLE IF NOT EXISTS `artist` (`id` TEXT NOT NULL, `name` TEXT NOT NULL, `thumbnailUrl` TEXT, `channelId` TEXT, `lastUpdateTime` INTEGER NOT NULL, `bookmarkedAt` INTEGER, `isLocal` INTEGER NOT NULL DEFAULT false, PRIMARY KEY(`id`))",
    // AlbumEntity.kt — playlistId = the YT album playlist (OMV/RAAK ids).
    "CREATE TABLE IF NOT EXISTS `album` (`id` TEXT NOT NULL, `playlistId` TEXT, `title` TEXT NOT NULL, `year` INTEGER, `thumbnailUrl` TEXT, `themeColor` INTEGER, `songCount` INTEGER NOT NULL, `duration` INTEGER NOT NULL, `explicit` INTEGER NOT NULL DEFAULT 0, `lastUpdateTime` INTEGER NOT NULL, `bookmarkedAt` INTEGER, `likedDate` INTEGER, `inLibrary` INTEGER, `description` TEXT, `isLocal` INTEGER NOT NULL DEFAULT false, `isUploaded` INTEGER NOT NULL DEFAULT false, PRIMARY KEY(`id`))",
    // PlaylistEntity.kt.
    "CREATE TABLE IF NOT EXISTS `playlist` (`id` TEXT NOT NULL, `name` TEXT NOT NULL, `browseId` TEXT, `createdAt` INTEGER, `lastUpdateTime` INTEGER, `isEditable` INTEGER NOT NULL DEFAULT true, `bookmarkedAt` INTEGER, `remoteSongCount` INTEGER, `playEndpointParams` TEXT, `thumbnailUrl` TEXT, `shuffleEndpointParams` TEXT, `radioEndpointParams` TEXT, `isLocal` INTEGER NOT NULL DEFAULT false, `isAutoSync` INTEGER NOT NULL DEFAULT false, `description` TEXT, PRIMARY KEY(`id`))",
    // SongArtistMap.kt.
    "CREATE TABLE IF NOT EXISTS `song_artist_map` (`songId` TEXT NOT NULL, `artistId` TEXT NOT NULL, `position` INTEGER NOT NULL, PRIMARY KEY(`songId`, `artistId`), FOREIGN KEY(`songId`) REFERENCES `song`(`id`) ON UPDATE NO ACTION ON DELETE CASCADE , FOREIGN KEY(`artistId`) REFERENCES `artist`(`id`) ON UPDATE NO ACTION ON DELETE CASCADE )",
    // SongAlbumMap.kt — `index` is a SQLite keyword, quoted like Room does.
    "CREATE TABLE IF NOT EXISTS `song_album_map` (`songId` TEXT NOT NULL, `albumId` TEXT NOT NULL, `index` INTEGER NOT NULL, PRIMARY KEY(`songId`, `albumId`), FOREIGN KEY(`songId`) REFERENCES `song`(`id`) ON UPDATE NO ACTION ON DELETE CASCADE , FOREIGN KEY(`albumId`) REFERENCES `album`(`id`) ON UPDATE NO ACTION ON DELETE CASCADE )",
    // AlbumArtistMap.kt.
    "CREATE TABLE IF NOT EXISTS `album_artist_map` (`albumId` TEXT NOT NULL, `artistId` TEXT NOT NULL, `order` INTEGER NOT NULL, PRIMARY KEY(`albumId`, `artistId`), FOREIGN KEY(`albumId`) REFERENCES `album`(`id`) ON UPDATE NO ACTION ON DELETE CASCADE , FOREIGN KEY(`artistId`) REFERENCES `artist`(`id`) ON UPDATE NO ACTION ON DELETE CASCADE )",
    // PlaylistSongMap.kt — setVideoId rides along for YT playlist moves.
    "CREATE TABLE IF NOT EXISTS `playlist_song_map` (`id` INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL, `playlistId` TEXT NOT NULL, `songId` TEXT NOT NULL, `position` INTEGER NOT NULL, `setVideoId` TEXT, FOREIGN KEY(`playlistId`) REFERENCES `playlist`(`id`) ON UPDATE NO ACTION ON DELETE CASCADE , FOREIGN KEY(`songId`) REFERENCES `song`(`id`) ON UPDATE NO ACTION ON DELETE CASCADE )",
    // SearchHistory.kt — unique index on query.
    "CREATE TABLE IF NOT EXISTS `search_history` (`id` INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL, `query` TEXT NOT NULL)",
    // FormatEntity.kt — loudnessDb + perceptualLoudnessDb (perceptual added by MIGRATION_24_25).
    "CREATE TABLE IF NOT EXISTS `format` (`id` TEXT NOT NULL, `itag` INTEGER NOT NULL, `mimeType` TEXT NOT NULL, `codecs` TEXT NOT NULL, `bitrate` INTEGER NOT NULL, `sampleRate` INTEGER, `contentLength` INTEGER NOT NULL, `loudnessDb` REAL, `perceptualLoudnessDb` REAL, `playbackUrl` TEXT, PRIMARY KEY(`id`))",
    // LyricsEntity.kt — LYRICS_NOT_FOUND sentinel lives in the lyrics column;
    // provider + translation columns (MIGRATION_29_30 for provider).
    "CREATE TABLE IF NOT EXISTS `lyrics` (`id` TEXT NOT NULL, `lyrics` TEXT NOT NULL, `provider` TEXT NOT NULL DEFAULT 'Unknown', `translatedLyrics` TEXT NOT NULL DEFAULT '', `translationLanguage` TEXT NOT NULL DEFAULT '', `translationMode` TEXT NOT NULL DEFAULT '', PRIMARY KEY(`id`))",
    // Event.kt — see importer::event_time for the timestamp convention.
    "CREATE TABLE IF NOT EXISTS `event` (`id` INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL, `songId` TEXT NOT NULL, `timestamp` INTEGER NOT NULL, `playTime` INTEGER NOT NULL, FOREIGN KEY(`songId`) REFERENCES `song`(`id`) ON UPDATE NO ACTION ON DELETE CASCADE )",
    // RelatedSongMap.kt.
    "CREATE TABLE IF NOT EXISTS `related_song_map` (`id` INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL, `songId` TEXT NOT NULL, `relatedSongId` TEXT NOT NULL, FOREIGN KEY(`songId`) REFERENCES `song`(`id`) ON UPDATE NO ACTION ON DELETE CASCADE , FOREIGN KEY(`relatedSongId`) REFERENCES `song`(`id`) ON UPDATE NO ACTION ON DELETE CASCADE )",
    // SetVideoIdEntity.kt.
    "CREATE TABLE IF NOT EXISTS `set_video_id` (`videoId` TEXT NOT NULL, `setVideoId` TEXT, PRIMARY KEY(`videoId`))",
    // PlayCountEntity.kt — keyed (song, year, month); table name is camelCase.
    "CREATE TABLE IF NOT EXISTS `playCount` (`song` TEXT NOT NULL, `year` INTEGER NOT NULL, `month` INTEGER NOT NULL, `count` INTEGER NOT NULL, PRIMARY KEY(`song`, `year`, `month`))",
    // RecognitionHistory.kt — index on trackId.
    "CREATE TABLE IF NOT EXISTS `recognition_history` (`id` INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL, `trackId` TEXT NOT NULL, `title` TEXT NOT NULL, `artist` TEXT NOT NULL, `album` TEXT, `coverArtUrl` TEXT, `coverArtHqUrl` TEXT, `genre` TEXT, `releaseDate` TEXT, `label` TEXT, `shazamUrl` TEXT, `appleMusicUrl` TEXT, `spotifyUrl` TEXT, `isrc` TEXT, `youtubeVideoId` TEXT, `recognizedAt` INTEGER NOT NULL, `liked` INTEGER NOT NULL)",
    // SpeedDialItem.kt.
    "CREATE TABLE IF NOT EXISTS `speed_dial_item` (`id` TEXT NOT NULL, `secondaryId` TEXT, `title` TEXT NOT NULL, `subtitle` TEXT, `thumbnailUrl` TEXT, `type` TEXT NOT NULL, `explicit` INTEGER NOT NULL, `createDate` INTEGER NOT NULL, PRIMARY KEY(`id`))",
    // MIGRATION_35_36 DDL, verbatim.
    "CREATE TABLE IF NOT EXISTS `playlist_event` (`id` INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL, `playlistId` TEXT NOT NULL, `timestamp` INTEGER NOT NULL, FOREIGN KEY(`playlistId`) REFERENCES `playlist`(`id`) ON UPDATE NO ACTION ON DELETE CASCADE )",
    // ImportedSong.kt — NO foreign key, by design (songs are replaced).
    // DDL from MIGRATION_38_39, verbatim.
    "CREATE TABLE IF NOT EXISTS `imported_song` (`songId` TEXT NOT NULL, `source` TEXT NOT NULL, `importedAt` INTEGER NOT NULL, PRIMARY KEY(`songId`, `source`))",
    // ImportSession.kt — DDL from MIGRATION_39_40, verbatim.
    "CREATE TABLE IF NOT EXISTS `import_session` (`id` INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL, `source` TEXT NOT NULL, `startedAt` INTEGER NOT NULL)",
    // ImportSessionSong.kt — no FK; DDL from MIGRATION_39_40, verbatim.
    "CREATE TABLE IF NOT EXISTS `import_session_song` (`sessionId` INTEGER NOT NULL, `songId` TEXT NOT NULL, `addedToLibrary` INTEGER NOT NULL, `liked` INTEGER NOT NULL, PRIMARY KEY(`sessionId`, `songId`))",
    // ImportedPlaylist.kt — no FK; base DDL from MIGRATION_39_40, remote
    // columns from MIGRATION_40_41, author columns from MIGRATION_44_45.
    "CREATE TABLE IF NOT EXISTS `imported_playlist` (`playlistId` TEXT NOT NULL, `source` TEXT NOT NULL, `sessionId` INTEGER, `importedAt` INTEGER NOT NULL, `remoteId` TEXT, `remoteUrl` TEXT, `lastSyncedAt` INTEGER, `authorName` TEXT, `authorAvatarUrl` TEXT, `authorUrl` TEXT, PRIMARY KEY(`playlistId`))",
    // ImportedPlaylistTrack.kt — MIGRATION_40_41 DDL, verbatim.
    "CREATE TABLE IF NOT EXISTS `imported_playlist_track` (`playlistId` TEXT NOT NULL, `trackKey` TEXT NOT NULL, PRIMARY KEY(`playlistId`, `trackKey`))",
    // SongEdit.kt — artists is a JSON list (EditedArtist); MIGRATION_42_43 DDL.
    "CREATE TABLE IF NOT EXISTS `song_edit` (`songId` TEXT NOT NULL, `title` TEXT, `artists` TEXT, `album` TEXT, `year` INTEGER, `cover` TEXT, `videoId` TEXT, `noVideo` INTEGER NOT NULL DEFAULT 0, PRIMARY KEY(`songId`))",
];

/// Index DDL — names are Room's `index_<table>_<column>` convention
/// (v34.json `createSql` + the post-34 migration `CREATE INDEX` lines).
const INDEX_DDL: &[&str] = &[
    // song
    "CREATE INDEX IF NOT EXISTS `index_song_albumId` ON `song` (`albumId`)",
    // song_artist_map
    "CREATE INDEX IF NOT EXISTS `index_song_artist_map_songId` ON `song_artist_map` (`songId`)",
    "CREATE INDEX IF NOT EXISTS `index_song_artist_map_artistId` ON `song_artist_map` (`artistId`)",
    // song_album_map
    "CREATE INDEX IF NOT EXISTS `index_song_album_map_songId` ON `song_album_map` (`songId`)",
    "CREATE INDEX IF NOT EXISTS `index_song_album_map_albumId` ON `song_album_map` (`albumId`)",
    // album_artist_map
    "CREATE INDEX IF NOT EXISTS `index_album_artist_map_albumId` ON `album_artist_map` (`albumId`)",
    "CREATE INDEX IF NOT EXISTS `index_album_artist_map_artistId` ON `album_artist_map` (`artistId`)",
    // playlist_song_map
    "CREATE INDEX IF NOT EXISTS `index_playlist_song_map_playlistId` ON `playlist_song_map` (`playlistId`)",
    "CREATE INDEX IF NOT EXISTS `index_playlist_song_map_songId` ON `playlist_song_map` (`songId`)",
    // search_history (unique)
    "CREATE UNIQUE INDEX IF NOT EXISTS `index_search_history_query` ON `search_history` (`query`)",
    // event
    "CREATE INDEX IF NOT EXISTS `index_event_songId` ON `event` (`songId`)",
    // related_song_map
    "CREATE INDEX IF NOT EXISTS `index_related_song_map_songId` ON `related_song_map` (`songId`)",
    "CREATE INDEX IF NOT EXISTS `index_related_song_map_relatedSongId` ON `related_song_map` (`relatedSongId`)",
    // recognition_history
    "CREATE INDEX IF NOT EXISTS `index_recognition_history_trackId` ON `recognition_history` (`trackId`)",
    // MIGRATION_35_36
    "CREATE INDEX IF NOT EXISTS `index_playlist_event_playlistId` ON `playlist_event` (`playlistId`)",
    // MIGRATION_38_39
    "CREATE INDEX IF NOT EXISTS `index_imported_song_source` ON `imported_song` (`source`)",
    // MIGRATION_39_40
    "CREATE INDEX IF NOT EXISTS `index_import_session_song_songId` ON `import_session_song` (`songId`)",
    "CREATE INDEX IF NOT EXISTS `index_imported_playlist_source` ON `imported_playlist` (`source`)",
];

/// View DDL — verbatim from the @DatabaseView values
/// (SortedSongArtistMap.kt, SortedSongAlbumMap.kt, PlaylistSongMapPreview.kt).
const VIEW_DDL: &[&str] = &[
    "CREATE VIEW IF NOT EXISTS `sorted_song_artist_map` AS SELECT * FROM song_artist_map ORDER BY position",
    "CREATE VIEW IF NOT EXISTS `sorted_song_album_map` AS SELECT * FROM song_album_map ORDER BY `index`",
    "CREATE VIEW IF NOT EXISTS `playlist_song_map_preview` AS SELECT * FROM playlist_song_map WHERE position <= 3 ORDER BY position",
];

/// The desktop schema version of the v45 baseline DDL.
///
/// Fresh databases start at 1 (blueprint ch. 7.1: "PRAGMA user_version
/// starting at 1 with the v45 DDL as the baseline"). Android's 35..45 range
/// maps onto this baseline through the importer; room_master_table does not
/// exist on desktop.
pub const USER_VERSION: i64 = 1;

/// The accepted Android `user_version` range for the importer
/// (only 35→45 has a contiguous non-destructive upgrade path — verified
/// fact #6; older upgrades fall into Room's destructive reset on Android
/// itself, MusicDatabase.kt `.fallbackToDestructiveMigration`).
pub const ANDROID_IMPORTABLE_MIN_VERSION: i64 = 35;
pub const ANDROID_IMPORTABLE_MAX_VERSION: i64 = 45;

/// Connection PRAGMAs, from `InternalDatabase.newInstance`'s onOpen callback
/// (MusicDatabase.kt:183-190) plus Room's FK enforcement and WAL journal
/// mode. `foreign_keys` is enabled because Room turns constraint enforcement
/// on for schemas that declare them; the importer is the one caller that
/// deliberately runs with it off.
pub fn apply_pragmas(conn: &Connection) -> DbResult<()> {
    // JournalMode.WRITE_AHEAD_LOGGING — one writer + concurrent readers.
    let _: String = conn.query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))?;
    conn.pragma_update(None, "busy_timeout", 60_000_i64)?;
    conn.pragma_update(None, "cache_size", -16_000_i64)?;
    conn.pragma_update(None, "wal_autocheckpoint", 1_000_i64)?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    Ok(())
}

/// Create the v45 baseline schema on `conn` and stamp `user_version = 1`.
///
/// Idempotent (`IF NOT EXISTS` everywhere). Fails with [`DbError::BadSchema`]
/// if the file carries an unknown `user_version` — a future desktop version
/// would carry its own migration chain, which does not exist yet.
pub fn apply(conn: &Connection, path: &Path) -> DbResult<()> {
    let current: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    match current {
        0 => {
            for ddl in TABLE_DDL.iter().chain(VIEW_DDL).chain(INDEX_DDL) {
                conn.execute_batch(ddl)?;
            }
            conn.pragma_update(None, "user_version", USER_VERSION)?;
        }
        USER_VERSION => {
            // Already a desktop database — verify the mirror is intact so a
            // truncated/hand-edited file fails loudly instead of at query time.
            for table in TABLES {
                if !table_exists(conn, table)? {
                    return Err(DbError::BadSchema {
                        path: path.to_owned(),
                        reason: format!("user_version 1 but table `{table}` is missing"),
                    });
                }
            }
        }
        other => {
            return Err(DbError::BadSchema {
                path: path.to_owned(),
                reason: format!(
                    "unsupported user_version {other} (desktop schema is at {USER_VERSION})"
                ),
            });
        }
    }
    Ok(())
}

/// Whether a table (or view) exists.
pub fn object_exists(conn: &Connection, name: &str) -> DbResult<bool> {
    let sql = "SELECT COUNT(1) FROM sqlite_master WHERE name = ?1";
    let n: i64 = conn.query_row(sql, [name], |row| row.get(0))?;
    Ok(n > 0)
}

/// Whether a *table* exists (views excluded).
pub fn table_exists(conn: &Connection, name: &str) -> DbResult<bool> {
    let sql = "SELECT COUNT(1) FROM sqlite_master WHERE type = 'table' AND name = ?1";
    let n: i64 = conn.query_row(sql, [name], |row| row.get(0))?;
    Ok(n > 0)
}

/// One column of a table as SQLite sees it (`PRAGMA table_info`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnInfo {
    /// Column name (identical to Android's, by contract).
    pub name: String,
    /// Declared type (TEXT / INTEGER / REAL).
    pub decl_type: String,
    /// NOT NULL flag.
    pub not_null: bool,
    /// Default value expression, if any.
    pub default: Option<String>,
}

/// Column list of a table, in declaration order.
pub fn columns(conn: &Connection, table: &str) -> DbResult<Vec<ColumnInfo>> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info(`{table}`)"))?;
    let rows = stmt.query_map([], |row| {
        Ok(ColumnInfo {
            name: row.get::<_, String>(1)?,
            decl_type: row.get::<_, String>(2)?,
            not_null: row.get::<_, i64>(3)? != 0,
            default: row.get::<_, Option<String>>(4)?,
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// Introspection report: every table with its columns, and the view list.
/// The schema-report test prints this (24 tables + 3 views with counts).
#[derive(Debug, Clone)]
pub struct SchemaReport {
    /// `(table, columns)` in mirror order.
    pub tables: Vec<(String, Vec<ColumnInfo>)>,
    /// View names in mirror order.
    pub views: Vec<String>,
}

/// Build the schema report for a database.
pub fn report(conn: &Connection) -> DbResult<SchemaReport> {
    let mut tables = Vec::with_capacity(TABLES.len());
    for table in TABLES {
        let cols = columns(conn, table)?;
        if cols.is_empty() {
            return Err(DbError::BadSchema {
                path: Path::new(":memory:").to_owned(),
                reason: format!("table `{table}` has no columns"),
            });
        }
        tables.push((table.to_string(), cols));
    }
    let mut views = Vec::with_capacity(VIEWS.len());
    for view in VIEWS {
        if !object_exists(conn, view)? {
            return Err(DbError::BadSchema {
                path: Path::new(":memory:").to_owned(),
                reason: format!("view `{view}` is missing"),
            });
        }
        views.push(view.to_string());
    }
    Ok(SchemaReport { tables, views })
}
