//! Repositories per aggregate, over the [`Db`] actor.
//!
//! Each repository wraps the sync query layer ([`crate::queries`]) with the
//! actor's two paths: reads take a pooled connection, writes go through the
//! single writer and fire the table invalidation after commit (blueprint
//! ch. 7.3). Persistence stays pure: the network side-effects that upstream
//! embedded in entity methods (`SongEntity.toggleLike` ->
//! `YouTube.likeVideo`, SongEntity.kt:64-72) are NOT here — Phase 14's
//! application SongService calls these repositories for the row change and
//! performs the YouTube call itself (documented hook, blueprint ch. 7.3).
//!
//! Read methods are blocking (WAL reads are sub-millisecond); write methods
//! are async (single-writer actor). Later GPUI entities subscribe via
//! [`Db::watch`] / [`Db::subscribe`] and re-run their reads on invalidation.

use crate::db::Db;
use crate::entities::*;
use crate::error::DbResult;
use crate::queries::{self, *};
use crate::watch::{Table, TableSet};

fn tables(ts: &[Table]) -> TableSet {
    TableSet::of(ts)
}

// ---------------------------------------------------------------------------
// SongRepository
// ---------------------------------------------------------------------------

/// The `song` aggregate (plus its maps and format row).
#[derive(Debug, Clone)]
pub struct SongRepository {
    db: std::sync::Arc<Db>,
}

impl SongRepository {
    pub(crate) fn new(db: std::sync::Arc<Db>) -> Self {
        Self { db }
    }

    /// Full song relation (song + artists + album + format).
    pub fn song(&self, id: &str) -> DbResult<Option<Song>> {
        self.db.read(|conn| queries::song(conn, id))
    }

    /// Library songs (DatabaseDao.kt:92-124).
    pub fn songs_in_library(&self, sort: SongSort, desc: bool) -> DbResult<Vec<Song>> {
        self.db
            .read(|conn| queries::songs_in_library(conn, sort, desc))
    }

    /// Liked songs (DatabaseDao.kt:176-224).
    pub fn liked_songs(&self, sort: SongSort, desc: bool) -> DbResult<Vec<Song>> {
        self.db.read(|conn| queries::liked_songs(conn, sort, desc))
    }

    /// Liked song count (DatabaseDao.kt:226-228).
    pub fn liked_songs_count(&self) -> DbResult<i64> {
        self.db.read(queries::liked_songs_count)
    }

    /// Local (imported-file) songs (DatabaseDao.kt:126-174).
    pub fn local_songs(&self, sort: SongSort, desc: bool) -> DbResult<Vec<Song>> {
        self.db.read(|conn| queries::local_songs(conn, sort, desc))
    }

    /// Downloaded songs (DatabaseDao.kt:1280-1306).
    pub fn downloaded_songs(&self, sort: SongSort, desc: bool) -> DbResult<Vec<Song>> {
        self.db
            .read(|conn| queries::downloaded_songs(conn, sort, desc))
    }

    /// Ids liked after `since` (DatabaseDao.kt:1211-1213).
    pub fn liked_song_ids_since(&self, since: i64) -> DbResult<Vec<String>> {
        self.db
            .read(|conn| queries::liked_song_ids_since(conn, since))
    }

    /// Insert song + artists + positions (DatabaseDao.kt:1599-1631).
    /// The `now_ms` stamps generated artists' `lastUpdateTime`.
    pub async fn insert_song_with_artists(
        &self,
        song: SongEntity,
        artists: Vec<(Option<String>, String)>,
        now_ms: i64,
    ) -> DbResult<bool> {
        self.db
            .write(
                tables(&[Table::Song, Table::Artist, Table::SongArtistMap]),
                move |conn| queries::insert_song_with_artists(conn, &song, &artists, now_ms),
            )
            .await
    }

    /// `SongEntity.toggleLike()` state machine — persistence only; the
    /// `YouTube.likeVideo` call is the Phase 14 SongService hook.
    pub async fn toggle_like(&self, id: String, now_ms: i64) -> DbResult<Option<SongEntity>> {
        self.db
            .write(tables(&[Table::Song]), move |conn| {
                queries::toggle_like(conn, &id, now_ms)
            })
            .await
    }

