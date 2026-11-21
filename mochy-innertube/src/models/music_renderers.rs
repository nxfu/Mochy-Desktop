//! The shelf/list renderers: `models/MusicShelfRenderer.kt`,
//! `models/MusicResponsiveListItemRenderer.kt`,
//! `models/MusicTwoRowItemRenderer.kt`,
//! `models/MusicCarouselShelfRenderer.kt`, `models/MusicCardShelfRenderer.kt`,
//! `models/MusicQueueRenderer.kt`, `models/MusicPlaylistShelfRenderer.kt`,
//! `models/MusicDescriptionShelfRenderer.kt`.

use serde::{Deserialize, Serialize};

use crate::models::common::{Badges, Button, Continuation, ContinuationItemRenderer};
use crate::models::endpoint::NavigationEndpoint;
use crate::models::menu::Menu;
use crate::models::runs::Runs;
use crate::models::thumbnails::ThumbnailRenderer;

// ---------------------------------------------------------------------------
// MusicShelfRenderer.kt
// ---------------------------------------------------------------------------

/// `MusicShelfRenderer.kt:6-11`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MusicShelfRenderer {
    #[serde(default)]
    pub title: Option<Runs>,
    #[serde(default)]
    pub contents: Option<Vec<MusicShelfContent>>,
    #[serde(default)]
    pub continuations: Option<Vec<Continuation>>,
    #[serde(default)]
    pub bottom_endpoint: Option<NavigationEndpoint>,
    #[serde(default)]
    pub more_content_button: Option<Button>,
}

/// `MusicShelfRenderer.kt:13-17`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MusicShelfContent {
    #[serde(default)]
    pub music_responsive_list_item_renderer: Option<MusicResponsiveListItemRenderer>,
    #[serde(default)]
    pub continuation_item_renderer: Option<ContinuationItemRenderer>,
}

/// `List<MusicShelfRenderer.Content>.getItems()` (MusicShelfRenderer.kt:20-21).
pub fn shelf_items(contents: &[MusicShelfContent]) -> Vec<&MusicResponsiveListItemRenderer> {
    contents
        .iter()
        .filter_map(|content| content.music_responsive_list_item_renderer.as_ref())
        .collect()
}

/// `List<MusicShelfRenderer.Content>.getContinuation()`
/// (MusicShelfRenderer.kt:23-28) — the token carried by the shelf's
/// trailing continuation item.
pub fn shelf_continuation(contents: &[MusicShelfContent]) -> Option<String> {
    contents
        .iter()
        .find(|content| content.continuation_item_renderer.is_some())?
        .continuation_item_renderer
        .as_ref()?
        .continuation_endpoint
        .as_ref()?
        .continuation_command
        .as_ref()?
        .token
        .clone()
}

// ---------------------------------------------------------------------------
// MusicResponsiveListItemRenderer.kt
// ---------------------------------------------------------------------------

/// `MusicResponsiveListItemRenderer.kt:15-24`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MusicResponsiveListItemRenderer {
    #[serde(default)]
    pub badges: Option<Vec<Badges>>,
    #[serde(default)]
    pub fixed_columns: Option<Vec<FlexColumn>>,
    pub flex_columns: Vec<FlexColumn>,
    #[serde(default)]
    pub thumbnail: Option<ThumbnailRenderer>,
    #[serde(default)]
    pub menu: Option<Menu>,
    #[serde(default)]
    pub playlist_item_data: Option<PlaylistItemData>,
    #[serde(default)]
    pub overlay: Option<Overlay>,
    #[serde(default)]
    pub navigation_endpoint: Option<NavigationEndpoint>,
}

impl MusicResponsiveListItemRenderer {
    /// `isSong` (MusicResponsiveListItemRenderer.kt:25-26).
    pub fn is_song(&self) -> bool {
        match &self.navigation_endpoint {
            None => true,
            Some(endpoint) => {
                endpoint.watch_endpoint.is_some() || endpoint.watch_playlist_endpoint.is_some()
            }
        }
    }

    /// `isPlaylist` (MusicResponsiveListItemRenderer.kt:27-28).
    pub fn is_playlist(&self) -> bool {
        self.page_type_is(crate::models::endpoint::page_type::PLAYLIST)
    }

    /// `isAlbum` (MusicResponsiveListItemRenderer.kt:29-31) — audiobooks
    /// count.
    pub fn is_album(&self) -> bool {
        self.page_type_is(crate::models::endpoint::page_type::ALBUM)
            || self.page_type_is(crate::models::endpoint::page_type::AUDIOBOOK)
    }

    /// `isArtist` (MusicResponsiveListItemRenderer.kt:32-34) — a library
    /// artist is an artist too.
    pub fn is_artist(&self) -> bool {
        self.page_type_is(crate::models::endpoint::page_type::ARTIST)
            || self.page_type_is(crate::models::endpoint::page_type::LIBRARY_ARTIST)
    }

