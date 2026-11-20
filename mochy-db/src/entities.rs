//! Entity structs mirroring the 24 Room entities plus the relation wrappers
//! the DAO queries return (Song, Album, Artist, Playlist, PlaylistSong,
//! EventWithSong).
//!
//! Field semantics are identical to upstream `db/entities/*.kt`; timestamps
//! are epoch milliseconds (`Converters.kt` maps `LocalDateTime` <-> epoch ms
//! through `ZoneOffset.UTC`). Desktop convention (blueprint ch. 7.2): every
//! stored timestamp is TRUE UTC; the Android importer normalizes the
//! wall-clock-as-UTC values exactly once (see `importer::event_time`).

use rusqlite::Row;

fn get<T: rusqlite::types::FromSql>(row: &Row<'_>, col: &str) -> rusqlite::Result<T> {
    row.get(col)
}

/// `song` row — SongEntity.kt (26 columns at v45).
///
/// Tri-states preserved from upstream:
/// - `in_library: None` = not in library;
/// - `is_local` + `local_uri` + `genre`: `genre` is `None` until the file's
///   tag was read, `Some("")` once a tagless file was read.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SongEntity {
    pub id: String,
    pub title: String,
    pub duration: i32,
    pub thumbnail_url: Option<String>,
    pub album_id: Option<String>,
    pub album_name: Option<String>,
    pub explicit: bool,
    pub year: Option<i32>,
    /// Epoch ms (upstream `date`).
    pub date: Option<i64>,
    /// Epoch ms (upstream `dateModified`).
    pub date_modified: Option<i64>,
    pub liked: bool,
    /// Epoch ms (upstream `likedDate`).
    pub liked_date: Option<i64>,
    pub total_play_time: i64,
    /// Epoch ms; `None` = not in library.
    pub in_library: Option<i64>,
    /// Epoch ms (upstream `dateDownload`).
    pub date_download: Option<i64>,
    pub is_local: bool,
    pub library_add_token: Option<String>,
    pub library_remove_token: Option<String>,
    pub lyrics_offset: i32,
    pub romanize_lyrics: bool,
    pub is_downloaded: bool,
    pub is_uploaded: bool,
    pub is_video: bool,
    pub local_uri: Option<String>,
    pub added_by_like: bool,
    pub genre: Option<String>,
}

impl SongEntity {
    /// Builds the entity's column list shared by SELECT and INSERT.
    pub const COLUMNS: &'static [&'static str] = &[
        "id",
        "title",
        "duration",
        "thumbnailUrl",
        "albumId",
        "albumName",
        "explicit",
        "year",
        "date",
        "dateModified",
        "liked",
        "likedDate",
        "totalPlayTime",
        "inLibrary",
        "dateDownload",
        "isLocal",
        "libraryAddToken",
        "libraryRemoveToken",
        "lyricsOffset",
        "romanizeLyrics",
        "isDownloaded",
        "isUploaded",
        "isVideo",
        "localUri",
        "addedByLike",
        "genre",
    ];

    pub fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: get(row, "id")?,
            title: get(row, "title")?,
            duration: get(row, "duration")?,
            thumbnail_url: get(row, "thumbnailUrl")?,
            album_id: get(row, "albumId")?,
            album_name: get(row, "albumName")?,
            explicit: get(row, "explicit")?,
            year: get(row, "year")?,
            date: get(row, "date")?,
            date_modified: get(row, "dateModified")?,
            liked: get(row, "liked")?,
            liked_date: get(row, "likedDate")?,
            total_play_time: get(row, "totalPlayTime")?,
            in_library: get(row, "inLibrary")?,
            date_download: get(row, "dateDownload")?,
            is_local: get(row, "isLocal")?,
            library_add_token: get(row, "libraryAddToken")?,
            library_remove_token: get(row, "libraryRemoveToken")?,
            lyrics_offset: get(row, "lyricsOffset")?,
            romanize_lyrics: get(row, "romanizeLyrics")?,
            is_downloaded: get(row, "isDownloaded")?,
            is_uploaded: get(row, "isUploaded")?,
            is_video: get(row, "isVideo")?,
            local_uri: get(row, "localUri")?,
            added_by_like: get(row, "addedByLike")?,
            genre: get(row, "genre")?,
        })
    }
}