    /// `SongEntity.toggleLibrary(by_like)` state machine — persistence only
    /// (`syncToYouTube` is the Phase 14 hook).
    pub async fn toggle_library(
        &self,
        id: String,
        now_ms: i64,
        by_like: bool,
    ) -> DbResult<Option<SongEntity>> {
        self.db
            .write(tables(&[Table::Song]), move |conn| {
                queries::toggle_library(conn, &id, now_ms, by_like)
            })
            .await
    }

    /// `inLibrary(...)` direct set (DatabaseDao.kt:1481-1485).
    pub async fn set_in_library(&self, id: String, in_library: Option<i64>) -> DbResult<()> {
        self.db
            .write(tables(&[Table::Song]), move |conn| {
                queries::set_in_library(conn, &id, in_library)
            })
            .await
    }

    /// Library tokens from a YouTube library response
    /// (DatabaseDao.kt:1487-1492).
    pub async fn set_library_tokens(
        &self,
        id: String,
        add_token: Option<String>,
        remove_token: Option<String>,
    ) -> DbResult<()> {
        self.db
            .write(tables(&[Table::Song]), move |conn| {
                queries::set_library_tokens(
                    conn,
                    &id,
                    add_token.as_deref(),
                    remove_token.as_deref(),
                )
            })
            .await
    }

    /// `incrementTotalPlayTime` (DatabaseDao.kt:1573-1574).
    pub async fn increment_total_play_time(&self, id: String, play_time_ms: i64) -> DbResult<()> {
        self.db
            .write(tables(&[Table::Song]), move |conn| {
                queries::increment_total_play_time(conn, &id, play_time_ms)
            })
            .await
    }

    /// Lyrics offset edit (DatabaseDao.kt:1726-1727).
    pub async fn set_lyrics_offset(&self, id: String, offset_ms: i32) -> DbResult<()> {
        self.db
            .write(tables(&[Table::Song]), move |conn| {
                queries::set_lyrics_offset(conn, &id, offset_ms)
            })
            .await
    }

    /// Download state change (DatabaseDao.kt:1308-1309).
    pub async fn update_downloaded_info(
        &self,
        id: String,
        downloaded: bool,
        date: Option<i64>,
    ) -> DbResult<()> {
        self.db
            .write(tables(&[Table::Song]), move |conn| {
                queries::update_downloaded_info(conn, &id, downloaded, date)
            })
            .await
    }

    /// Local file genre tag (DatabaseDao.kt:1145-1146).
    pub async fn set_song_genre(&self, id: String, genre: Option<String>) -> DbResult<()> {
        self.db
            .write(tables(&[Table::Song]), move |conn| {
                queries::set_song_genre(conn, &id, genre.as_deref())
            })
            .await
    }

    /// Delete a song (cascades maps/events through FKs, DatabaseDao.kt:2078).
    pub async fn delete_song(&self, id: String) -> DbResult<()> {
        self.db
            .write(
                tables(&[
                    Table::Song,
                    Table::SongArtistMap,
                    Table::SongAlbumMap,
                    Table::Event,
                    Table::RelatedSongMap,
                ]),
                move |conn| queries::delete_song(conn, &id),
            )
            .await
    }
}

// ---------------------------------------------------------------------------
// ArtistRepository
// ---------------------------------------------------------------------------

/// The `artist` aggregate.
#[derive(Debug, Clone)]
pub struct ArtistRepository {
    db: std::sync::Arc<Db>,
}

impl ArtistRepository {
    pub(crate) fn new(db: std::sync::Arc<Db>) -> Self {
        Self { db }
    }

    /// Artist with library song count (DatabaseDao.kt:864-866).
    pub fn artist(&self, id: &str) -> DbResult<Option<Artist>> {
        self.db.read(|conn| queries::artist(conn, id))
    }

    /// Library artists (DatabaseDao.kt:840-850).
    pub fn artists(&self, sort: ArtistSort, desc: bool) -> DbResult<Vec<Artist>> {
        self.db.read(|conn| queries::artists(conn, sort, desc))
    }

    /// Bookmarked (subscribed) artists (DatabaseDao.kt:804-862).
    pub fn artists_bookmarked(&self, sort: ArtistSort, desc: bool) -> DbResult<Vec<Artist>> {
        self.db
            .read(|conn| queries::artists_bookmarked(conn, sort, desc))
    }

    /// An artist's library songs (DatabaseDao.kt:241-290).
    pub fn artist_songs(&self, artist_id: &str, sort: SongSort, desc: bool) -> DbResult<Vec<Song>> {
        self.db
            .read(|conn| queries::artist_songs(conn, artist_id, sort, desc))
    }

