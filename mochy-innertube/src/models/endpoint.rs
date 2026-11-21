//! `models/Endpoint.kt` + `models/NavigationEndpoint.kt` — the navigation
//! endpoints runs and renderers carry.

use serde::{Deserialize, Serialize};

/// `Endpoint.kt:13-37` — `WatchEndpoint`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct WatchEndpoint {
    pub video_id: Option<String>,
    pub playlist_id: Option<String>,
    pub playlist_set_video_id: Option<String>,
    pub params: Option<String>,
    pub index: Option<i32>,
    pub watch_endpoint_music_supported_configs: Option<WatchEndpointMusicSupportedConfigs>,
}

/// `Endpoint.kt:23-36`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchEndpointMusicSupportedConfigs {
    pub watch_endpoint_music_config: WatchEndpointMusicConfig,
}

/// `Endpoint.kt:27-35`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchEndpointMusicConfig {
    pub music_video_type: String,
}

/// `WatchEndpointMusicConfig` companion constants (`Endpoint.kt:30-34`).
pub mod music_video_type {
    pub const OMV: &str = "MUSIC_VIDEO_TYPE_OMV";
    pub const UGC: &str = "MUSIC_VIDEO_TYPE_UGC";
    pub const ATV: &str = "MUSIC_VIDEO_TYPE_ATV";
}

/// `Endpoint.kt:40-73` — `BrowseEndpoint`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BrowseEndpoint {
    pub browse_id: String,
    #[serde(default)]
    pub params: Option<String>,
    #[serde(default)]
    pub browse_endpoint_context_supported_configs: Option<BrowseEndpointContextSupportedConfigs>,
}

impl BrowseEndpoint {
    /// `isArtistEndpoint` (`Endpoint.kt:45-46`).
    pub fn is_artist_endpoint(&self) -> bool {
        self.page_type()
            .map(|t| t == page_type::ARTIST)
            .unwrap_or(false)
    }

    /// `isAlbumEndpoint` (`Endpoint.kt:47-49`) — an audiobook is an album
    /// page too.
    pub fn is_album_endpoint(&self) -> bool {
        self.page_type()
            .map(|t| t == page_type::ALBUM || t == page_type::AUDIOBOOK)
            .unwrap_or(false)
    }

    /// `isPlaylistEndpoint` (`Endpoint.kt:50-51`).
    pub fn is_playlist_endpoint(&self) -> bool {
        self.page_type()
            .map(|t| t == page_type::PLAYLIST)
            .unwrap_or(false)
    }

    fn page_type(&self) -> Option<&str> {
        self.browse_endpoint_context_supported_configs
            .as_ref()?
            .browse_endpoint_context_music_config
            .page_type
            .as_deref()
    }
}

/// `Endpoint.kt:53-72`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowseEndpointContextSupportedConfigs {
    pub browse_endpoint_context_music_config: BrowseEndpointContextMusicConfig,
}

/// `Endpoint.kt:58-70` — the `pageType` string plus its companion
/// constants.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowseEndpointContextMusicConfig {
    pub page_type: Option<String>,
}

/// `BrowseEndpointContextMusicConfig` companion constants
/// (`Endpoint.kt:61-69`).
pub mod page_type {
    pub const ALBUM: &str = "MUSIC_PAGE_TYPE_ALBUM";
    pub const AUDIOBOOK: &str = "MUSIC_PAGE_TYPE_AUDIOBOOK";
    pub const PLAYLIST: &str = "MUSIC_PAGE_TYPE_PLAYLIST";
    pub const ARTIST: &str = "MUSIC_PAGE_TYPE_ARTIST";
    pub const LIBRARY_ARTIST: &str = "MUSIC_PAGE_TYPE_LIBRARY_ARTIST";
    pub const USER_CHANNEL: &str = "MUSIC_PAGE_TYPE_USER_CHANNEL";
    pub const TRACK_LYRICS: &str = "MUSIC_PAGE_TYPE_TRACK_LYRICS";
    pub const TRACK_RELATED: &str = "MUSIC_PAGE_TYPE_TRACK_RELATED";
}

/// `Endpoint.kt:75-79`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchEndpoint {
    pub params: Option<String>,
    pub query: String,
}

/// `Endpoint.kt:81-84`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedbackEndpoint {
    pub feedback_token: String,
}

/// `Endpoint.kt:86-96`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct QueueAddEndpoint {
    pub queue_insert_position: Option<String>,
    pub queue_target: Option<QueueTarget>,
}

/// `Endpoint.kt:92-95`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct QueueTarget {
    pub video_id: Option<String>,
    pub playlist_id: Option<String>,
}

/// `Endpoint.kt:98-101`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareEntityEndpoint {
    pub serialized_share_entity: String,
}

/// `Endpoint.kt:103-113` — `DefaultServiceEndpoint` + its nested
/// `SubscribeEndpoint`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DefaultServiceEndpoint {
    #[serde(default)]
    pub subscribe_endpoint: Option<SubscribeEndpoint>,
    #[serde(default)]
    pub feedback_endpoint: Option<FeedbackEndpoint>,
}

/// `Endpoint.kt:109-112`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscribeEndpoint {
    pub channel_ids: Vec<String>,
    #[serde(default)]
    pub params: Option<String>,
}

/// `Endpoint.kt:115-118`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ToggledServiceEndpoint {
    #[serde(default)]
    pub feedback_endpoint: Option<FeedbackEndpoint>,
}

/// `models/NavigationEndpoint.kt:6-16`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct NavigationEndpoint {
    #[serde(default)]
    pub watch_endpoint: Option<WatchEndpoint>,
    #[serde(default)]
    pub watch_playlist_endpoint: Option<WatchEndpoint>,
    #[serde(default)]
    pub browse_endpoint: Option<BrowseEndpoint>,
    #[serde(default)]
    pub search_endpoint: Option<SearchEndpoint>,
    #[serde(default)]
    pub queue_add_endpoint: Option<QueueAddEndpoint>,
    #[serde(default)]
    pub share_entity_endpoint: Option<ShareEntityEndpoint>,
    #[serde(default)]
    pub feedback_endpoint: Option<FeedbackEndpoint>,
    #[serde(default)]
    pub url_endpoint: Option<UrlEndpoint>,
    #[serde(default)]
    pub continuation_command: Option<ContinuationCommand>,
}

/// `NavigationEndpoint.kt:17-20`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ContinuationCommand {
    pub token: Option<String>,
}

impl NavigationEndpoint {
    /// `anyWatchEndpoint` (`NavigationEndpoint.kt:31-34`).
    pub fn any_watch_endpoint(&self) -> Option<&WatchEndpoint> {
        self.watch_endpoint
            .as_ref()
            .or(self.watch_playlist_endpoint.as_ref())
    }

    /// `musicVideoType` (`NavigationEndpoint.kt:35-39`).
    pub fn music_video_type(&self) -> Option<&str> {
        Some(
            &self
                .any_watch_endpoint()?
                .watch_endpoint_music_supported_configs
                .as_ref()?
                .watch_endpoint_music_config
                .music_video_type,
        )
    }
}

/// `models/UrlEndpoint.kt`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UrlEndpoint {
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub target: Option<String>,
}