/// `artist` row — ArtistEntity.kt.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ArtistEntity {
    pub id: String,
    pub name: String,
    pub thumbnail_url: Option<String>,
    pub channel_id: Option<String>,
    /// Epoch ms.
    pub last_update_time: i64,
    /// Epoch ms; `Some` = bookmarked ("liked artist" upstream).
    pub bookmarked_at: Option<i64>,
    pub is_local: bool,
}

impl ArtistEntity {
    pub const COLUMNS: &'static [&'static str] = &[
        "id",
        "name",
        "thumbnailUrl",
        "channelId",
        "lastUpdateTime",
        "bookmarkedAt",
        "isLocal",
    ];

    pub fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: get(row, "id")?,
            name: get(row, "name")?,
            thumbnail_url: get(row, "thumbnailUrl")?,
            channel_id: get(row, "channelId")?,
            last_update_time: get(row, "lastUpdateTime")?,
            bookmarked_at: get(row, "bookmarkedAt")?,
            is_local: get(row, "isLocal")?,
        })
    }

    /// `ArtistEntity.isYouTubeArtist` (ArtistEntity.kt:41-43).
    pub fn is_youtube_artist(&self) -> bool {
        self.id.starts_with("UC")
            || self
                .id
                .starts_with("FEmusic_library_privately_owned_artist")
    }

    /// `ArtistEntity.isPrivatelyOwnedArtist` (ArtistEntity.kt:45-47).
    pub fn is_privately_owned_artist(&self) -> bool {
        self.id
            .starts_with("FEmusic_library_privately_owned_artist")
    }
}

/// `album` row — AlbumEntity.kt. `playlist_id` is the YouTube album playlist.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AlbumEntity {
    pub id: String,
    pub playlist_id: Option<String>,
    pub title: String,
    pub year: Option<i32>,
    pub thumbnail_url: Option<String>,
    pub theme_color: Option<i32>,
    pub song_count: i32,
    pub duration: i32,
    pub explicit: bool,
    /// Epoch ms.
    pub last_update_time: i64,
    /// Epoch ms; `Some` = bookmarked.
    pub bookmarked_at: Option<i64>,
    /// Epoch ms.
    pub liked_date: Option<i64>,
    /// Epoch ms; `None` = not in library.
    pub in_library: Option<i64>,
    pub description: Option<String>,
    pub is_local: bool,
    pub is_uploaded: bool,
}

impl AlbumEntity {
    pub const COLUMNS: &'static [&'static str] = &[
        "id",
        "playlistId",
        "title",
        "year",
        "thumbnailUrl",
        "themeColor",
        "songCount",
        "duration",
        "explicit",
        "lastUpdateTime",
        "bookmarkedAt",
        "likedDate",
        "inLibrary",
        "description",
        "isLocal",
        "isUploaded",
    ];

    pub fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: get(row, "id")?,
            playlist_id: get(row, "playlistId")?,
            title: get(row, "title")?,
            year: get(row, "year")?,
            thumbnail_url: get(row, "thumbnailUrl")?,
            theme_color: get(row, "themeColor")?,
            song_count: get(row, "songCount")?,
            duration: get(row, "duration")?,
            explicit: get(row, "explicit")?,
            last_update_time: get(row, "lastUpdateTime")?,
            bookmarked_at: get(row, "bookmarkedAt")?,
            liked_date: get(row, "likedDate")?,
            in_library: get(row, "inLibrary")?,
            description: get(row, "description")?,
            is_local: get(row, "isLocal")?,
            is_uploaded: get(row, "isUploaded")?,
        })
    }
}