    /// Insert an artist (IGNORE, DatabaseDao.kt:1553-1554).
    pub async fn insert_artist(&self, artist: ArtistEntity) -> DbResult<bool> {
        self.db
            .write(tables(&[Table::Artist]), move |conn| {
                queries::insert_artist(conn, &artist)
            })
            .await
    }

    /// Bookmark toggle — `ArtistEntity.localToggleLike()`
    /// (ArtistEntity.kt:38-40); the `YouTube.subscribeChannel` call is the
    /// Phase 14 hook.
    pub async fn toggle_bookmark(&self, id: String, now_ms: i64) -> DbResult<Option<ArtistEntity>> {
        self.db
            .write(tables(&[Table::Artist]), move |conn| {
                queries::toggle_bookmark_artist(conn, &id, now_ms)
            })
            .await
    }
}

// ---------------------------------------------------------------------------
// AlbumRepository
// ---------------------------------------------------------------------------

/// The `album` aggregate.
#[derive(Debug, Clone)]
pub struct AlbumRepository {
    db: std::sync::Arc<Db>,
}

impl AlbumRepository {
    pub(crate) fn new(db: std::sync::Arc<Db>) -> Self {
        Self { db }
    }

    /// Album + artists.
    pub fn album(&self, id: &str) -> DbResult<Option<Album>> {
        self.db.read(|conn| queries::album(conn, id))
    }

    /// Library albums (DatabaseDao.kt:870-1010).
    pub fn albums(&self, sort: AlbumSort, desc: bool) -> DbResult<Vec<Album>> {
        self.db.read(|conn| queries::albums(conn, sort, desc))
    }

    /// Album songs — the `sorted_song_album_map` consuming query
    /// (DatabaseDao.kt:230-232).
    pub fn album_songs(&self, album_id: &str) -> DbResult<Vec<Song>> {
        self.db.read(|conn| queries::album_songs(conn, album_id))
    }

    /// Insert an album (IGNORE, DatabaseDao.kt:1556-1557).
    pub async fn insert_album(&self, album: AlbumEntity) -> DbResult<bool> {
        self.db
            .write(tables(&[Table::Album]), move |conn| {
                queries::insert_album(conn, &album)
            })
            .await
    }

    /// Bookmark toggle — `AlbumEntity.localToggleLike()` (AlbumEntity.kt:30-32);
    /// `YouTube.likePlaylist` is the Phase 14 hook.
    pub async fn toggle_bookmark(&self, id: String, now_ms: i64) -> DbResult<Option<AlbumEntity>> {
        self.db
            .write(tables(&[Table::Album]), move |conn| {
                queries::toggle_bookmark_album(conn, &id, now_ms)
            })
            .await
    }

    /// Library toggle — `AlbumEntity.toggleLibrary()` (AlbumEntity.kt:34-36).
    pub async fn toggle_library(&self, id: String, now_ms: i64) -> DbResult<Option<AlbumEntity>> {
        self.db
            .write(tables(&[Table::Album]), move |conn| {
                queries::toggle_album_library(conn, &id, now_ms)
            })
            .await
    }

    /// Re-count a local album (DatabaseDao.kt:1132-1140).
    pub async fn refresh_local_album(
        &self,
        album_id: String,
        thumbnail_url: Option<String>,
        year: Option<i32>,
    ) -> DbResult<()> {
        self.db
            .write(tables(&[Table::Album]), move |conn| {
                queries::refresh_local_album(conn, &album_id, thumbnail_url.as_deref(), year)
            })
            .await
    }
}

// ---------------------------------------------------------------------------
// PlaylistRepository
// ---------------------------------------------------------------------------

/// The `playlist` aggregate.
#[derive(Debug, Clone)]
pub struct PlaylistRepository {
    db: std::sync::Arc<Db>,
}

impl PlaylistRepository {
    pub(crate) fn new(db: std::sync::Arc<Db>) -> Self {
        Self { db }
    }

    /// Playlist + songCount + preview thumbnails
    /// (`playlist_song_map_preview` consumer).
    pub fn playlist(&self, id: &str) -> DbResult<Option<Playlist>> {
        self.db.read(|conn| queries::playlist(conn, id))
    }

    /// Pinned playlists (DatabaseDao.kt:1153-1179).
    pub fn playlists(&self, sort: PlaylistSort, desc: bool) -> DbResult<Vec<Playlist>> {
        self.db.read(|conn| queries::playlists(conn, sort, desc))
    }