    fn page_type_is(&self, wanted: &str) -> bool {
        self.navigation_endpoint
            .as_ref()
            .and_then(|endpoint| endpoint.browse_endpoint.as_ref())
            .and_then(|browse| browse.browse_endpoint_context_supported_configs.as_ref())
            .map(|configs| {
                configs
                    .browse_endpoint_context_music_config
                    .page_type
                    .as_deref()
                    == Some(wanted)
            })
            .unwrap_or(false)
    }

    /// `musicVideoType` (MusicResponsiveListItemRenderer.kt:36-44) — the
    /// overlay's play button first, then the navigation endpoint.
    pub fn music_video_type(&self) -> Option<String> {
        self.overlay
            .as_ref()?
            .music_item_thumbnail_overlay_renderer
            .content
            .music_play_button_renderer
            .play_navigation_endpoint
            .as_ref()?
            .music_video_type()
            .map(str::to_owned)
            .or_else(|| {
                self.navigation_endpoint
                    .as_ref()
                    .and_then(|endpoint| endpoint.music_video_type().map(str::to_owned))
            })
    }
}

/// `MusicResponsiveListItemRenderer.FlexColumn`
/// (MusicResponsiveListItemRenderer.kt:46-55) — the fixed columns reuse the
/// same shape under `musicResponsiveListItemFixedColumnRenderer`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FlexColumn {
    #[serde(alias = "musicResponsiveListItemFixedColumnRenderer")]
    pub music_responsive_list_item_flex_column_renderer: MusicResponsiveListItemFlexColumnRenderer,
}

/// MusicResponsiveListItemRenderer.kt:52-54.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MusicResponsiveListItemFlexColumnRenderer {
    #[serde(default)]
    pub text: Option<Runs>,
}

/// MusicResponsiveListItemRenderer.kt:57-61.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistItemData {
    pub playlist_set_video_id: Option<String>,
    pub video_id: String,
}

/// MusicResponsiveListItemRenderer.kt:63-81.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Overlay {
    pub music_item_thumbnail_overlay_renderer: MusicItemThumbnailOverlayRenderer,
}

/// MusicResponsiveListItemRenderer.kt:67-70.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicItemThumbnailOverlayRenderer {
    pub content: OverlayContent,
}

/// MusicResponsiveListItemRenderer.kt:71-74.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayContent {
    pub music_play_button_renderer: MusicPlayButtonRenderer,
}

/// MusicResponsiveListItemRenderer.kt:75-78.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MusicPlayButtonRenderer {
    #[serde(default)]
    pub play_navigation_endpoint: Option<NavigationEndpoint>,
}

// ---------------------------------------------------------------------------
// MusicTwoRowItemRenderer.kt
// ---------------------------------------------------------------------------

/// `MusicTwoRowItemRenderer.kt:10-17`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicTwoRowItemRenderer {
    pub title: Runs,
    #[serde(default)]
    pub subtitle: Option<Runs>,
    #[serde(default)]
    pub subtitle_badges: Option<Vec<Badges>>,
    #[serde(default)]
    pub menu: Option<Menu>,
    pub thumbnail_renderer: ThumbnailRenderer,
    pub navigation_endpoint: NavigationEndpoint,
    #[serde(default)]
    pub thumbnail_overlay: Option<Overlay>,
}

impl MusicTwoRowItemRenderer {
    /// `isSong` (MusicTwoRowItemRenderer.kt:19-20) — the endpoint resolves
    /// to a watch.
    pub fn is_song(&self) -> bool {
        self.navigation_endpoint.any_watch_endpoint().is_some()
    }

    /// `isPlaylist` (MusicTwoRowItemRenderer.kt:21-27).
    pub fn is_playlist(&self) -> bool {
        self.page_type_is(crate::models::endpoint::page_type::PLAYLIST)
    }

    /// `isAlbum` (MusicTwoRowItemRenderer.kt:28-39).
    pub fn is_album(&self) -> bool {
        self.page_type_is(crate::models::endpoint::page_type::ALBUM)
            || self.page_type_is(crate::models::endpoint::page_type::AUDIOBOOK)
    }

    /// `isArtist` (MusicTwoRowItemRenderer.kt:40-46).
    pub fn is_artist(&self) -> bool {
        self.page_type_is(crate::models::endpoint::page_type::ARTIST)
    }

    fn page_type_is(&self, wanted: &str) -> bool {
        self.navigation_endpoint
            .browse_endpoint
            .as_ref()
            .and_then(|browse| browse.browse_endpoint_context_supported_configs.as_ref())
            .map(|configs| {
                configs
                    .browse_endpoint_context_music_config
                    .page_type
                    .as_deref()
                    == Some(wanted)
            })
            .unwrap_or(false)
    }