/// `playlist` row — PlaylistEntity.kt. `browse_id` is the remote (YT) id.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlaylistEntity {
    pub id: String,
    pub name: String,
    pub browse_id: Option<String>,
    /// Epoch ms.
    pub created_at: Option<i64>,
    /// Epoch ms.
    pub last_update_time: Option<i64>,
    pub is_editable: bool,
    /// Epoch ms; `Some` = pinned.
    pub bookmarked_at: Option<i64>,
    pub remote_song_count: Option<i32>,
    pub play_endpoint_params: Option<String>,
    pub thumbnail_url: Option<String>,
    pub shuffle_endpoint_params: Option<String>,
    pub radio_endpoint_params: Option<String>,
    pub is_local: bool,
    pub is_auto_sync: bool,
    /// Free text; local only, never sent to YouTube.
    pub description: Option<String>,
}

impl PlaylistEntity {
    pub const COLUMNS: &'static [&'static str] = &[
        "id",
        "name",
        "browseId",
        "createdAt",
        "lastUpdateTime",
        "isEditable",
        "bookmarkedAt",
        "remoteSongCount",
        "playEndpointParams",
        "thumbnailUrl",
        "shuffleEndpointParams",
        "radioEndpointParams",
        "isLocal",
        "isAutoSync",
        "description",
    ];

    pub fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: get(row, "id")?,
            name: get(row, "name")?,
            browse_id: get(row, "browseId")?,
            created_at: get(row, "createdAt")?,
            last_update_time: get(row, "lastUpdateTime")?,
            is_editable: get(row, "isEditable")?,
            bookmarked_at: get(row, "bookmarkedAt")?,
            remote_song_count: get(row, "remoteSongCount")?,
            play_endpoint_params: get(row, "playEndpointParams")?,
            thumbnail_url: get(row, "thumbnailUrl")?,
            shuffle_endpoint_params: get(row, "shuffleEndpointParams")?,
            radio_endpoint_params: get(row, "radioEndpointParams")?,
            is_local: get(row, "isLocal")?,
            is_auto_sync: get(row, "isAutoSync")?,
            description: get(row, "description")?,
        })
    }
}

/// `song_artist_map` row — SongArtistMap.kt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SongArtistMap {
    pub song_id: String,
    pub artist_id: String,
    pub position: i32,
}

/// `song_album_map` row — SongAlbumMap.kt (`index` is the disc/track index).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SongAlbumMap {
    pub song_id: String,
    pub album_id: String,
    pub index: i32,
}

/// `album_artist_map` row — AlbumArtistMap.kt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlbumArtistMap {
    pub album_id: String,
    pub artist_id: String,
    pub order: i32,
}

/// `playlist_song_map` row — PlaylistSongMap.kt.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlaylistSongMap {
    pub id: i64,
    pub playlist_id: String,
    pub song_id: String,
    pub position: i32,
    /// Rides along for YouTube playlist moves (radio/automix continuity).
    pub set_video_id: Option<String>,
}

/// `search_history` row — SearchHistory.kt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHistory {
    pub id: i64,
    pub query: String,
}

/// `format` row — FormatEntity.kt (stream format cache per song).
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FormatEntity {
    pub id: String,
    pub itag: i32,
    pub mime_type: String,
    pub codecs: String,
    pub bitrate: i32,
    pub sample_rate: Option<i32>,
    pub content_length: i64,
    pub loudness_db: Option<f64>,
    pub perceptual_loudness_db: Option<f64>,
    /// Deprecated upstream ("retrieve from a fresh player request"); kept
    /// because Android imports carry it.
    pub playback_url: Option<String>,
}

impl FormatEntity {
    pub fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: get(row, "id")?,
            itag: get(row, "itag")?,
            mime_type: get(row, "mimeType")?,
            codecs: get(row, "codecs")?,
            bitrate: get(row, "bitrate")?,
            sample_rate: get(row, "sampleRate")?,
            content_length: get(row, "contentLength")?,
            loudness_db: get(row, "loudnessDb")?,
            perceptual_loudness_db: get(row, "perceptualLoudnessDb")?,
            playback_url: get(row, "playbackUrl")?,
        })
    }
}

/// `lyrics` row — LyricsEntity.kt.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LyricsEntity {
    pub id: String,
    /// Raw lyrics text, or the sentinel [`Self::LYRICS_NOT_FOUND`].
    pub lyrics: String,
    pub provider: String,
    pub translated_lyrics: String,
    pub translation_language: String,
    pub translation_mode: String,
}