    /// Editable pinned playlists (DatabaseDao.kt:1177-1179).
    pub fn editable_playlists(&self) -> DbResult<Vec<Playlist>> {
        self.db.read(queries::editable_playlists)
    }

    /// Playlist songs, ORDER BY position (DatabaseDao.kt:234-236).
    pub fn playlist_songs(&self, id: &str) -> DbResult<Vec<PlaylistSong>> {
        self.db.read(|conn| queries::playlist_songs(conn, id))
    }

    /// Song ids of a playlist (DatabaseDao.kt:1244-1245).
    pub fn playlist_song_ids(&self, id: &str) -> DbResult<Vec<String>> {
        self.db.read(|conn| queries::playlist_song_ids(conn, id))
    }

    /// Insert a playlist (IGNORE, DatabaseDao.kt:1558-1559).
    pub async fn insert_playlist(&self, playlist: PlaylistEntity) -> DbResult<bool> {
        self.db
            .write(tables(&[Table::Playlist]), move |conn| {
                queries::insert_playlist(conn, &playlist)
            })
            .await
    }

    /// Pin toggle — `PlaylistEntity.localToggleLike()` (PlaylistEntity.kt:48-50).
    pub async fn toggle_pin(&self, id: String, now_ms: i64) -> DbResult<Option<PlaylistEntity>> {
        self.db
            .write(tables(&[Table::Playlist]), move |conn| {
                queries::toggle_pin_playlist(conn, &id, now_ms)
            })
            .await
    }

    /// Append songs (DatabaseDao.kt:1261-1270).
    pub async fn add_songs(&self, playlist_id: String, song_ids: Vec<String>) -> DbResult<()> {
        self.db
            .write(
                tables(&[Table::PlaylistSongMap, Table::Playlist]),
                move |conn| queries::add_songs_to_playlist(conn, &playlist_id, &song_ids),
            )
            .await
    }

    /// Prepend songs for YT-mirroring playlists (DatabaseDao.kt:1253-1259).
    pub async fn add_songs_to_top(
        &self,
        playlist_id: String,
        song_ids: Vec<String>,
    ) -> DbResult<()> {
        self.db
            .write(
                tables(&[Table::PlaylistSongMap, Table::Playlist]),
                move |conn| queries::add_songs_to_playlist_top(conn, &playlist_id, &song_ids),
            )
            .await
    }

    /// Move a song within a playlist (DatabaseDao.kt:1494-1510).
    pub async fn move_song(
        &self,
        playlist_id: String,
        from_position: i32,
        to_position: i32,
    ) -> DbResult<()> {
        self.db
            .write(tables(&[Table::PlaylistSongMap]), move |conn| {
                queries::move_playlist_song(conn, &playlist_id, from_position, to_position)
            })
            .await
    }

    /// Remove rows (undo of an append, DatabaseDao.kt:1239-1241).
    pub async fn remove_songs_from_position(
        &self,
        playlist_id: String,
        song_ids: Vec<String>,
        from_position: i32,
    ) -> DbResult<()> {
        self.db
            .write(tables(&[Table::PlaylistSongMap]), move |conn| {
                queries::remove_playlist_songs_from_position(
                    conn,
                    &playlist_id,
                    &song_ids,
                    from_position,
                )
            })
            .await
    }

    /// Swap the cover if unchanged (DatabaseDao.kt:1215-1217).
    pub async fn replace_cover(
        &self,
        playlist_id: String,
        expected: Option<String>,
        thumbnail_url: String,
    ) -> DbResult<bool> {
        self.db
            .write(tables(&[Table::Playlist]), move |conn| {
                queries::replace_playlist_cover(
                    conn,
                    &playlist_id,
                    expected.as_deref(),
                    &thumbnail_url,
                )
            })
            .await
    }

    /// Clear all songs (DatabaseDao.kt:1512-1513).
    pub async fn clear(&self, playlist_id: String) -> DbResult<()> {
        self.db
            .write(tables(&[Table::PlaylistSongMap]), move |conn| {
                queries::clear_playlist(conn, &playlist_id)
            })
            .await
    }

    /// Delete the playlist (DatabaseDao.kt:2099-2100).
    pub async fn delete(&self, playlist_id: String) -> DbResult<()> {
        self.db
            .write(
                tables(&[Table::Playlist, Table::PlaylistSongMap]),
                move |conn| queries::delete_playlist(conn, &playlist_id),
            )
            .await
    }
}

