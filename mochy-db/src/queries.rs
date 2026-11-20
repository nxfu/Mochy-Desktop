//! The sync SQL layer: every query the repositories expose, over a plain
//! `&Connection`. The async Db actor and the importer both call into these.
//!
//! Every function carries an upstream citation (file + line at pin
//! `bde01cb4`). Where Android relied on one of the 3 view-sorted views
//! (`sorted_song_artist_map`, `sorted_song_album_map`,
//! `playlist_song_map_preview`), the ORDER BY is written into the consuming
//! query — SQLite does not guarantee view ordering (verified fact; blueprint
//! ch. 7.1), so the port cannot lean on the view's internal ORDER BY.
//!
//! Sort enums mirror `constants/PreferenceKeys.kt:524-583`
//! (SongSortType / ArtistSortType / AlbumSortType / PlaylistSortType).

use rusqlite::{Connection, OptionalExtension, Row};

use crate::entities::*;
use crate::error::{DbError, DbResult};

// ---------------------------------------------------------------------------
// Sort types (constants/PreferenceKeys.kt:524-583)
// ---------------------------------------------------------------------------

/// `SongSortType` (PreferenceKeys.kt:524-529).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SongSort {
    #[default]
    CreateDate,
    Name,
    Artist,
    PlayTime,
}

/// `ArtistSortType` (PreferenceKeys.kt:555-560).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ArtistSort {
    #[default]
    CreateDate,
    Name,
    SongCount,
    PlayTime,
}

/// `AlbumSortType` (PreferenceKeys.kt:568-576).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AlbumSort {
    #[default]
    CreateDate,
    Name,
    Artist,
    Year,
    SongCount,
    Length,
    PlayTime,
}

/// `PlaylistSortType` (PreferenceKeys.kt:578-583).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlaylistSort {
    #[default]
    CreateDate,
    Name,
    SongCount,
    LastUpdated,
}

// ---------------------------------------------------------------------------
// Relation fragments (the view consumers)
// ---------------------------------------------------------------------------