impl LyricsEntity {
    /// Songs without any lyrics found store this string instead of a NULL
    /// row, so the provider fan-out knows a lookup already failed
    /// (LyricsEntity.kt companion).
    pub const LYRICS_NOT_FOUND: &'static str = "LYRICS_NOT_FOUND";

    pub fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: get(row, "id")?,
            lyrics: get(row, "lyrics")?,
            provider: get(row, "provider")?,
            translated_lyrics: get(row, "translatedLyrics")?,
            translation_language: get(row, "translationLanguage")?,
            translation_mode: get(row, "translationMode")?,
        })
    }
}

/// `event` row — Event.kt (one play that passed the history threshold).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Event {
    pub id: i64,
    pub song_id: String,
    /// Epoch ms. Desktop stores true UTC; Android stored local wall clock
    /// as UTC ms (normalized once, on import).
    pub timestamp: i64,
    pub play_time: i64,
}

/// `playlist_event` row — PlaylistEvent.kt (playlist plays for recap).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlaylistEvent {
    pub id: i64,
    pub playlist_id: String,
    /// Epoch ms (same convention as `Event::timestamp`).
    pub timestamp: i64,
}

/// `playCount` row — PlayCountEntity.kt (monthly aggregate).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayCountEntity {
    pub song: String,
    pub year: i32,
    pub month: i32,
    pub count: i32,
}

/// `related_song_map` row — RelatedSongMap.kt (radio adjacency cache).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelatedSongMap {
    pub id: i64,
    pub song_id: String,
    pub related_song_id: String,
}

/// `set_video_id` row — SetVideoIdEntity.kt (queue ids for YT moves).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetVideoIdEntity {
    pub video_id: String,
    pub set_video_id: Option<String>,
}

/// `recognition_history` row — RecognitionHistory.kt (Shazam results).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RecognitionHistory {
    pub id: i64,
    pub track_id: String,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub cover_art_url: Option<String>,
    pub cover_art_hq_url: Option<String>,
    pub genre: Option<String>,
    pub release_date: Option<String>,
    pub label: Option<String>,
    pub shazam_url: Option<String>,
    pub apple_music_url: Option<String>,
    pub spotify_url: Option<String>,
    pub isrc: Option<String>,
    pub youtube_video_id: Option<String>,
    /// Epoch ms.
    pub recognized_at: i64,
    pub liked: bool,
}

impl RecognitionHistory {
    pub fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: get(row, "id")?,
            track_id: get(row, "trackId")?,
            title: get(row, "title")?,
            artist: get(row, "artist")?,
            album: get(row, "album")?,
            cover_art_url: get(row, "coverArtUrl")?,
            cover_art_hq_url: get(row, "coverArtHqUrl")?,
            genre: get(row, "genre")?,
            release_date: get(row, "releaseDate")?,
            label: get(row, "label")?,
            shazam_url: get(row, "shazamUrl")?,
            apple_music_url: get(row, "appleMusicUrl")?,
            spotify_url: get(row, "spotifyUrl")?,
            isrc: get(row, "isrc")?,
            youtube_video_id: get(row, "youtubeVideoId")?,
            recognized_at: get(row, "recognizedAt")?,
            liked: get(row, "liked")?,
        })
    }
}

/// `speed_dial_item` row — SpeedDialItem.kt (the pinned 3x3 grid).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpeedDialItem {
    pub id: String,
    pub secondary_id: Option<String>,
    pub title: String,
    pub subtitle: Option<String>,
    pub thumbnail_url: Option<String>,
    /// `SONG` / `ALBUM` / `ARTIST` / `PLAYLIST`.
    pub item_type: String,
    pub explicit: bool,
    /// TRUE UTC already upstream (`System.currentTimeMillis()`), so the
    /// importer does NOT shift it.
    pub create_date: i64,
}

/// `imported_song` row — ImportedSong.kt. No FK by design.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedSong {
    pub song_id: String,
    /// An `ImportSource.key`.
    pub source: String,
    /// Epoch ms.
    pub imported_at: i64,
}