// ---------------------------------------------------------------------------
// FormatRepository
// ---------------------------------------------------------------------------

/// The stream-format cache.
#[derive(Debug, Clone)]
pub struct FormatRepository {
    db: std::sync::Arc<Db>,
}

impl FormatRepository {
    pub(crate) fn new(db: std::sync::Arc<Db>) -> Self {
        Self { db }
    }

    /// Cached format for a song.
    pub fn format(&self, id: &str) -> DbResult<Option<FormatEntity>> {
        self.db.read(|conn| queries::format(conn, id))
    }

    /// Cache a format row.
    pub async fn upsert(&self, f: FormatEntity) -> DbResult<()> {
        self.db
            .write(tables(&[Table::Format]), move |conn| {
                queries::upsert_format(conn, &f)
            })
            .await
    }

    /// Drop a cache row (DatabaseDao.kt:1944-1945).
    pub async fn delete(&self, id: String) -> DbResult<()> {
        self.db
            .write(tables(&[Table::Format]), move |conn| {
                queries::delete_format(conn, &id)
            })
            .await
    }
}

// ---------------------------------------------------------------------------
// LyricsRepository
// ---------------------------------------------------------------------------

/// The lyrics cache (sentinel + provider + translation columns).
#[derive(Debug, Clone)]
pub struct LyricsRepository {
    db: std::sync::Arc<Db>,
}

impl LyricsRepository {
    pub(crate) fn new(db: std::sync::Arc<Db>) -> Self {
        Self { db }
    }

    /// Cached lyrics for a song (LYRICS_NOT_FOUND sentinel included).
    pub fn lyrics(&self, id: &str) -> DbResult<Option<LyricsEntity>> {
        self.db.read(|conn| queries::lyrics(conn, id))
    }

    /// Save lyrics or the negative sentinel.
    pub async fn upsert(&self, l: LyricsEntity) -> DbResult<()> {
        self.db
            .write(tables(&[Table::Lyrics]), move |conn| {
                queries::upsert_lyrics(conn, &l)
            })
            .await
    }

    /// Wipe fetched lyrics, keeping manual sources
    /// (DatabaseDao.kt:1588-1590).
    pub async fn clear_fetched(&self, keep_providers: Vec<String>) -> DbResult<usize> {
        self.db
            .write(tables(&[Table::Lyrics]), move |conn| {
                queries::clear_fetched_lyrics(conn, &keep_providers)
            })
            .await
    }
}

// ---------------------------------------------------------------------------
// EventRepository (event + playCount + playlist_event)
// ---------------------------------------------------------------------------

/// Listening history and monthly aggregates.
#[derive(Debug, Clone)]
pub struct EventRepository {
    db: std::sync::Arc<Db>,
}

impl EventRepository {
    pub(crate) fn new(db: std::sync::Arc<Db>) -> Self {
        Self { db }
    }

    /// Events in a UTC-ms range, with songs.
    pub fn events_in_range(&self, from_ms: i64, to_ms: i64) -> DbResult<Vec<EventWithSong>> {
        self.db
            .read(|conn| queries::events_in_range(conn, from_ms, to_ms))
    }

    /// Total listening time (DatabaseDao.kt:292-295).
    pub fn total_listening_time_ms(&self) -> DbResult<i64> {
        self.db.read(queries::total_listening_time_ms)
    }

    /// Lifetime play count for a song (DatabaseDao.kt:635-636).
    pub fn lifetime_play_count(&self, song_id: &str) -> DbResult<i64> {
        self.db
            .read(|conn| queries::lifetime_play_count(conn, song_id))
    }

    /// Record a play that crossed the history threshold
    /// (MusicService.kt:4370-4379 + DatabaseDao.kt:1577-1578), update the
    /// song's total, and bump the monthly count
    /// (DatabaseDao.kt:1466-1476). One write transaction so the three
    /// tables move together.
    pub async fn report_play(
        &self,
        song_id: String,
        now_ms: i64,
        play_time_ms: i64,
    ) -> DbResult<()> {
        self.db
            .write(
                tables(&[Table::Event, Table::PlayCount, Table::Song]),
                move |conn| {
                    queries::insert_event(
                        conn,
                        &Event {
                            id: 0,
                            song_id: song_id.clone(),
                            timestamp: now_ms,
                            play_time: play_time_ms,
                        },
                    )?;
                    queries::increment_total_play_time(conn, &song_id, play_time_ms)?;
                    queries::increment_play_count(conn, &song_id, now_ms)?;
                    Ok(())
                },
            )
            .await
    }