    /// `musicVideoType` (MusicTwoRowItemRenderer.kt:48-56).
    pub fn music_video_type(&self) -> Option<String> {
        self.thumbnail_overlay
            .as_ref()?
            .music_item_thumbnail_overlay_renderer
            .content
            .music_play_button_renderer
            .play_navigation_endpoint
            .as_ref()?
            .music_video_type()
            .map(str::to_owned)
            .or_else(|| {
                self.navigation_endpoint
                    .music_video_type()
                    .map(str::to_owned)
            })
    }
}

// ---------------------------------------------------------------------------
// MusicCarouselShelfRenderer.kt
// ---------------------------------------------------------------------------

/// `MusicCarouselShelfRenderer.kt:6-11`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicCarouselShelfRenderer {
    #[serde(default)]
    pub header: Option<CarouselHeader>,
    pub contents: Vec<CarouselContent>,
    pub item_size: Option<String>,
    #[serde(default)]
    pub num_items_per_column: Option<i64>,
}

/// `MusicCarouselShelfRenderer.kt:13-15`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CarouselHeader {
    pub music_carousel_shelf_basic_header_renderer: MusicCarouselShelfBasicHeaderRenderer,
}

/// `MusicCarouselShelfRenderer.kt:17-22`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicCarouselShelfBasicHeaderRenderer {
    #[serde(default)]
    pub strapline: Option<Runs>,
    pub title: Runs,
    #[serde(default)]
    pub thumbnail: Option<ThumbnailRenderer>,
    #[serde(default)]
    pub more_content_button: Option<Button>,
}

/// `MusicCarouselShelfRenderer.kt:25-30`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CarouselContent {
    #[serde(default)]
    pub music_two_row_item_renderer: Option<MusicTwoRowItemRenderer>,
    #[serde(default)]
    pub music_responsive_list_item_renderer: Option<MusicResponsiveListItemRenderer>,
    #[serde(default)]
    pub music_navigation_button_renderer:
        Option<crate::models::music_headers::MusicNavigationButtonRenderer>,
}

// ---------------------------------------------------------------------------
// MusicCardShelfRenderer.kt
// ---------------------------------------------------------------------------

/// `MusicCardShelfRenderer.kt:6-15`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicCardShelfRenderer {
    pub title: Runs,
    pub subtitle: Runs,
    pub thumbnail: ThumbnailRenderer,
    #[serde(default)]
    pub header: Option<CardHeader>,
    #[serde(default)]
    pub contents: Option<Vec<CardContent>>,
    pub buttons: Vec<crate::models::common::Button>,
    pub on_tap: NavigationEndpoint,
    #[serde(default)]
    pub subtitle_badges: Option<Vec<Badges>>,
}

/// `MusicCardShelfRenderer.kt:17-19`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CardHeader {
    pub music_card_shelf_header_basic_renderer: MusicCardShelfHeaderBasicRenderer,
}

/// `MusicCardShelfRenderer.kt:21-23`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicCardShelfHeaderBasicRenderer {
    pub title: Runs,
}

/// `MusicCardShelfRenderer.kt:26-29`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CardContent {
    #[serde(default)]
    pub music_responsive_list_item_renderer: Option<MusicResponsiveListItemRenderer>,
}

// ---------------------------------------------------------------------------
// MusicQueueRenderer.kt
// ---------------------------------------------------------------------------

/// `MusicQueueRenderer.kt:6-9`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MusicQueueRenderer {
    #[serde(default)]
    pub content: Option<QueueContent>,
    #[serde(default)]
    pub header: Option<QueueHeader>,
}

/// `MusicQueueRenderer.kt:11-13`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueContent {
    pub playlist_panel_renderer: crate::models::playlist_panel::PlaylistPanelRenderer,
}

/// `MusicQueueRenderer.kt:16-18`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct QueueHeader {
    #[serde(default)]
    pub music_queue_header_renderer: Option<MusicQueueHeaderRenderer>,
}

/// `MusicQueueRenderer.kt:20-23`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MusicQueueHeaderRenderer {
    #[serde(default)]
    pub title: Option<Runs>,
    #[serde(default)]
    pub subtitle: Option<Runs>,
}

// ---------------------------------------------------------------------------
// MusicPlaylistShelfRenderer.kt
// ---------------------------------------------------------------------------

/// `MusicPlaylistShelfRenderer.kt:6-11`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MusicPlaylistShelfRenderer {
    #[serde(default)]
    pub playlist_id: Option<String>,
    #[serde(default)]
    pub contents: Vec<MusicShelfContent>,
    #[serde(default)]
    pub collapsed_item_count: Option<i64>,
    #[serde(default)]
    pub continuations: Option<Vec<Continuation>>,
}

// ---------------------------------------------------------------------------
// MusicDescriptionShelfRenderer.kt
// ---------------------------------------------------------------------------

/// `MusicDescriptionShelfRenderer.kt:6-11`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicDescriptionShelfRenderer {
    #[serde(default)]
    pub header: Option<Runs>,
    #[serde(default)]
    pub subheader: Option<Runs>,
    pub description: Runs,
    #[serde(default)]
    pub footer: Option<Runs>,
}