/// A song's artists, ordered by `song_artist_map.position`.
///
/// CONSUMING QUERY for the `sorted_song_artist_map` view: upstream's
/// `Song` relation joins through
/// `Junction(SortedSongArtistMap::class)` (entities/Song.kt:12-22), and the
/// view supplies `ORDER BY position`
/// (entities/SortedSongArtistMap.kt:6-9). The view's ordering is inlined
/// here because SQLite does not guarantee view ordering.
pub fn song_artists(conn: &Connection, song_id: &str) -> DbResult<Vec<ArtistEntity>> {
    let mut stmt = conn.prepare(
        "SELECT artist.* FROM song_artist_map \
         JOIN artist ON artist.id = song_artist_map.artistId \
         WHERE song_artist_map.songId = ?1 \
         ORDER BY song_artist_map.position",
    )?;
    let rows = stmt.query_map([song_id], ArtistEntity::from_row)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// A song's album (first `song_album_map` row's album).
///
/// Upstream: `Song.album` relation, `Junction(SongAlbumMap::class)`
/// (entities/Song.kt:23-31). Room emits no ORDER BY; the map has one row
/// per (song, album) pair, so `LIMIT 1` is exact.
pub fn song_album(conn: &Connection, song_id: &str) -> DbResult<Option<AlbumEntity>> {
    let mut stmt = conn.prepare(
        "SELECT album.* FROM song_album_map \
         JOIN album ON album.id = song_album_map.albumId \
         WHERE song_album_map.songId = ?1 LIMIT 1",
    )?;
    stmt.query_row([song_id], AlbumEntity::from_row)
        .optional()
        .map_err(DbError::Sqlite)
}

/// A song's cached stream format.
///
/// Upstream: `Song.format` relation (entities/Song.kt:33-36) — plain PK
/// projection onto the `format` table.
pub fn song_format(conn: &Connection, song_id: &str) -> DbResult<Option<FormatEntity>> {
    let mut stmt = conn.prepare("SELECT * FROM format WHERE id = ?1")?;
    stmt.query_row([song_id], FormatEntity::from_row)
        .optional()
        .map_err(DbError::Sqlite)
}

/// Assembles the full `Song` relation (song + artists + album + format).
///
/// Upstream: the `@Transaction` wrapper Room puts around `Flow<List<Song>>`
/// queries (db/DatabaseDao.kt:76-77 etc.).
pub fn song_with_relations(conn: &Connection, song: SongEntity) -> DbResult<Song> {
    Ok(Song {
        artists: song_artists(conn, &song.id)?,
        album: song_album(conn, &song.id)?,
        format: song_format(conn, &song.id)?,
        song,
    })
}

fn query_songs(
    conn: &Connection,
    where_clause: &str,
    order_by: &str,
    params: &[&dyn rusqlite::ToSql],
) -> DbResult<Vec<Song>> {
    let sql = format!("SELECT song.* FROM song WHERE {where_clause} ORDER BY {order_by}");
    let mut stmt = conn.prepare(&sql)?;
    let ids: Vec<SongEntity> = {
        let rows = stmt.query_map(params, SongEntity::from_row)?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    ids.into_iter()
        .map(|song| song_with_relations(conn, song))
        .collect()
}

/// The song-list query family. `desc` reverses, matching the Kotlin
/// `.map { it.reversed(descending) }` pattern (DatabaseDao.kt:112 etc.).
///
/// The upstream `NAME`/`ARTIST` sorts re-sort in Kotlin with a
/// locale `Collator` (PRIMARY strength). SQLite `ORDER BY` is byte order;
/// script-aware collation is a UI-layer concern in the port (the
/// mochy-ui-kit index rail owns it) — recorded in HANDOFF Phase 03.
fn song_order_by(sort: SongSort, desc: bool) -> &'static str {
    match (sort, desc) {
        // songsByRowIdAsc / songsByCreateDateAsc (DatabaseDao.kt:77-82)
        (SongSort::CreateDate, false) => "inLibrary",
        (SongSort::CreateDate, true) => "inLibrary DESC",
        // songsByNameAsc (DatabaseDao.kt:85-86)
        (SongSort::Name, false) => "title",
        (SongSort::Name, true) => "title DESC",
        // ARTIST sorts by artists-then-album in Kotlin; rowId is the base
        // feed (songsByRowIdAsc, DatabaseDao.kt:77-78) before that Kotlin
        // pass. The port returns the base order; UI re-sorts like upstream.
        (SongSort::Artist, false) => "rowId",
        (SongSort::Artist, true) => "rowId DESC",
        // songsByPlayTimeAsc (DatabaseDao.kt:89-90)
        (SongSort::PlayTime, false) => "totalPlayTime",
        (SongSort::PlayTime, true) => "totalPlayTime DESC",
    }
}

// ---------------------------------------------------------------------------
// Song queries (DatabaseDao.kt:76-232)
// ---------------------------------------------------------------------------

/// `songs(...)` — library songs (DatabaseDao.kt:92-124;
/// `WHERE inLibrary IS NOT NULL`).
pub fn songs_in_library(conn: &Connection, sort: SongSort, desc: bool) -> DbResult<Vec<Song>> {
    query_songs(
        conn,
        "inLibrary IS NOT NULL",
        song_order_by(sort, desc),
        &[],
    )
}

/// `likedSongs(...)` (DatabaseDao.kt:176-224; `WHERE liked`).
pub fn liked_songs(conn: &Connection, sort: SongSort, desc: bool) -> DbResult<Vec<Song>> {
    query_songs(conn, "liked", song_order_by(sort, desc), &[])
}

/// `likedSongsCount()` (DatabaseDao.kt:226-228).
pub fn liked_songs_count(conn: &Connection) -> DbResult<i64> {
    Ok(conn.query_row("SELECT COUNT(1) FROM song WHERE liked", [], |r| r.get(0))?)
}

/// `localSongs(...)` (DatabaseDao.kt:126-174; `WHERE isLocal = 1`).
pub fn local_songs(conn: &Connection, sort: SongSort, desc: bool) -> DbResult<Vec<Song>> {
    query_songs(conn, "isLocal = 1", song_order_by(sort, desc), &[])
}

/// `downloadedSongs(...)` (DatabaseDao.kt:1280-1306;
/// `WHERE isDownloaded = 1`, CREATE_DATE sorts by dateDownload).
pub fn downloaded_songs(conn: &Connection, sort: SongSort, desc: bool) -> DbResult<Vec<Song>> {
    let order = match (sort, desc) {
        (SongSort::CreateDate, false) => "dateDownload",
        (SongSort::CreateDate, true) => "dateDownload DESC",
        (SongSort::Name, false) => "title",
        (SongSort::Name, true) => "title DESC",
        (SongSort::Artist, false) => "title",
        (SongSort::Artist, true) => "title DESC",
        (SongSort::PlayTime, false) => "totalPlayTime",
        (SongSort::PlayTime, true) => "totalPlayTime DESC",
    };
    query_songs(conn, "isDownloaded = 1", order, &[])
}

/// `song(id)` — full relation for one song.
pub fn song(conn: &Connection, id: &str) -> DbResult<Option<Song>> {
    song_entity(conn, id)?.map_or(Ok(None), |s| Ok(Some(song_with_relations(conn, s)?)))
}

/// `getSongByIdBlocking`-shaped raw entity read.
pub fn song_entity(conn: &Connection, id: &str) -> DbResult<Option<SongEntity>> {
    conn.query_row(
        "SELECT * FROM song WHERE id = ?1",
        [id],
        SongEntity::from_row,
    )
    .optional()
    .map_err(DbError::Sqlite)
}

/// `allSongs()` (DatabaseDao.kt:720-721).
pub fn all_songs(conn: &Connection) -> DbResult<Vec<Song>> {
    query_songs(conn, "1", "rowId", &[])
}

/// `albumSongs(albumId)` (DatabaseDao.kt:230-232).
///
/// CONSUMING QUERY for the `sorted_song_album_map` view: the
/// `AlbumWithSongs.songs` relation joins through
/// `Junction(SortedSongAlbumMap::class)` (entities/AlbumWithSongs.kt:22-29)
/// whose view supplies `ORDER BY \`index\``
/// (entities/SortedSongAlbumMap.kt:6-9). Inlined here (view ordering is
/// not guaranteed); the `rowid` tie-break matches the upstream query text.
pub fn album_songs(conn: &Connection, album_id: &str) -> DbResult<Vec<Song>> {
    let mut stmt = conn.prepare(
        "SELECT song.* FROM song JOIN song_album_map ON song.id = song_album_map.songId \
         WHERE song_album_map.albumId = ?1 \
         ORDER BY song_album_map.`index`, song_album_map.rowid",
    )?;
    let songs: Vec<SongEntity> = {
        let rows = stmt.query_map([album_id], SongEntity::from_row)?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    songs
        .into_iter()
        .map(|s| song_with_relations(conn, s))
        .collect()
}

/// `artistSongsBy*Asc` family (DatabaseDao.kt:241-290).
pub fn artist_songs(
    conn: &Connection,
    artist_id: &str,
    sort: SongSort,
    desc: bool,
) -> DbResult<Vec<Song>> {
    let order = match (sort, desc) {
        (SongSort::CreateDate, false) => "inLibrary",
        (SongSort::CreateDate, true) => "inLibrary DESC",
        (SongSort::Name, false) => "title",
        (SongSort::Name, true) => "title DESC",
        (SongSort::PlayTime, false) => "totalPlayTime",
        (SongSort::PlayTime, true) => "totalPlayTime DESC",
        (SongSort::Artist, false) => "inLibrary",
        (SongSort::Artist, true) => "inLibrary DESC",
    };
    let sql = format!(
        "SELECT song.* FROM song_artist_map JOIN song ON song_artist_map.songId = song.id \
         WHERE artistId = ?1 AND inLibrary IS NOT NULL ORDER BY {order}"
    );
    let mut stmt = conn.prepare(&sql)?;
    let songs: Vec<SongEntity> = {
        let rows = stmt.query_map([artist_id], SongEntity::from_row)?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    songs
        .into_iter()
        .map(|s| song_with_relations(conn, s))
        .collect()
}

/// `artistSongsPreview` (DatabaseDao.kt:322-327).
pub fn artist_songs_preview(
    conn: &Connection,
    artist_id: &str,
    preview_size: i64,
) -> DbResult<Vec<Song>> {
    let mut stmt = conn.prepare(
        "SELECT song.* FROM song_artist_map JOIN song ON song_artist_map.songId = song.id \
         WHERE artistId = ?1 AND inLibrary IS NOT NULL LIMIT ?2",
    )?;
    let songs: Vec<SongEntity> = {
        let rows = stmt.query_map(
            rusqlite::params![artist_id, preview_size],
            SongEntity::from_row,
        )?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    songs
        .into_iter()
        .map(|s| song_with_relations(conn, s))
        .collect()
}

/// `likedSongIdsSince(since)` (DatabaseDao.kt:1211-1213) — "in the same
/// stored form as likedDate".
pub fn liked_song_ids_since(conn: &Connection, since: i64) -> DbResult<Vec<String>> {
    let mut stmt = conn.prepare("SELECT id FROM song WHERE liked = 1 AND likedDate > ?1")?;
    let rows = stmt.query_map([since], |r| r.get::<_, String>(0))?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// `insert(song: SongEntity)` — `@Insert(onConflict = IGNORE)`
/// (DatabaseDao.kt:1550-1551). Returns whether the row was inserted.
pub fn insert_song(conn: &Connection, song: &SongEntity) -> DbResult<bool> {
    let cols = SongEntity::COLUMNS;
    let placeholders = cols.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
    let sql = format!(
        "INSERT OR IGNORE INTO song ({}) VALUES ({})",
        cols.iter()
            .map(|c| format!("`{c}`"))
            .collect::<Vec<_>>()
            .join(", "),
        placeholders
    );
    let n = conn.execute(
        &sql,
        rusqlite::params![
            song.id,
            song.title,
            song.duration,
            song.thumbnail_url,
            song.album_id,
            song.album_name,
            song.explicit,
            song.year,
            song.date,
            song.date_modified,
            song.liked,
            song.liked_date,
            song.total_play_time,
            song.in_library,
            song.date_download,
            song.is_local,
            song.library_add_token,
            song.library_remove_token,
            song.lyrics_offset,
            song.romanize_lyrics,
            song.is_downloaded,
            song.is_uploaded,
            song.is_video,
            song.local_uri,
            song.added_by_like,
            song.genre,
        ],
    )?;
    Ok(n == 1)
}

/// `update(song: SongEntity)` — `@Update` full-row rewrite
/// (DatabaseDao.kt:1724).
pub fn update_song(conn: &Connection, song: &SongEntity) -> DbResult<()> {
    let sets = SongEntity::COLUMNS
        .iter()
        .filter(|c| **c != "id")
        .map(|c| format!("`{c}` = ?"))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!("UPDATE song SET {sets} WHERE id = ?");
    conn.execute(
        &sql,
        rusqlite::params![
            song.title,
            song.duration,
            song.thumbnail_url,
            song.album_id,
            song.album_name,
            song.explicit,
            song.year,
            song.date,
            song.date_modified,
            song.liked,
            song.liked_date,
            song.total_play_time,
            song.in_library,
            song.date_download,
            song.is_local,
            song.library_add_token,
            song.library_remove_token,
            song.lyrics_offset,
            song.romanize_lyrics,
            song.is_downloaded,
            song.is_uploaded,
            song.is_video,
            song.local_uri,
            song.added_by_like,
            song.genre,
            song.id,
        ],
    )?;
    Ok(())
}

/// `SongEntity.toggleLike()` (SongEntity.kt:64-72) — the STATE MACHINE only.
///
/// liked flips; likedDate stamps on like, clears on unlike; a like also
/// backfills inLibrary and sets addedByLike when the song had no library
/// date. The upstream method then fires `YouTube.likeVideo` on a detached
/// coroutine — that network side-effect is OUT of the persistence layer by
/// design (blueprint ch. 7.3) and belongs to the application SongService
/// (Phase 14). Hook: the service calls this function for the row change and
/// issues the like/unlike call itself.
pub fn toggle_like(conn: &Connection, id: &str, now_ms: i64) -> DbResult<Option<SongEntity>> {
    let Some(mut song) = song_entity(conn, id)? else {
        return Ok(None);
    };
    song.liked = !song.liked;
    song.liked_date = if song.liked { Some(now_ms) } else { None };
    if song.liked && song.in_library.is_none() {
        song.in_library = Some(now_ms);
        song.added_by_like = true;
    }
    update_song(conn, &song)?;
    Ok(Some(song))
}

/// `SongEntity.localToggleLike()` (SongEntity.kt:55-61) — same flip without
/// the library backfill (local songs never sync).
pub fn local_toggle_like(conn: &Connection, id: &str, now_ms: i64) -> DbResult<Option<SongEntity>> {
    let Some(mut song) = song_entity(conn, id)? else {
        return Ok(None);
    };
    song.liked = !song.liked;
    song.liked_date = if song.liked { Some(now_ms) } else { None };
    update_song(conn, &song)?;
    Ok(Some(song))
}

/// `SongEntity.toggleLibrary(syncToYouTube = false, byLike)`
/// (SongEntity.kt:74-88) — STATE MACHINE only; the YouTube library-toggle
/// call is the Phase 14 SongService hook.
///
/// Upstream on ADD (`inLibrary == null`): `liked` and `likedDate` are LEFT
/// UNCHANGED (`liked = if (inLibrary == null) liked else false`), only
/// `inLibrary` is stamped and `addedByLike = byLike`. On REMOVE: `liked`
/// and `likedDate` are cleared.
pub fn toggle_library(
    conn: &Connection,
    id: &str,
    now_ms: i64,
    by_like: bool,
) -> DbResult<Option<SongEntity>> {
    let Some(mut song) = song_entity(conn, id)? else {
        return Ok(None);
    };
    if song.in_library.is_none() {
        // Adding: liked/likedDate keep their current values (SongEntity.kt:76-78).
        song.in_library = Some(now_ms);
        song.added_by_like = by_like;
    } else {
        // Removing: the song is no longer liked either (SongEntity.kt:76-79).
        song.liked = false;
        song.in_library = None;
        song.liked_date = None;
        song.added_by_like = false;
    }
    update_song(conn, &song)?;
    Ok(Some(song))
}

/// `inLibrary(songId, inLibrary)` (DatabaseDao.kt:1481-1485).
pub fn set_in_library(conn: &Connection, id: &str, in_library: Option<i64>) -> DbResult<()> {
    conn.execute(
        "UPDATE song SET inLibrary = ?1, addedByLike = 0 WHERE id = ?2",
        rusqlite::params![in_library, id],
    )?;
    Ok(())
}

/// `addLibraryTokens` (DatabaseDao.kt:1487-1492).
pub fn set_library_tokens(
    conn: &Connection,
    id: &str,
    add_token: Option<&str>,
    remove_token: Option<&str>,
) -> DbResult<()> {
    conn.execute(
        "UPDATE song SET libraryAddToken = ?1, libraryRemoveToken = ?2 WHERE id = ?3",
        rusqlite::params![add_token, remove_token, id],
    )?;
    Ok(())
}

/// `incrementTotalPlayTime` (DatabaseDao.kt:1573-1574).
pub fn increment_total_play_time(conn: &Connection, id: &str, play_time_ms: i64) -> DbResult<()> {
    conn.execute(
        "UPDATE song SET totalPlayTime = totalPlayTime + ?1 WHERE id = ?2",
        rusqlite::params![play_time_ms, id],
    )?;
    Ok(())
}

/// `updateLyricsOffset` (DatabaseDao.kt:1726-1727).
pub fn set_lyrics_offset(conn: &Connection, id: &str, offset_ms: i32) -> DbResult<()> {
    conn.execute(
        "UPDATE song SET lyricsOffset = ?1 WHERE id = ?2",
        rusqlite::params![offset_ms, id],
    )?;
    Ok(())
}

/// `updateDownloadedInfo` (DatabaseDao.kt:1308-1309).
pub fn update_downloaded_info(
    conn: &Connection,
    id: &str,
    downloaded: bool,
    date: Option<i64>,
) -> DbResult<()> {
    conn.execute(
        "UPDATE song SET isDownloaded = ?1, dateDownload = ?2 WHERE id = ?3",
        rusqlite::params![downloaded, date, id],
    )?;
    Ok(())
}

/// `setSongGenre` (DatabaseDao.kt:1145-1146).
pub fn set_song_genre(conn: &Connection, id: &str, genre: Option<&str>) -> DbResult<()> {
    conn.execute(
        "UPDATE song SET genre = ?1 WHERE id = ?2",
        rusqlite::params![genre, id],
    )?;
    Ok(())
}

/// `localSongsWithGenre` (DatabaseDao.kt:1148-1150).
pub fn local_songs_with_genre(conn: &Connection) -> DbResult<Vec<Song>> {
    query_songs(
        conn,
        "isLocal AND genre IS NOT NULL AND genre != ''",
        "title COLLATE NOCASE",
        &[],
    )
}

/// `localSongsWithoutGenre` (DatabaseDao.kt:1141-1142).
pub fn local_songs_without_genre(conn: &Connection) -> DbResult<Vec<SongEntity>> {
    let mut stmt = conn
        .prepare("SELECT * FROM song WHERE isLocal AND genre IS NULL AND localUri IS NOT NULL")?;
    let rows = stmt.query_map([], SongEntity::from_row)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// `delete(song: SongEntity)` (DatabaseDao.kt:2078).
pub fn delete_song(conn: &Connection, id: &str) -> DbResult<()> {
    conn.execute("DELETE FROM song WHERE id = ?1", [id])?;
    Ok(())
}

/// `String.looksLikeTimestampName()` (DatabaseDao.kt:2131 +
/// TIMESTAMP_NAME_REGEX at 2130): artist rows named like a duration
/// ("3:45", "12:03:22") are junk from bad metadata and never get linked.
fn looks_like_timestamp_name(name: &str) -> bool {
    let t = name.trim();
    let parts: Vec<&str> = t.split(':').collect();
    match parts.len() {
        2 | 3 => {
            let first = parts[0];
            let rest_ok = parts[1..]
                .iter()
                .all(|p| p.len() == 2 && p.chars().all(|c| c.is_ascii_digit()));
            let first_ok =
                (1..=2).contains(&first.len()) && first.chars().all(|c| c.is_ascii_digit());
            !t.is_empty() && first_ok && rest_ok
        }
        _ => false,
    }
}

/// `insert(mediaMetadata, block)` (DatabaseDao.kt:1599-1631) — song +
/// artists + positions, ignoring timestamp-looking artist names. The
/// desktop call shape takes the pieces directly (Phase 06's innertube
/// models produce them); `now_ms` stamps generated artists'
/// `lastUpdateTime`. Returns whether the song row was inserted.
pub fn insert_song_with_artists(
    conn: &Connection,
    song: &SongEntity,
    artists: &[(Option<String>, String)], // (id, name) — id None = generate "LA..." style
    now_ms: i64,
) -> DbResult<bool> {
    if !insert_song(conn, song)? {
        return Ok(false);
    }
    for (index, (artist_id, name)) in artists.iter().enumerate() {
        if looks_like_timestamp_name(name) {
            continue;
        }
        let resolved = match artist_id {
            Some(id) => id.clone(),
            None => match artist_by_name(conn, name)? {
                Some(existing) => existing.id,
                // ArtistEntity.generateArtistId() — "LA" + 8 alnum
                // (ArtistEntity.kt:51-53). RandomStringUtils.insecure()
                // uses [a-zA-Z0-9]; we keep the same shape.
                None => format!("LA{}", crate::util::random_artist_suffix()),
            },
        };
        conn.execute(
            "INSERT OR IGNORE INTO artist (id, name, thumbnailUrl, channelId, lastUpdateTime, bookmarkedAt, isLocal) \
             VALUES (?1, ?2, NULL, ?3, ?4, NULL, 0)",
            rusqlite::params![resolved, name, artist_id.as_deref(), now_ms],
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO song_artist_map (songId, artistId, position) VALUES (?1, ?2, ?3)",
            rusqlite::params![song.id, resolved, index as i32],
        )?;
    }
    Ok(true)
}

/// `songArtistMap(songId)` (DatabaseDao.kt:716-717).
pub fn song_artist_map(conn: &Connection, song_id: &str) -> DbResult<Vec<SongArtistMap>> {
    let mut stmt =
        conn.prepare("SELECT songId, artistId, position FROM song_artist_map WHERE songId = ?1")?;
    let rows = stmt.query_map([song_id], |row| {
        Ok(SongArtistMap {
            song_id: row.get(0)?,
            artist_id: row.get(1)?,
            position: row.get(2)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

// ---------------------------------------------------------------------------
// Artist queries (DatabaseDao.kt:764-866)
// ---------------------------------------------------------------------------

fn artist_from_row_with_count(row: &Row<'_>) -> rusqlite::Result<Artist> {
    Ok(Artist {
        artist: ArtistEntity::from_row(row)?,
        song_count: row.get("songCount")?,
        time_listened: None,
    })
}

/// `artist(id)` (DatabaseDao.kt:864-866) — with library song count.
pub fn artist(conn: &Connection, id: &str) -> DbResult<Option<Artist>> {
    conn.query_row(
        "SELECT *, (SELECT COUNT(1) FROM song_artist_map JOIN song ON song_artist_map.songId = song.id \
         WHERE artistId = artist.id AND song.inLibrary IS NOT NULL) AS songCount \
         FROM artist WHERE id = ?1",
        [id],
        artist_from_row_with_count,
    )
    .optional()
    .map_err(DbError::Sqlite)
}

/// `artistByName` (DatabaseDao.kt:1543-1545).
pub fn artist_by_name(conn: &Connection, name: &str) -> DbResult<Option<ArtistEntity>> {
    conn.query_row(
        "SELECT * FROM artist WHERE name = ?1",
        [name],
        ArtistEntity::from_row,
    )
    .optional()
    .map_err(DbError::Sqlite)
}

/// `getArtistById` (DatabaseDao.kt:1547-1548).
pub fn artist_entity(conn: &Connection, id: &str) -> DbResult<Option<ArtistEntity>> {
    conn.query_row(
        "SELECT * FROM artist WHERE id = ?1 LIMIT 1",
        [id],
        ArtistEntity::from_row,
    )
    .optional()
    .map_err(DbError::Sqlite)
}

/// `artists(sortType, descending)` (DatabaseDao.kt:840-850) — library
/// artists (songCount > 0), then the Kotlin-side filter
/// `isYouTubeArtist || isLocal` is applied (id starts with "UC" or
/// "FEmusic_library_privately_owned_artist", or isLocal —
/// ArtistEntity.kt:41-46).
pub fn artists(conn: &Connection, sort: ArtistSort, desc: bool) -> DbResult<Vec<Artist>> {
    let order = match (sort, desc) {
        (ArtistSort::CreateDate, false) => "rowId",
        (ArtistSort::CreateDate, true) => "rowId DESC",
        (ArtistSort::Name, false) => "name",
        (ArtistSort::Name, true) => "name DESC",
        (ArtistSort::SongCount, false) => "songCount",
        (ArtistSort::SongCount, true) => "songCount DESC",
        (ArtistSort::PlayTime, false) => "playTimeSum",
        (ArtistSort::PlayTime, true) => "playTimeSum DESC",
    };
    let sql = format!(
        "SELECT artist.*, \
         (SELECT COUNT(1) FROM song_artist_map JOIN song ON song_artist_map.songId = song.id \
          WHERE artistId = artist.id AND song.inLibrary IS NOT NULL) AS songCount, \
         COALESCE((SELECT SUM(song.totalPlayTime) FROM song_artist_map \
          JOIN song ON song_artist_map.songId = song.id \
          WHERE artistId = artist.id AND song.inLibrary IS NOT NULL), 0) AS playTimeSum \
         FROM artist WHERE songCount > 0 ORDER BY {order}"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |row| {
        Ok(Artist {
            time_listened: row.get::<_, Option<i64>>("playTimeSum")?,
            ..artist_from_row_with_count(row)?
        })
    })?;
    let mut out: Vec<Artist> = rows.collect::<Result<Vec<_>, _>>()?;
    out.retain(|a| a.artist.is_youtube_artist() || a.artist.is_local);
    Ok(out)
}

/// `artistsBookmarked(sortType, descending)` (DatabaseDao.kt:804-862).
pub fn artists_bookmarked(
    conn: &Connection,
    sort: ArtistSort,
    desc: bool,
) -> DbResult<Vec<Artist>> {
    let order = match (sort, desc) {
        (ArtistSort::CreateDate, false) => "bookmarkedAt",
        (ArtistSort::CreateDate, true) => "bookmarkedAt DESC",
        (ArtistSort::Name, false) => "name",
        (ArtistSort::Name, true) => "name DESC",
        (ArtistSort::SongCount, false) => "songCount",
        (ArtistSort::SongCount, true) => "songCount DESC",
        (ArtistSort::PlayTime, false) => "bookmarkedAt",
        (ArtistSort::PlayTime, true) => "bookmarkedAt DESC",
    };
    let sql = format!(
        "SELECT *, (SELECT COUNT(1) FROM song_artist_map JOIN song ON song_artist_map.songId = song.id \
         WHERE artistId = artist.id AND song.inLibrary IS NOT NULL) AS songCount \
         FROM artist WHERE bookmarkedAt IS NOT NULL ORDER BY {order}"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], artist_from_row_with_count)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// `insert(artist: ArtistEntity)` — IGNORE (DatabaseDao.kt:1553-1554).
pub fn insert_artist(conn: &Connection, artist: &ArtistEntity) -> DbResult<bool> {
    let n = conn.execute(
        "INSERT OR IGNORE INTO artist (id, name, thumbnailUrl, channelId, lastUpdateTime, bookmarkedAt, isLocal) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![
            artist.id,
            artist.name,
            artist.thumbnail_url,
            artist.channel_id,
            artist.last_update_time,
            artist.bookmarked_at,
            artist.is_local
        ],
    )?;
    Ok(n == 1)
}

/// `update(artist: ArtistEntity)` — @Update (DatabaseDao.kt:1959-1960).
pub fn update_artist(conn: &Connection, artist: &ArtistEntity) -> DbResult<()> {
    conn.execute(
        "UPDATE artist SET name = ?1, thumbnailUrl = ?2, channelId = ?3, lastUpdateTime = ?4, \
         bookmarkedAt = ?5, isLocal = ?6 WHERE id = ?7",
        rusqlite::params![
            artist.name,
            artist.thumbnail_url,
            artist.channel_id,
            artist.last_update_time,
            artist.bookmarked_at,
            artist.is_local,
            artist.id
        ],
    )?;
    Ok(())
}

/// `ArtistEntity.localToggleLike()` (ArtistEntity.kt:38-40) — bookmark
/// toggle, row-level only. The `YouTube.subscribeChannel` call is the Phase
/// 14 hook (ArtistEntity.kt:42-50).
pub fn toggle_bookmark_artist(
    conn: &Connection,
    id: &str,
    now_ms: i64,
) -> DbResult<Option<ArtistEntity>> {
    let Some(mut artist) = artist_entity(conn, id)? else {
        return Ok(None);
    };
    artist.bookmarked_at = if artist.bookmarked_at.is_some() {
        None
    } else {
        Some(now_ms)
    };
    update_artist(conn, &artist)?;
    Ok(Some(artist))
}

/// `delete(artist: ArtistEntity)` (DatabaseDao.kt:2084).
pub fn delete_artist(conn: &Connection, id: &str) -> DbResult<()> {
    conn.execute("DELETE FROM artist WHERE id = ?1", [id])?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Album queries (DatabaseDao.kt:868-1100)
// ---------------------------------------------------------------------------

fn album_with_artists(conn: &Connection, album: AlbumEntity) -> DbResult<Album> {
    let mut stmt = conn.prepare(
        "SELECT artist.* FROM album_artist_map \
         JOIN artist ON artist.id = album_artist_map.artistId \
         WHERE album_artist_map.albumId = ?1",
    )?;
    let rows = stmt.query_map([&album.id], ArtistEntity::from_row)?;
    Ok(Album {
        artists: rows.collect::<Result<Vec<_>, _>>()?,
        song_count_listened: None,
        time_listened: None,
        album,
    })
}

/// `album(id)` — entity + artists. The upstream `Album` relation joins
/// `Junction(AlbumArtistMap::class)` (entities/Album.kt:10-18); Room emits
/// no ORDER BY (not one of the view-sorted junctions), so neither do we.
pub fn album(conn: &Connection, id: &str) -> DbResult<Option<Album>> {
    let found: Option<AlbumEntity> = conn
        .query_row(
            "SELECT * FROM album WHERE id = ?1",
            [id],
            AlbumEntity::from_row,
        )
        .optional()?;
    found.map_or(Ok(None), |a| Ok(Some(album_with_artists(conn, a)?)))
}

/// `albums(sortType, descending)` (DatabaseDao.kt:870-988 + 988-1010):
/// albums with at least one library song.
pub fn albums(conn: &Connection, sort: AlbumSort, desc: bool) -> DbResult<Vec<Album>> {
    let order = match (sort, desc) {
        (AlbumSort::CreateDate, false) => "rowId",
        (AlbumSort::CreateDate, true) => "rowId DESC",
        (AlbumSort::Name, false) => "title",
        (AlbumSort::Name, true) => "title DESC",
        (AlbumSort::Year, false) => "year",
        (AlbumSort::Year, true) => "year DESC",
        (AlbumSort::SongCount, false) => "songCount",
        (AlbumSort::SongCount, true) => "songCount DESC",
        (AlbumSort::Length, false) => "duration",
        (AlbumSort::Length, true) => "duration DESC",
        (AlbumSort::PlayTime, false) => "playTimeSum",
        (AlbumSort::PlayTime, true) => "playTimeSum DESC",
        (AlbumSort::Artist, false) => "rowId",
        (AlbumSort::Artist, true) => "rowId DESC",
    };
    let sql = format!(
        "SELECT album.*, COALESCE((SELECT SUM(song.totalPlayTime) FROM song \
          WHERE song.albumId = album.id AND song.inLibrary IS NOT NULL), 0) AS playTimeSum \
         FROM album WHERE EXISTS(SELECT * FROM song WHERE song.albumId = album.id AND song.inLibrary IS NOT NULL) \
         ORDER BY {order}"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], AlbumEntity::from_row)?;
    let albums: Vec<AlbumEntity> = rows.collect::<Result<Vec<_>, _>>()?;
    albums
        .into_iter()
        .map(|a| album_with_artists(conn, a))
        .collect()
}

/// `insert(album: AlbumEntity)` — IGNORE (DatabaseDao.kt:1556-1557).
pub fn insert_album(conn: &Connection, album: &AlbumEntity) -> DbResult<bool> {
    let n = conn.execute(
        "INSERT OR IGNORE INTO album (id, playlistId, title, year, thumbnailUrl, themeColor, songCount, duration, \
         explicit, lastUpdateTime, bookmarkedAt, likedDate, inLibrary, description, isLocal, isUploaded) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
        rusqlite::params![
            album.id,
            album.playlist_id,
            album.title,
            album.year,
            album.thumbnail_url,
            album.theme_color,
            album.song_count,
            album.duration,
            album.explicit,
            album.last_update_time,
            album.bookmarked_at,
            album.liked_date,
            album.in_library,
            album.description,
            album.is_local,
            album.is_uploaded
        ],
    )?;
    Ok(n == 1)
}

/// `update(album: AlbumEntity)` — @Update (DatabaseDao.kt:1962-1963).
pub fn update_album(conn: &Connection, album: &AlbumEntity) -> DbResult<()> {
    conn.execute(
        "UPDATE album SET playlistId = ?1, title = ?2, year = ?3, thumbnailUrl = ?4, themeColor = ?5, \
         songCount = ?6, duration = ?7, explicit = ?8, lastUpdateTime = ?9, bookmarkedAt = ?10, \
         likedDate = ?11, inLibrary = ?12, description = ?13, isLocal = ?14, isUploaded = ?15 WHERE id = ?16",
        rusqlite::params![
            album.playlist_id,
            album.title,
            album.year,
            album.thumbnail_url,
            album.theme_color,
            album.song_count,
            album.duration,
            album.explicit,
            album.last_update_time,
            album.bookmarked_at,
            album.liked_date,
            album.in_library,
            album.description,
            album.is_local,
            album.is_uploaded,
            album.id
        ],
    )?;
    Ok(())
}

/// `AlbumEntity.localToggleLike()` (AlbumEntity.kt:30-32) — bookmark toggle.
/// `YouTube.likePlaylist` is the Phase 14 hook.
pub fn toggle_bookmark_album(
    conn: &Connection,
    id: &str,
    now_ms: i64,
) -> DbResult<Option<AlbumEntity>> {
    let Some(mut album): Option<AlbumEntity> = conn
        .query_row(
            "SELECT * FROM album WHERE id = ?1",
            [id],
            AlbumEntity::from_row,
        )
        .optional()?
    else {
        return Ok(None);
    };
    album.bookmarked_at = if album.bookmarked_at.is_some() {
        None
    } else {
        Some(now_ms)
    };
    update_album(conn, &album)?;
    Ok(Some(album))
}

/// `AlbumEntity.toggleLibrary()` (AlbumEntity.kt:34-36).
pub fn toggle_album_library(
    conn: &Connection,
    id: &str,
    now_ms: i64,
) -> DbResult<Option<AlbumEntity>> {
    let Some(mut album): Option<AlbumEntity> = conn
        .query_row(
            "SELECT * FROM album WHERE id = ?1",
            [id],
            AlbumEntity::from_row,
        )
        .optional()?
    else {
        return Ok(None);
    };
    album.in_library = if album.in_library.is_some() {
        None
    } else {
        Some(now_ms)
    };
    update_album(conn, &album)?;
    Ok(Some(album))
}

/// `refreshLocalAlbum` (DatabaseDao.kt:1132-1140) — re-count a local
/// album's songs/length and take the first cover + year any file has.
pub fn refresh_local_album(
    conn: &Connection,
    album_id: &str,
    thumbnail_url: Option<&str>,
    year: Option<i32>,
) -> DbResult<()> {
    conn.execute(
        "UPDATE album SET \
         songCount = (SELECT COUNT(*) FROM song_album_map WHERE albumId = ?1), \
         duration = (SELECT COALESCE(SUM(song.duration), 0) FROM song \
         JOIN song_album_map ON song_album_map.songId = song.id WHERE song_album_map.albumId = ?1), \
         thumbnailUrl = COALESCE(thumbnailUrl, ?2), year = COALESCE(year, ?3) \
         WHERE id = ?1",
        rusqlite::params![album_id, thumbnail_url, year],
    )?;
    Ok(())
}

/// `setSongAlbum` (DatabaseDao.kt:1127-1128).
pub fn set_song_album(conn: &Connection, song_id: &str, album_id: &str) -> DbResult<()> {
    conn.execute(
        "UPDATE song SET albumId = ?1 WHERE id = ?2",
        rusqlite::params![album_id, song_id],
    )?;
    Ok(())
}

/// `delete(album: AlbumEntity)` (DatabaseDao.kt:2087).
pub fn delete_album(conn: &Connection, id: &str) -> DbResult<()> {
    conn.execute("DELETE FROM album WHERE id = ?1", [id])?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Playlist queries (DatabaseDao.kt:1153-1267, 1494-1540)
// ---------------------------------------------------------------------------

/// The `playlist_song_map_preview` CONSUMING QUERY: upstream's `Playlist`
/// relation reads its first thumbnails through
/// `Junction(PlaylistSongMapPreview::class)` (entities/Playlist.kt:14-27);
/// the view supplies `WHERE position <= 3 ORDER BY position`
/// (entities/PlaylistSongMapPreview.kt:6-9). Inlined here because SQLite
/// does not guarantee view ordering.
pub fn playlist_preview_thumbnails(
    conn: &Connection,
    playlist_id: &str,
) -> DbResult<Vec<Option<String>>> {
    let mut stmt = conn.prepare(
        "SELECT song.thumbnailUrl FROM playlist_song_map \
         JOIN song ON song.id = playlist_song_map.songId \
         WHERE playlist_song_map.playlistId = ?1 AND playlist_song_map.position <= 3 \
         ORDER BY playlist_song_map.position",
    )?;
    let rows = stmt.query_map([playlist_id], |row| row.get::<_, Option<String>>(0))?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Maps a `playlist.*  + songCount` row to the entity pair; the preview
/// thumbnails are filled by the caller (a nested query cannot run inside
/// `query_map`'s borrow).
fn playlist_parts_from_row(row: &Row<'_>) -> rusqlite::Result<(PlaylistEntity, i32)> {
    Ok((PlaylistEntity::from_row(row)?, row.get("songCount")?))
}

fn playlist_with_thumbnails(
    conn: &Connection,
    (playlist, song_count): (PlaylistEntity, i32),
) -> DbResult<Playlist> {
    Ok(Playlist {
        song_thumbnails: playlist_preview_thumbnails(conn, &playlist.id)?,
        song_count,
        playlist,
    })
}

/// `playlist(playlistId)` (DatabaseDao.kt:1182-1183) — entity + songCount +
/// preview thumbnails.
pub fn playlist(conn: &Connection, id: &str) -> DbResult<Option<Playlist>> {
    let parts = conn
        .query_row(
            "SELECT *, (SELECT COUNT(*) FROM playlist_song_map WHERE playlistId = playlist.id) AS songCount \
             FROM playlist WHERE id = ?1",
            [id],
            playlist_parts_from_row,
        )
        .optional()?;
    parts.map_or(Ok(None), |p| Ok(Some(playlist_with_thumbnails(conn, p)?)))
}

/// `playlistEntity` (DatabaseDao.kt:1185-1186).
pub fn playlist_entity(conn: &Connection, id: &str) -> DbResult<Option<PlaylistEntity>> {
    conn.query_row(
        "SELECT * FROM playlist WHERE id = ?1",
        [id],
        PlaylistEntity::from_row,
    )
    .optional()
    .map_err(DbError::Sqlite)
}

/// `playlists(sortType, descending)` (DatabaseDao.kt:1153-1179) — pinned
/// playlists (`bookmarkedAt IS NOT NULL`).
pub fn playlists(conn: &Connection, sort: PlaylistSort, desc: bool) -> DbResult<Vec<Playlist>> {
    let order = match (sort, desc) {
        (PlaylistSort::CreateDate, false) => "rowId",
        (PlaylistSort::CreateDate, true) => "rowId DESC",
        (PlaylistSort::Name, false) => "name",
        (PlaylistSort::Name, true) => "name DESC",
        (PlaylistSort::SongCount, false) => "songCount",
        (PlaylistSort::SongCount, true) => "songCount DESC",
        (PlaylistSort::LastUpdated, false) => "lastUpdateTime",
        (PlaylistSort::LastUpdated, true) => "lastUpdateTime DESC",
    };
    let sql = format!(
        "SELECT *, (SELECT COUNT(*) FROM playlist_song_map WHERE playlistId = playlist.id) AS songCount \
         FROM playlist WHERE bookmarkedAt IS NOT NULL ORDER BY {order}"
    );
    let mut stmt = conn.prepare(&sql)?;
    let parts: Vec<(PlaylistEntity, i32)> = {
        let rows = stmt.query_map([], playlist_parts_from_row)?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    parts
        .into_iter()
        .map(|p| playlist_with_thumbnails(conn, p))
        .collect()
}

/// `editablePlaylistsByNameAsc` (DatabaseDao.kt:1177-1179).
pub fn editable_playlists(conn: &Connection) -> DbResult<Vec<Playlist>> {
    let mut stmt = conn.prepare(
        "SELECT *, (SELECT COUNT(*) FROM playlist_song_map WHERE playlistId = playlist.id) AS songCount \
         FROM playlist WHERE isEditable AND bookmarkedAt IS NOT NULL ORDER BY name",
    )?;
    let parts: Vec<(PlaylistEntity, i32)> = {
        let rows = stmt.query_map([], playlist_parts_from_row)?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    parts
        .into_iter()
        .map(|p| playlist_with_thumbnails(conn, p))
        .collect()
}

/// `playlistSongs(playlistId)` (DatabaseDao.kt:234-236) — ORDER BY position
/// written into the consuming query (matches the map's natural order).
pub fn playlist_songs(conn: &Connection, playlist_id: &str) -> DbResult<Vec<PlaylistSong>> {
    let mut stmt =
        conn.prepare("SELECT * FROM playlist_song_map WHERE playlistId = ?1 ORDER BY position")?;
    let maps: Vec<PlaylistSongMap> = {
        let rows = stmt.query_map([playlist_id], |row| {
            Ok(PlaylistSongMap {
                id: row.get("id")?,
                playlist_id: row.get("playlistId")?,
                song_id: row.get("songId")?,
                position: row.get("position")?,
                set_video_id: row.get("setVideoId")?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    maps.into_iter()
        .map(|map| match song(conn, &map.song_id)? {
            Some(song) => Ok(PlaylistSong { map, song }),
            // Room's @Relation silently drops orphans; so does the port.
            None => Err(DbError::Conversion(format!(
                "playlist_song_map row {} references missing song {}",
                map.id, map.song_id
            ))),
        })
        .collect()
}

/// `playlistSongIds` (DatabaseDao.kt:1244-1245).
pub fn playlist_song_ids(conn: &Connection, playlist_id: &str) -> DbResult<Vec<String>> {
    let mut stmt = conn.prepare("SELECT songId FROM playlist_song_map WHERE playlistId = ?1")?;
    let rows = stmt.query_map([playlist_id], |r| r.get::<_, String>(0))?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// `checkInPlaylist` (DatabaseDao.kt:1226-1231).
pub fn song_in_playlist(conn: &Connection, playlist_id: &str, song_id: &str) -> DbResult<bool> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) from playlist_song_map WHERE playlistId = ?1 AND songId = ?2 LIMIT 1",
        rusqlite::params![playlist_id, song_id],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

/// `lastPlaylistEntryId` (DatabaseDao.kt:1188-1190).
pub fn last_playlist_entry_id(conn: &Connection, playlist_id: &str) -> DbResult<Option<i64>> {
    conn.query_row(
        "SELECT MAX(id) FROM playlist_song_map WHERE playlistId = ?1",
        [playlist_id],
        |r| r.get(0),
    )
    .optional()
    .map_err(DbError::Sqlite)
}

/// `insert(playlist: PlaylistEntity)` — IGNORE (DatabaseDao.kt:1558-1559).
pub fn insert_playlist(conn: &Connection, playlist: &PlaylistEntity) -> DbResult<bool> {
    let n = conn.execute(
        "INSERT OR IGNORE INTO playlist (id, name, browseId, createdAt, lastUpdateTime, isEditable, bookmarkedAt, \
         remoteSongCount, playEndpointParams, thumbnailUrl, shuffleEndpointParams, radioEndpointParams, isLocal, \
         isAutoSync, description) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
        rusqlite::params![
            playlist.id,
            playlist.name,
            playlist.browse_id,
            playlist.created_at,
            playlist.last_update_time,
            playlist.is_editable,
            playlist.bookmarked_at,
            playlist.remote_song_count,
            playlist.play_endpoint_params,
            playlist.thumbnail_url,
            playlist.shuffle_endpoint_params,
            playlist.radio_endpoint_params,
            playlist.is_local,
            playlist.is_auto_sync,
            playlist.description
        ],
    )?;
    Ok(n == 1)
}

/// `update(playlist: PlaylistEntity)` — @Update (DatabaseDao.kt:1965-1966).
pub fn update_playlist(conn: &Connection, playlist: &PlaylistEntity) -> DbResult<()> {
    conn.execute(
        "UPDATE playlist SET name = ?1, browseId = ?2, createdAt = ?3, lastUpdateTime = ?4, isEditable = ?5, \
         bookmarkedAt = ?6, remoteSongCount = ?7, playEndpointParams = ?8, thumbnailUrl = ?9, \
         shuffleEndpointParams = ?10, radioEndpointParams = ?11, isLocal = ?12, isAutoSync = ?13, \
         description = ?14 WHERE id = ?15",
        rusqlite::params![
            playlist.name,
            playlist.browse_id,
            playlist.created_at,
            playlist.last_update_time,
            playlist.is_editable,
            playlist.bookmarked_at,
            playlist.remote_song_count,
            playlist.play_endpoint_params,
            playlist.thumbnail_url,
            playlist.shuffle_endpoint_params,
            playlist.radio_endpoint_params,
            playlist.is_local,
            playlist.is_auto_sync,
            playlist.description,
            playlist.id
        ],
    )?;
    Ok(())
}

/// `PlaylistEntity.localToggleLike()` (PlaylistEntity.kt:48-50) — pin
/// toggle; the `YouTube.likePlaylist` call is the Phase 14 hook.
pub fn toggle_pin_playlist(
    conn: &Connection,
    id: &str,
    now_ms: i64,
) -> DbResult<Option<PlaylistEntity>> {
    let Some(mut playlist) = playlist_entity(conn, id)? else {
        return Ok(None);
    };
    playlist.bookmarked_at = if playlist.bookmarked_at.is_some() {
        None
    } else {
        Some(now_ms)
    };
    update_playlist(conn, &playlist)?;
    Ok(Some(playlist))
}

/// `addSongToPlaylist(playlist, songIds)` (DatabaseDao.kt:1261-1270):
/// append at `songCount`, `songCount+1`, …
pub fn add_songs_to_playlist(
    conn: &Connection,
    playlist_id: &str,
    song_ids: &[String],
) -> DbResult<()> {
    let base: i32 = conn
        .query_row(
            "SELECT COUNT(*) FROM playlist_song_map WHERE playlistId = ?1",
            [playlist_id],
            |r| r.get::<_, i32>(0),
        )
        .unwrap_or(0);
    for (offset, id) in song_ids.iter().enumerate() {
        conn.execute(
            "INSERT OR IGNORE INTO playlist_song_map (playlistId, songId, position, setVideoId) \
             VALUES (?1, ?2, ?3, NULL)",
            rusqlite::params![playlist_id, id, base + offset as i32],
        )?;
    }
    conn.execute(
        "UPDATE playlist SET lastUpdateTime = ?1 WHERE id = ?2",
        rusqlite::params![crate::util::now_ms(), playlist_id],
    )?;
    Ok(())
}

/// `addSongsToPlaylistTop` (DatabaseDao.kt:1253-1259): shift existing rows
/// up by the batch size, then insert at positions 0..n (mirrors YouTube).
pub fn add_songs_to_playlist_top(
    conn: &Connection,
    playlist_id: &str,
    song_ids: &[String],
) -> DbResult<()> {
    if song_ids.is_empty() {
        return Ok(());
    }
    conn.execute(
        "UPDATE playlist_song_map SET position = position + ?1 WHERE playlistId = ?2",
        rusqlite::params![song_ids.len() as i32, playlist_id],
    )?;
    for (index, id) in song_ids.iter().enumerate() {
        conn.execute(
            "INSERT OR IGNORE INTO playlist_song_map (playlistId, songId, position, setVideoId) \
             VALUES (?1, ?2, ?3, NULL)",
            rusqlite::params![playlist_id, id, index as i32],
        )?;
    }
    Ok(())
}

/// `move(playlistId, fromPosition, toPosition)` (DatabaseDao.kt:1494-1510).
pub fn move_playlist_song(
    conn: &Connection,
    playlist_id: &str,
    from_position: i32,
    to_position: i32,
) -> DbResult<()> {
    conn.execute(
        "UPDATE playlist_song_map SET position = \
            CASE \
                WHEN position < ?1 THEN position + 1 \
                WHEN position > ?1 THEN position - 1 \
                ELSE ?2 \
            END \
         WHERE playlistId = ?3 AND position BETWEEN MIN(?1, ?2) AND MAX(?1, ?2)",
        rusqlite::params![from_position, to_position, playlist_id],
    )?;
    Ok(())
}

/// `removeSongsFromPlaylist`-shaped range delete
/// (`removeAddedSongs`, DatabaseDao.kt:1239-1241).
pub fn remove_playlist_songs_from_position(
    conn: &Connection,
    playlist_id: &str,
    song_ids: &[String],
    from_position: i32,
) -> DbResult<()> {
    for id in song_ids {
        conn.execute(
            "DELETE FROM playlist_song_map WHERE playlistId = ?1 AND songId = ?2 AND position >= ?3",
            rusqlite::params![playlist_id, id, from_position],
        )?;
    }
    Ok(())
}

/// `clearPlaylist` (DatabaseDao.kt:1512-1513).
pub fn clear_playlist(conn: &Connection, playlist_id: &str) -> DbResult<()> {
    conn.execute(
        "DELETE FROM playlist_song_map WHERE playlistId = ?1",
        [playlist_id],
    )?;
    Ok(())
}

/// `deletePlaylistById` (DatabaseDao.kt:2099-2100) — by browseId upstream;
/// the desktop takes the local id.
pub fn delete_playlist(conn: &Connection, id: &str) -> DbResult<()> {
    conn.execute("DELETE FROM playlist WHERE id = ?1", [id])?;
    Ok(())
}

/// `replacePlaylistCover` (DatabaseDao.kt:1215-1217): swap the cover only
/// while it still equals `expected` (a late redraw never undoes a newer
/// edit).
pub fn replace_playlist_cover(
    conn: &Connection,
    playlist_id: &str,
    expected: Option<&str>,
    thumbnail_url: &str,
) -> DbResult<bool> {
    let n = conn.execute(
        "UPDATE playlist SET thumbnailUrl = ?1 WHERE id = ?2 AND thumbnailUrl IS ?3",
        rusqlite::params![thumbnail_url, playlist_id, expected],
    )?;
    Ok(n == 1)
}

// ---------------------------------------------------------------------------
// Format queries (DatabaseDao.kt:1944-1946)
// ---------------------------------------------------------------------------

/// `format(id)` (DatabaseDao.kt:760-762 shape).
pub fn format(conn: &Connection, id: &str) -> DbResult<Option<FormatEntity>> {
    song_format(conn, id)
}

/// `upsert(format)` — @Upsert semantics (REPLACE by PK).
pub fn upsert_format(conn: &Connection, f: &FormatEntity) -> DbResult<()> {
    conn.execute(
        "INSERT OR REPLACE INTO format (id, itag, mimeType, codecs, bitrate, sampleRate, contentLength, \
         loudnessDb, perceptualLoudnessDb, playbackUrl) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        rusqlite::params![
            f.id,
            f.itag,
            f.mime_type,
            f.codecs,
            f.bitrate,
            f.sample_rate,
            f.content_length,
            f.loudness_db,
            f.perceptual_loudness_db,
            f.playback_url
        ],
    )?;
    Ok(())
}

/// `deleteFormat(songId)` (DatabaseDao.kt:1944-1945).
pub fn delete_format(conn: &Connection, id: &str) -> DbResult<()> {
    conn.execute("DELETE FROM format WHERE id = ?1", [id])?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Lyrics queries (DatabaseDao.kt:757-762, 1947-1948)
// ---------------------------------------------------------------------------

/// `lyrics(id)` (DatabaseDao.kt:760-762).
pub fn lyrics(conn: &Connection, id: &str) -> DbResult<Option<LyricsEntity>> {
    conn.query_row(
        "SELECT * FROM lyrics WHERE id = ?1",
        [id],
        LyricsEntity::from_row,
    )
    .optional()
    .map_err(DbError::Sqlite)
}

/// `upsert(lyrics)` — @Upsert; saves fetched lyrics AND the
/// LYRICS_NOT_FOUND sentinel (a negative cache entry).
pub fn upsert_lyrics(conn: &Connection, l: &LyricsEntity) -> DbResult<()> {
    conn.execute(
        "INSERT OR REPLACE INTO lyrics (id, lyrics, provider, translatedLyrics, translationLanguage, translationMode) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![
            l.id,
            l.lyrics,
            l.provider,
            l.translated_lyrics,
            l.translation_language,
            l.translation_mode
        ],
    )?;
    Ok(())
}

/// `delete(lyrics)` (DatabaseDao.kt:2102-2103).
pub fn delete_lyrics(conn: &Connection, id: &str) -> DbResult<()> {
    conn.execute("DELETE FROM lyrics WHERE id = ?1", [id])?;
    Ok(())
}

/// `clearFetchedLyrics(keepProviders)` (DatabaseDao.kt:1588-1590): every
/// fetched lyric (and every saved "not found") goes, so they are looked up
/// again; lyrics from `keep_providers` stay.
pub fn clear_fetched_lyrics(conn: &Connection, keep_providers: &[String]) -> DbResult<usize> {
    let list = keep_providers
        .iter()
        .map(|p| format!("'{}'", p.replace('\'', "''")))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = if keep_providers.is_empty() {
        "DELETE FROM lyrics".to_string()
    } else {
        format!("DELETE FROM lyrics WHERE provider NOT IN ({list})")
    };
    Ok(conn.execute(&sql, [])?)
}

// ---------------------------------------------------------------------------
// Event + playCount queries (DatabaseDao.kt:291-319, 1440-1476)
// ---------------------------------------------------------------------------

/// `insert(event: Event)` — IGNORE (DatabaseDao.kt:1577-1578). Called when
/// a play crossed the history threshold (MusicService.kt:4370-4379:
/// `Event(songId, timestamp = LocalDateTime.now(), playTime)`).
pub fn insert_event(conn: &Connection, event: &Event) -> DbResult<()> {
    conn.execute(
        "INSERT OR IGNORE INTO event (songId, timestamp, playTime) VALUES (?1, ?2, ?3)",
        rusqlite::params![event.song_id, event.timestamp, event.play_time],
    )?;
    Ok(())
}

/// `insert(event: PlaylistEvent)` (DatabaseDao.kt:1580-1581).
pub fn insert_playlist_event(conn: &Connection, event: &PlaylistEvent) -> DbResult<()> {
    conn.execute(
        "INSERT OR IGNORE INTO playlist_event (playlistId, timestamp) VALUES (?1, ?2)",
        rusqlite::params![event.playlist_id, event.timestamp],
    )?;
    Ok(())
}

/// `totalListeningTimeMs()` (DatabaseDao.kt:292-295).
pub fn total_listening_time_ms(conn: &Connection) -> DbResult<i64> {
    Ok(
        conn.query_row("SELECT COALESCE(SUM(playTime), 0) FROM event", [], |r| {
            r.get(0)
        })?,
    )
}

/// Events in a UTC-ms range (the shape behind mostPlayed*/recap queries,
/// e.g. DatabaseDao.kt:594-618).
pub fn events_in_range(
    conn: &Connection,
    from_ms: i64,
    to_ms: i64,
) -> DbResult<Vec<EventWithSong>> {
    let mut stmt = conn.prepare(
        "SELECT * FROM event WHERE timestamp >= ?1 AND timestamp <= ?2 ORDER BY timestamp",
    )?;
    let events: Vec<Event> = {
        let rows = stmt.query_map(rusqlite::params![from_ms, to_ms], |row| {
            Ok(Event {
                id: row.get("id")?,
                song_id: row.get("songId")?,
                timestamp: row.get("timestamp")?,
                play_time: row.get("playTime")?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    events
        .into_iter()
        .map(|event| match song(conn, &event.song_id)? {
            Some(song) => Ok(EventWithSong { event, song }),
            None => Ok(EventWithSong {
                event,
                song: Song::default(),
            }),
        })
        .collect()
}

/// `incrementPlayCount(songId)` (DatabaseDao.kt:1466-1476): get-or-insert
/// the (song, year, month) row then bump it. `now_ms` is supplied by the
/// caller (the service clock) instead of `LocalDateTime.now()`.
pub fn increment_play_count(conn: &Connection, song_id: &str, now_ms: i64) -> DbResult<()> {
    // Year/month in UTC — same as LocalDateTime.now().atOffset(UTC) did for
    // the desktop's true-UTC convention.
    let (year, month) = crate::util::year_month_utc(now_ms);
    let existing: Option<i32> = conn
        .query_row(
            "SELECT count from playCount WHERE song = ?1 AND year = ?2 AND month = ?3",
            rusqlite::params![song_id, year, month],
            |r| r.get(0),
        )
        .optional()?;
    if existing.is_none() {
        conn.execute(
            "INSERT OR IGNORE INTO playCount (song, year, month, count) VALUES (?1, ?2, ?3, 0)",
            rusqlite::params![song_id, year, month],
        )?;
    }
    conn.execute(
        "UPDATE playCount SET count = count + 1 WHERE song = ?1 AND year = ?2 AND month = ?3",
        rusqlite::params![song_id, year, month],
    )?;
    Ok(())
}

/// `getLifetimePlayCount` (DatabaseDao.kt:635-636).
pub fn lifetime_play_count(conn: &Connection, song_id: &str) -> DbResult<i64> {
    Ok(conn.query_row(
        "SELECT COALESCE(sum(count), 0) from playCount WHERE song = ?1",
        [song_id],
        |r| r.get(0),
    )?)
}

// ---------------------------------------------------------------------------
// Search history (DatabaseDao.kt:1519-1536)
// ---------------------------------------------------------------------------

/// `searchHistory(query)`-shaped prefix search
/// (`SELECT * FROM search_history WHERE query LIKE '%' || :query || '%' ORDER BY id DESC`).
pub fn search_history(conn: &Connection, query: &str) -> DbResult<Vec<SearchHistory>> {
    let mut stmt = conn.prepare(
        "SELECT id, query FROM search_history WHERE query LIKE '%' || ?1 || '%' ORDER BY id DESC",
    )?;
    let rows = stmt.query_map([query], |row| {
        Ok(SearchHistory {
            id: row.get(0)?,
            query: row.get(1)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// `insert(searchHistory)` — REPLACE on the unique query
/// (DatabaseDao.kt:1568-1569): re-searching a term moves it to the top.
pub fn upsert_search_history(conn: &Connection, query: &str) -> DbResult<()> {
    conn.execute(
        "INSERT OR REPLACE INTO search_history (query) VALUES (?1)",
        [query],
    )?;
    Ok(())
}

/// `delete(searchHistory)` (DatabaseDao.kt:2105-2106) — by query here.
pub fn delete_search_history(conn: &Connection, query: &str) -> DbResult<()> {
    conn.execute("DELETE FROM search_history WHERE query = ?1", [query])?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Radio caches: related_song_map + set_video_id
// ---------------------------------------------------------------------------

/// `hasRelatedSongs` (DatabaseDao.kt:1497-1498).
pub fn has_related_songs(conn: &Connection, song_id: &str) -> DbResult<bool> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(1) FROM related_song_map WHERE songId = ?1 LIMIT 1",
        [song_id],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

/// `relatedSongs` (DatabaseDao.kt:1512-1524): adjacency deduped by
/// relatedSongId, joined back to full songs.
pub fn related_songs(conn: &Connection, song_id: &str) -> DbResult<Vec<Song>> {
    let mut stmt = conn.prepare(
        "SELECT song.* FROM (SELECT * FROM related_song_map GROUP BY relatedSongId) map \
         JOIN song ON song.id = map.relatedSongId WHERE songId = ?1",
    )?;
    let songs: Vec<SongEntity> = {
        let rows = stmt.query_map([song_id], SongEntity::from_row)?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    songs
        .into_iter()
        .map(|s| song_with_relations(conn, s))
        .collect()
}

/// `insert(map: RelatedSongMap)` — IGNORE (DatabaseDao.kt:1583-1584).
pub fn insert_related_song(
    conn: &Connection,
    song_id: &str,
    related_song_id: &str,
) -> DbResult<()> {
    conn.execute(
        "INSERT OR IGNORE INTO related_song_map (songId, relatedSongId) VALUES (?1, ?2)",
        rusqlite::params![song_id, related_song_id],
    )?;
    Ok(())
}

/// `getSetVideoId(songId)`-shaped read of the queue-id cache.
pub fn set_video_id(conn: &Connection, video_id: &str) -> DbResult<Option<SetVideoIdEntity>> {
    conn.query_row(
        "SELECT videoId, setVideoId FROM set_video_id WHERE videoId = ?1",
        [video_id],
        |row| {
            Ok(SetVideoIdEntity {
                video_id: row.get(0)?,
                set_video_id: row.get(1)?,
            })
        },
    )
    .optional()
    .map_err(DbError::Sqlite)
}

/// `upsert(setVideoIdEntity)`.
pub fn upsert_set_video_id(conn: &Connection, entity: &SetVideoIdEntity) -> DbResult<()> {
    conn.execute(
        "INSERT OR REPLACE INTO set_video_id (videoId, setVideoId) VALUES (?1, ?2)",
        rusqlite::params![entity.video_id, entity.set_video_id],
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Recognition history (DatabaseDao.kt:1440-1463)
// ---------------------------------------------------------------------------

/// `searchRecognitionHistory(query)` (DatabaseDao.kt:1440-1442).
pub fn search_recognition_history(
    conn: &Connection,
    query: &str,
) -> DbResult<Vec<RecognitionHistory>> {
    let mut stmt = conn.prepare(
        "SELECT * FROM recognition_history WHERE title LIKE '%' || ?1 || '%' OR artist LIKE '%' || ?1 || '%' \
         ORDER BY recognizedAt DESC",
    )?;
    let rows = stmt.query_map([query], RecognitionHistory::from_row)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// `insert(recognitionHistory)` — REPLACE (DatabaseDao.kt:1456-1457).
pub fn upsert_recognition_history(conn: &Connection, r: &RecognitionHistory) -> DbResult<()> {
    conn.execute(
        "INSERT OR REPLACE INTO recognition_history (trackId, title, artist, album, coverArtUrl, coverArtHqUrl, \
         genre, releaseDate, label, shazamUrl, appleMusicUrl, spotifyUrl, isrc, youtubeVideoId, recognizedAt, liked) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
        rusqlite::params![
            r.track_id,
            r.title,
            r.artist,
            r.album,
            r.cover_art_url,
            r.cover_art_hq_url,
            r.genre,
            r.release_date,
            r.label,
            r.shazam_url,
            r.apple_music_url,
            r.spotify_url,
            r.isrc,
            r.youtube_video_id,
            r.recognized_at,
            r.liked
        ],
    )?;
    Ok(())
}

/// `updateRecognitionHistoryLiked` (DatabaseDao.kt:1447-1448).
pub fn set_recognition_liked(conn: &Connection, id: i64, liked: bool) -> DbResult<()> {
    conn.execute(
        "UPDATE recognition_history SET liked = ?1 WHERE id = ?2",
        rusqlite::params![liked, id],
    )?;
    Ok(())
}

/// `clearRecognitionHistory` (DatabaseDao.kt:1444-1445).
pub fn clear_recognition_history(conn: &Connection) -> DbResult<()> {
    conn.execute("DELETE FROM recognition_history", [])?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Speed dial (daos/SpeedDialDao.kt)
// ---------------------------------------------------------------------------

/// `SpeedDialDao.MaxPinned` — the 3x3 grid cap (SpeedDialDao.kt:33-38),
/// enforced centrally in `MusicDatabase.pinToSpeedDial`
/// (MusicDatabase.kt:63-74).
pub const MAX_SPEED_DIAL_ITEMS: usize = 9;

/// `SpeedDialDao.getAll()` — ORDER BY createDate DESC.
pub fn speed_dial_items(conn: &Connection) -> DbResult<Vec<SpeedDialItem>> {
    let mut stmt = conn.prepare(
        "SELECT id, secondaryId, title, subtitle, thumbnailUrl, type, explicit, createDate \
                  FROM speed_dial_item ORDER BY createDate DESC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(SpeedDialItem {
            id: row.get(0)?,
            secondary_id: row.get(1)?,
            title: row.get(2)?,
            subtitle: row.get(3)?,
            thumbnail_url: row.get(4)?,
            item_type: row.get(5)?,
            explicit: row.get(6)?,
            create_date: row.get(7)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// `MusicDatabase.pinToSpeedDial(item)` (MusicDatabase.kt:63-74): evict the
/// oldest pin first when the cap would be exceeded, then insert (REPLACE).
pub fn pin_to_speed_dial(conn: &Connection, item: &SpeedDialItem) -> DbResult<()> {
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM speed_dial_item", [], |r| r.get(0))?;
    if count >= MAX_SPEED_DIAL_ITEMS as i64 {
        // SpeedDialDao.oldest() — ORDER BY createDate ASC LIMIT 1.
        let oldest: Option<String> = conn
            .query_row(
                "SELECT id FROM speed_dial_item ORDER BY createDate ASC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(oldest) = oldest {
            conn.execute("DELETE FROM speed_dial_item WHERE id = ?1", [oldest])?;
        }
    }
    conn.execute(
        "INSERT OR REPLACE INTO speed_dial_item (id, secondaryId, title, subtitle, thumbnailUrl, type, explicit, createDate) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        rusqlite::params![
            item.id,
            item.secondary_id,
            item.title,
            item.subtitle,
            item.thumbnail_url,
            item.item_type,
            item.explicit,
            item.create_date
        ],
    )?;
    Ok(())
}

/// `SpeedDialDao.delete(id)`.
pub fn delete_speed_dial_item(conn: &Connection, id: &str) -> DbResult<()> {
    conn.execute("DELETE FROM speed_dial_item WHERE id = ?1", [id])?;
    Ok(())
}

/// `SpeedDialDao.isPinned(id)`.
pub fn is_pinned(conn: &Connection, id: &str) -> DbResult<bool> {
    let n: i64 = conn.query_row(
        "SELECT EXISTS(SELECT * FROM speed_dial_item WHERE id = ?1)",
        [id],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

// ---------------------------------------------------------------------------
// Song edits (DatabaseDao.kt:1729-1745)
// ---------------------------------------------------------------------------

/// `songEditBlocking(songId)` (DatabaseDao.kt:1736-1737).
pub fn song_edit(conn: &Connection, song_id: &str) -> DbResult<Option<SongEdit>> {
    conn.query_row(
        "SELECT * FROM song_edit WHERE songId = ?1",
        [song_id],
        SongEdit::from_row,
    )
    .optional()
    .map_err(DbError::Sqlite)
}

/// `songEdits()` (DatabaseDao.kt:1739-1740).
pub fn song_edits(conn: &Connection) -> DbResult<Vec<SongEdit>> {
    let mut stmt = conn.prepare("SELECT * FROM song_edit")?;
    let rows = stmt.query_map([], SongEdit::from_row)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// `upsert(edit: SongEdit)` (DatabaseDao.kt:1742-1743).
pub fn upsert_song_edit(conn: &Connection, edit: &SongEdit) -> DbResult<()> {
    conn.execute(
        "INSERT OR REPLACE INTO song_edit (songId, title, artists, album, year, cover, videoId, noVideo) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        rusqlite::params![
            edit.song_id,
            edit.title,
            edit.artists,
            edit.album,
            edit.year,
            edit.cover,
            edit.video_id,
            edit.no_video
        ],
    )?;
    Ok(())
}

/// `deleteSongEdit`-shaped removal.
pub fn delete_song_edit(conn: &Connection, song_id: &str) -> DbResult<()> {
    conn.execute("DELETE FROM song_edit WHERE songId = ?1", [song_id])?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Import bookkeeping (DatabaseDao.kt:1747-1943)
// ---------------------------------------------------------------------------

/// `insert(imported: ImportedSong)` — IGNORE (DatabaseDao.kt:1747-1748).
pub fn insert_imported_song(conn: &Connection, row: &ImportedSong) -> DbResult<()> {
    conn.execute(
        "INSERT OR IGNORE INTO imported_song (songId, source, importedAt) VALUES (?1, ?2, ?3)",
        rusqlite::params![row.song_id, row.source, row.imported_at],
    )?;
    Ok(())
}

/// `importedSourceCounts()` (DatabaseDao.kt:1750-1756): only songs still in
/// the library or liked count as a service's.
pub fn imported_source_counts(conn: &Connection) -> DbResult<Vec<(String, i64)>> {
    let mut stmt = conn.prepare(
        "SELECT imported_song.source, COUNT(*) AS songCount FROM imported_song \
         JOIN song ON song.id = imported_song.songId \
         WHERE song.inLibrary IS NOT NULL OR song.liked GROUP BY imported_song.source",
    )?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// `insert(session: ImportSession)` (DatabaseDao.kt:1585-1586 shape).
pub fn insert_import_session(conn: &Connection, row: &ImportSession) -> DbResult<i64> {
    conn.execute(
        "INSERT INTO import_session (source, startedAt) VALUES (?1, ?2)",
        rusqlite::params![row.source, row.started_at],
    )?;
    Ok(conn.last_insert_rowid())
}

/// `importSessions()`-shaped list.
pub fn import_sessions(conn: &Connection) -> DbResult<Vec<ImportSession>> {
    let mut stmt = conn.prepare("SELECT id, source, startedAt FROM import_session ORDER BY id")?;
    let rows = stmt.query_map([], |r| {
        Ok(ImportSession {
            id: r.get(0)?,
            source: r.get(1)?,
            started_at: r.get(2)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// `deleteImportSession(sessionId)` + songs + imported rows
/// (DatabaseDao.kt:1850-1877, `deleteImportedPlaylist` 1933-1943).
pub fn delete_import_session(conn: &Connection, session_id: i64) -> DbResult<()> {
    conn.execute(
        "DELETE FROM import_session_song WHERE sessionId = ?1",
        [session_id],
    )?;
    conn.execute(
        "DELETE FROM imported_playlist WHERE sessionId = ?1 AND source != 'youtube_music'",
        [session_id],
    )?;
    conn.execute("DELETE FROM import_session WHERE id = ?1", [session_id])?;
    Ok(())
}

/// `importedPlaylists()`-shaped list.
pub fn imported_playlists(conn: &Connection) -> DbResult<Vec<ImportedPlaylist>> {
    let mut stmt = conn.prepare(
        "SELECT playlistId, source, sessionId, importedAt, remoteId, remoteUrl, lastSyncedAt, \
         authorName, authorAvatarUrl, authorUrl FROM imported_playlist",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(ImportedPlaylist {
            playlist_id: r.get(0)?,
            source: r.get(1)?,
            session_id: r.get(2)?,
            imported_at: r.get(3)?,
            remote_id: r.get(4)?,
            remote_url: r.get(5)?,
            last_synced_at: r.get(6)?,
            author_name: r.get(7)?,
            author_avatar_url: r.get(8)?,
            author_url: r.get(9)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// `insert(importedPlaylist)`-shaped IGNORE insert.
pub fn insert_imported_playlist(conn: &Connection, row: &ImportedPlaylist) -> DbResult<()> {
    conn.execute(
        "INSERT OR IGNORE INTO imported_playlist (playlistId, source, sessionId, importedAt, remoteId, remoteUrl, \
         lastSyncedAt, authorName, authorAvatarUrl, authorUrl) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        rusqlite::params![
            row.playlist_id,
            row.source,
            row.session_id,
            row.imported_at,
            row.remote_id,
            row.remote_url,
            row.last_synced_at,
            row.author_name,
            row.author_avatar_url,
            row.author_url
        ],
    )?;
    Ok(())
}