    /// Playlist play event (DatabaseDao.kt:1580-1581).
    pub async fn report_playlist_play(&self, playlist_id: String, now_ms: i64) -> DbResult<()> {
        self.db
            .write(tables(&[Table::PlaylistEvent]), move |conn| {
                queries::insert_playlist_event(
                    conn,
                    &PlaylistEvent {
                        id: 0,
                        playlist_id,
                        timestamp: now_ms,
                    },
                )
            })
            .await
    }
}

// ---------------------------------------------------------------------------
// SearchHistoryRepository
// ---------------------------------------------------------------------------

/// The search-history store.
#[derive(Debug, Clone)]
pub struct SearchHistoryRepository {
    db: std::sync::Arc<Db>,
}

impl SearchHistoryRepository {
    pub(crate) fn new(db: std::sync::Arc<Db>) -> Self {
        Self { db }
    }

    /// Prefix search over history (DatabaseDao.kt:1519-1521).
    pub fn search(&self, query: &str) -> DbResult<Vec<SearchHistory>> {
        self.db.read(|conn| queries::search_history(conn, query))
    }

    /// Insert-or-move-to-top (DatabaseDao.kt:1568-1569).
    pub async fn upsert(&self, query: String) -> DbResult<()> {
        self.db
            .write(tables(&[Table::SearchHistory]), move |conn| {
                queries::upsert_search_history(conn, &query)
            })
            .await
    }

    /// Delete one entry (DatabaseDao.kt:2105-2106).
    pub async fn delete(&self, query: String) -> DbResult<()> {
        self.db
            .write(tables(&[Table::SearchHistory]), move |conn| {
                queries::delete_search_history(conn, &query)
            })
            .await
    }
}

// ---------------------------------------------------------------------------
// RadioRepository (related_song_map + set_video_id)
// ---------------------------------------------------------------------------

/// Radio adjacency + queue-id caches.
#[derive(Debug, Clone)]
pub struct RadioRepository {
    db: std::sync::Arc<Db>,
}

impl RadioRepository {
    pub(crate) fn new(db: std::sync::Arc<Db>) -> Self {
        Self { db }
    }

    /// `hasRelatedSongs` (DatabaseDao.kt:1497-1498).
    pub fn has_related_songs(&self, song_id: &str) -> DbResult<bool> {
        self.db
            .read(|conn| queries::has_related_songs(conn, song_id))
    }

    /// Radio neighbours, deduped (DatabaseDao.kt:1512-1524).
    pub fn related_songs(&self, song_id: &str) -> DbResult<Vec<Song>> {
        self.db.read(|conn| queries::related_songs(conn, song_id))
    }

    /// Cache an adjacency row (DatabaseDao.kt:1583-1584).
    pub async fn insert_related_song(
        &self,
        song_id: String,
        related_song_id: String,
    ) -> DbResult<()> {
        self.db
            .write(tables(&[Table::RelatedSongMap]), move |conn| {
                queries::insert_related_song(conn, &song_id, &related_song_id)
            })
            .await
    }

    /// Queue-id lookup for YT playlist moves.
    pub fn set_video_id(&self, video_id: &str) -> DbResult<Option<SetVideoIdEntity>> {
        self.db.read(|conn| queries::set_video_id(conn, video_id))
    }

    /// Queue-id cache write.
    pub async fn upsert_set_video_id(&self, entity: SetVideoIdEntity) -> DbResult<()> {
        self.db
            .write(tables(&[Table::SetVideoId]), move |conn| {
                queries::upsert_set_video_id(conn, &entity)
            })
            .await
    }
}

// ---------------------------------------------------------------------------
// RecognitionRepository
// ---------------------------------------------------------------------------

/// Shazam recognition history.
#[derive(Debug, Clone)]
pub struct RecognitionRepository {
    db: std::sync::Arc<Db>,
}

impl RecognitionRepository {
    pub(crate) fn new(db: std::sync::Arc<Db>) -> Self {
        Self { db }
    }

    /// Search history rows (DatabaseDao.kt:1440-1442).
    pub fn search(&self, query: &str) -> DbResult<Vec<RecognitionHistory>> {
        self.db
            .read(|conn| queries::search_recognition_history(conn, query))
    }