/// `import_session` row — ImportSession.kt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportSession {
    pub id: i64,
    pub source: String,
    /// Epoch ms.
    pub started_at: i64,
}

impl ImportSession {
    /// The source of a session that imported audio files from the device.
    pub const DEVICE_SOURCE: &'static str = "device";
}

/// `import_session_song` row — ImportSessionSong.kt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportSessionSong {
    pub session_id: i64,
    pub song_id: String,
    pub added_to_library: bool,
    pub liked: bool,
}

/// `imported_playlist` row — ImportedPlaylist.kt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedPlaylist {
    pub playlist_id: String,
    pub source: String,
    /// `None` for playlists YouTube sync brought in (no session owns them).
    pub session_id: Option<i64>,
    /// Epoch ms.
    pub imported_at: i64,
    pub remote_id: Option<String>,
    pub remote_url: Option<String>,
    /// Epoch ms.
    pub last_synced_at: Option<i64>,
    pub author_name: Option<String>,
    pub author_avatar_url: Option<String>,
    pub author_url: Option<String>,
}

/// `imported_playlist_track` row — ImportedPlaylistTrack.kt (resync seen-set).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedPlaylistTrack {
    pub playlist_id: String,
    pub track_key: String,
}

/// `song_edit` row — SongEdit.kt (hand-edit overlay).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SongEdit {
    pub song_id: String,
    pub title: Option<String>,
    /// JSON list of `{"id","name","thumbnail"}` (EditedArtist).
    pub artists: Option<String>,
    pub album: Option<String>,
    pub year: Option<i32>,
    /// Provider cover link or `file://` picture.
    pub cover: Option<String>,
    /// The video picked in "select video".
    pub video_id: Option<String>,
    /// "none" in select video: this song never shows a video.
    pub no_video: bool,
}

impl SongEdit {
    pub fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            song_id: get(row, "songId")?,
            title: get(row, "title")?,
            artists: get(row, "artists")?,
            album: get(row, "album")?,
            year: get(row, "year")?,
            cover: get(row, "cover")?,
            video_id: get(row, "videoId")?,
            no_video: get(row, "noVideo")?,
        })
    }
}

/// Relation wrapper — `entities/Song.kt`: song + sorted artists + album +
/// format.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Song {
    pub song: SongEntity,
    /// Ordered by `song_artist_map.position` (the consuming query inlines
    /// the `sorted_song_artist_map` view's ORDER BY).
    pub artists: Vec<ArtistEntity>,
    pub album: Option<AlbumEntity>,
    pub format: Option<FormatEntity>,
}

impl Song {
    /// Upstream `Song.id`.
    pub fn id(&self) -> &str {
        &self.song.id
    }
    /// Upstream `Song.title`.
    pub fn title(&self) -> &str {
        &self.song.title
    }
    /// Upstream `Song.thumbnailUrl` (falls back to `""` like LocalItem use).
    pub fn thumbnail_url(&self) -> Option<&str> {
        self.song.thumbnail_url.as_deref()
    }
}

/// Relation wrapper — `entities/Album.kt`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Album {
    pub album: AlbumEntity,
    pub artists: Vec<ArtistEntity>,
    pub song_count_listened: Option<i32>,
    pub time_listened: Option<i64>,
}

/// Relation wrapper — `entities/Artist.kt`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Artist {
    pub artist: ArtistEntity,
    pub song_count: i32,
    pub time_listened: Option<i64>,
}

/// Relation wrapper — `entities/Playlist.kt`: playlist + song count + the
/// first-3 thumbnails (the `playlist_song_map_preview` view).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Playlist {
    pub playlist: PlaylistEntity,
    pub song_count: i32,
    /// `None` entries for songs without artwork, positional order.
    pub song_thumbnails: Vec<Option<String>>,
}

/// Relation wrapper — `entities/PlaylistSong.kt`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlaylistSong {
    pub map: PlaylistSongMap,
    pub song: Song,
}

/// Relation wrapper — `entities/EventWithSong.kt`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EventWithSong {
    pub event: Event,
    pub song: Song,
}