    /// Insert a recognition (DatabaseDao.kt:1456-1457).
    pub async fn upsert(&self, r: RecognitionHistory) -> DbResult<()> {
        self.db
            .write(tables(&[Table::RecognitionHistory]), move |conn| {
                queries::upsert_recognition_history(conn, &r)
            })
            .await
    }

    /// Like a recognition (DatabaseDao.kt:1447-1448).
    pub async fn set_liked(&self, id: i64, liked: bool) -> DbResult<()> {
        self.db
            .write(tables(&[Table::RecognitionHistory]), move |conn| {
                queries::set_recognition_liked(conn, id, liked)
            })
            .await
    }

    /// Clear all (DatabaseDao.kt:1444-1445).
    pub async fn clear(&self) -> DbResult<()> {
        self.db
            .write(tables(&[Table::RecognitionHistory]), move |conn| {
                queries::clear_recognition_history(conn)
            })
            .await
    }
}

// ---------------------------------------------------------------------------
// SpeedDialRepository
// ---------------------------------------------------------------------------

/// The pinned 3x3 grid (daos/SpeedDialDao.kt).
#[derive(Debug, Clone)]
pub struct SpeedDialRepository {
    db: std::sync::Arc<Db>,
}

impl SpeedDialRepository {
    pub(crate) fn new(db: std::sync::Arc<Db>) -> Self {
        Self { db }
    }

    /// Pinned items, newest first (SpeedDialDao.getAll).
    pub fn items(&self) -> DbResult<Vec<SpeedDialItem>> {
        self.db.read(queries::speed_dial_items)
    }

    /// Pin with the 9-item cap (`MusicDatabase.pinToSpeedDial`,
    /// MusicDatabase.kt:63-74) — the one place pins are created.
    pub async fn pin(&self, item: SpeedDialItem) -> DbResult<()> {
        self.db
            .write(tables(&[Table::SpeedDialItem]), move |conn| {
                queries::pin_to_speed_dial(conn, &item)
            })
            .await
    }

    /// Unpin (SpeedDialDao.delete).
    pub async fn delete(&self, id: String) -> DbResult<()> {
        self.db
            .write(tables(&[Table::SpeedDialItem]), move |conn| {
                queries::delete_speed_dial_item(conn, &id)
            })
            .await
    }

    /// Pinned? (SpeedDialDao.isPinned).
    pub fn is_pinned(&self, id: &str) -> DbResult<bool> {
        self.db.read(|conn| queries::is_pinned(conn, id))
    }
}

// ---------------------------------------------------------------------------
// SongEditRepository
// ---------------------------------------------------------------------------

/// The hand-edit overlay (SongEdit.kt).
#[derive(Debug, Clone)]
pub struct SongEditRepository {
    db: std::sync::Arc<Db>,
}

impl SongEditRepository {
    pub(crate) fn new(db: std::sync::Arc<Db>) -> Self {
        Self { db }
    }

    /// One edit row (DatabaseDao.kt:1736-1737).
    pub fn song_edit(&self, song_id: &str) -> DbResult<Option<SongEdit>> {
        self.db.read(|conn| queries::song_edit(conn, song_id))
    }

    /// All edits (DatabaseDao.kt:1739-1740).
    pub fn song_edits(&self) -> DbResult<Vec<SongEdit>> {
        self.db.read(queries::song_edits)
    }

    /// Save an edit (@Upsert, DatabaseDao.kt:1742-1743).
    pub async fn upsert(&self, edit: SongEdit) -> DbResult<()> {
        self.db
            .write(tables(&[Table::SongEdit]), move |conn| {
                queries::upsert_song_edit(conn, &edit)
            })
            .await
    }

    /// Remove an edit.
    pub async fn delete(&self, song_id: String) -> DbResult<()> {
        self.db
            .write(tables(&[Table::SongEdit]), move |conn| {
                queries::delete_song_edit(conn, &song_id)
            })
            .await
    }
}

// ---------------------------------------------------------------------------
// ImportBookkeepingRepository
// ---------------------------------------------------------------------------

/// The import_* tables (Phase 22 extends this with the full session/undo
/// engine; Phase 03 provides the bookkeeping reads/writes the importer and
/// library screens need).
#[derive(Debug, Clone)]
pub struct ImportBookkeepingRepository {
    db: std::sync::Arc<Db>,
}

impl ImportBookkeepingRepository {
    pub(crate) fn new(db: std::sync::Arc<Db>) -> Self {
        Self { db }
    }

    /// Songs each service brought in (DatabaseDao.kt:1750-1756).
    pub fn source_counts(&self) -> DbResult<Vec<(String, i64)>> {
        self.db.read(queries::imported_source_counts)
    }

    /// All sessions.
    pub fn sessions(&self) -> DbResult<Vec<ImportSession>> {
        self.db.read(queries::import_sessions)
    }

    /// All imported playlists.
    pub fn imported_playlists(&self) -> DbResult<Vec<ImportedPlaylist>> {
        self.db.read(queries::imported_playlists)
    }

    /// Record an imported song (IGNORE, DatabaseDao.kt:1747-1748).
    pub async fn insert_imported_song(&self, row: ImportedSong) -> DbResult<()> {
        self.db
            .write(tables(&[Table::ImportedSong]), move |conn| {
                queries::insert_imported_song(conn, &row)
            })
            .await
    }

    /// Start a session; returns its id.
    pub async fn start_session(&self, source: String, now_ms: i64) -> DbResult<i64> {
        self.db
            .write(tables(&[Table::ImportSession]), move |conn| {
                queries::insert_import_session(
                    conn,
                    &ImportSession {
                        id: 0,
                        source,
                        started_at: now_ms,
                    },
                )
            })
            .await
    }

    /// Undo a session (DatabaseDao.kt:1850-1877).
    pub async fn delete_session(&self, session_id: i64) -> DbResult<()> {
        self.db
            .write(
                tables(&[
                    Table::ImportSession,
                    Table::ImportSessionSong,
                    Table::ImportedPlaylist,
                ]),
                move |conn| queries::delete_import_session(conn, session_id),
            )
            .await
    }

    /// Record an imported playlist (IGNORE).
    pub async fn insert_imported_playlist(&self, row: ImportedPlaylist) -> DbResult<()> {
        self.db
            .write(tables(&[Table::ImportedPlaylist]), move |conn| {
                queries::insert_imported_playlist(conn, &row)
            })
            .await
    }
}

// ---------------------------------------------------------------------------
// The bundle
// ---------------------------------------------------------------------------

/// All repositories over one Db. Constructed via [`Repositories::new`].
#[derive(Debug, Clone)]
pub struct Repositories {
    /// The `song` aggregate.
    pub songs: SongRepository,
    /// The `artist` aggregate.
    pub artists: ArtistRepository,
    /// The `album` aggregate.
    pub albums: AlbumRepository,
    /// The `playlist` aggregate.
    pub playlists: PlaylistRepository,
    /// The stream-format cache.
    pub formats: FormatRepository,
    /// The lyrics cache.
    pub lyrics: LyricsRepository,
    /// Listening history + play counts.
    pub events: EventRepository,
    /// The search-history store.
    pub search_history: SearchHistoryRepository,
    /// Radio adjacency + queue-id caches.
    pub radio: RadioRepository,
    /// Shazam recognition history.
    pub recognition: RecognitionRepository,
    /// The pinned speed-dial grid.
    pub speed_dial: SpeedDialRepository,
    /// The song-edit overlay.
    pub song_edits: SongEditRepository,
    /// The import_* bookkeeping tables.
    pub imports: ImportBookkeepingRepository,
}

impl Repositories {
    /// Builds the repository bundle over `db`.
    pub fn new(db: std::sync::Arc<Db>) -> Self {
        Self {
            songs: SongRepository::new(std::sync::Arc::clone(&db)),
            artists: ArtistRepository::new(std::sync::Arc::clone(&db)),
            albums: AlbumRepository::new(std::sync::Arc::clone(&db)),
            playlists: PlaylistRepository::new(std::sync::Arc::clone(&db)),
            formats: FormatRepository::new(std::sync::Arc::clone(&db)),
            lyrics: LyricsRepository::new(std::sync::Arc::clone(&db)),
            events: EventRepository::new(std::sync::Arc::clone(&db)),
            search_history: SearchHistoryRepository::new(std::sync::Arc::clone(&db)),
            radio: RadioRepository::new(std::sync::Arc::clone(&db)),
            recognition: RecognitionRepository::new(std::sync::Arc::clone(&db)),
            speed_dial: SpeedDialRepository::new(std::sync::Arc::clone(&db)),
            song_edits: SongEditRepository::new(std::sync::Arc::clone(&db)),
            imports: ImportBookkeepingRepository::new(db),
        }
    }

    /// The shared database handle (watch/subscribe/import access).
    pub fn db(&self) -> &Db {
        &self.songs.db
    }
}
