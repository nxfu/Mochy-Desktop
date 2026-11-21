//! `models/response/BrowseResponse.kt` (154 lines).

use serde::{Deserialize, Serialize};

use crate::models::common::{Continuation, ResponseContext, SubscriptionButton};
use crate::models::grid::GridRenderer;
use crate::models::menu::Menu;
use crate::models::music_headers::{
    MusicDetailHeaderRenderer, MusicEditablePlaylistDetailHeaderRenderer,
    MusicResponsiveHeaderRenderer,
};
use crate::models::music_renderers::{
    MusicPlaylistShelfRenderer, MusicShelfContent, MusicShelfRenderer,
};
use crate::models::runs::Runs;
use crate::models::section_list::{Content as SectionContent, SectionListRenderer};
use crate::models::tabs::Tabs;
use crate::models::thumbnails::{ThumbnailRenderer, Thumbnails};

/// `BrowseResponse.kt:18-27`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BrowseResponse {
    #[serde(default)]
    pub contents: Option<Contents>,
    #[serde(default)]
    pub continuation_contents: Option<ContinuationContents>,
    #[serde(default)]
    pub on_response_received_actions: Option<Vec<ResponseAction>>,
    #[serde(default)]
    pub header: Option<Header>,
    #[serde(default)]
    pub microformat: Option<Microformat>,
    #[serde(default)]
    pub response_context: Option<ResponseContext>,
    #[serde(default)]
    pub background: Option<ThumbnailRenderer>,
}

/// `BrowseResponse.kt:29-33`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Contents {
    #[serde(default)]
    pub single_column_browse_results_renderer: Option<Tabs>,
    #[serde(default)]
    pub section_list_renderer: Option<SectionListRenderer>,
    #[serde(default)]
    pub two_column_browse_results_renderer: Option<TwoColumnBrowseResultsRenderer>,
}

/// `BrowseResponse.kt:35-39` — the response-scoped two-column renderer
/// (distinct from `models/TwoColumnBrowseResultsRenderer.kt`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TwoColumnBrowseResultsRenderer {
    #[serde(default)]
    pub tabs: Option<Vec<Option<crate::models::tabs::Tab>>>,
    #[serde(default)]
    pub secondary_contents: Option<SecondaryContents>,
}

/// `BrowseResponse.kt:40-43`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SecondaryContents {
    #[serde(default)]
    pub section_list_renderer: Option<SectionListRenderer>,
}

/// `BrowseResponse.kt:45-69`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ContinuationContents {
    #[serde(default)]
    pub section_list_continuation: Option<SectionListContinuation>,
    #[serde(default)]
    pub music_playlist_shelf_continuation: Option<MusicPlaylistShelfContinuation>,
    #[serde(default)]
    pub grid_continuation: Option<GridContinuation>,
    #[serde(default)]
    pub music_shelf_continuation: Option<MusicShelfRenderer>,
}

/// `BrowseResponse.kt:52-56`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SectionListContinuation {
    #[serde(default)]
    pub contents: Vec<SectionContent>,
    #[serde(default)]
    pub continuations: Option<Vec<Continuation>>,
}

/// `BrowseResponse.kt:58-62`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MusicPlaylistShelfContinuation {
    #[serde(default)]
    pub contents: Vec<MusicShelfContent>,
    #[serde(default)]
    pub continuations: Option<Vec<Continuation>>,
}

/// `BrowseResponse.kt:64-68`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GridContinuation {
    #[serde(default)]
    pub items: Vec<crate::models::grid::GridItem>,
    #[serde(default)]
    pub continuations: Option<Vec<Continuation>>,
}

/// `BrowseResponse.kt:71-79`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ResponseAction {
    #[serde(default)]
    pub append_continuation_items_action: Option<ContinuationItems>,
}

/// `BrowseResponse.kt:75-78`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ContinuationItems {
    #[serde(default)]
    pub continuation_items: Option<Vec<MusicShelfContent>>,
}

/// `BrowseResponse.kt:81-143`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Header {
    #[serde(default)]
    pub music_immersive_header_renderer: Option<MusicImmersiveHeaderRenderer>,
    #[serde(default)]
    pub music_detail_header_renderer: Option<MusicDetailHeaderRenderer>,
    #[serde(default)]
    pub music_editable_playlist_detail_header_renderer:
        Option<MusicEditablePlaylistDetailHeaderRenderer>,
    #[serde(default)]
    pub music_visual_header_renderer: Option<MusicVisualHeaderRenderer>,
    #[serde(default)]
    pub music_header_renderer: Option<MusicHeaderRenderer>,
}

/// `BrowseResponse.kt:90-110`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicImmersiveHeaderRenderer {
    pub title: Runs,
    #[serde(default)]
    pub description: Option<Runs>,
    #[serde(default)]
    pub thumbnail: Option<ThumbnailRenderer>,
    #[serde(default)]
    pub play_button: Option<crate::models::common::Button>,
    #[serde(default)]
    pub start_radio_button: Option<crate::models::common::Button>,
    #[serde(default)]
    pub subscription_button: Option<SubscriptionButton>,
    pub menu: Menu,
    #[serde(default)]
    pub subscription_button2: Option<SubscriptionButton2>,
    #[serde(default)]
    pub monthly_listener_count: Option<Runs>,
}

/// `BrowseResponse.kt:101-109`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionButton2 {
    #[serde(default)]
    pub subscribe_button_renderer: Option<SubscribeButtonRenderer2>,
}

/// `BrowseResponse.kt:104-108`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SubscribeButtonRenderer2 {
    #[serde(default)]
    pub subscriber_count_with_subscribe_text: Option<Runs>,
}

/// `BrowseResponse.kt:112-117`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicVisualHeaderRenderer {
    pub title: Runs,
    pub foreground_thumbnail: ThumbnailRenderer,
    #[serde(default)]
    pub thumbnail: Option<ThumbnailRenderer>,
}

/// `BrowseResponse.kt:119-122`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Buttons {
    #[serde(default)]
    pub menu_renderer: Option<Menu>,
}

/// `BrowseResponse.kt:124-133`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MusicHeaderRenderer {
    #[serde(default)]
    pub buttons: Option<Vec<Buttons>>,
    #[serde(default)]
    pub title: Option<Runs>,
    #[serde(default)]
    pub thumbnail: Option<HeaderMusicThumbnailRenderer>,
    #[serde(default)]
    pub subtitle: Option<Runs>,
    #[serde(default)]
    pub second_subtitle: Option<Runs>,
    #[serde(default)]
    pub strapline_text_one: Option<Runs>,
    #[serde(default)]
    pub strapline_thumbnail: Option<HeaderMusicThumbnailRenderer>,
}

/// `BrowseResponse.kt:138-142`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HeaderMusicThumbnailRenderer {
    pub music_thumbnail_renderer: crate::models::thumbnails::MusicThumbnailRenderer,
    #[serde(default)]
    pub thumbnails: Option<Vec<MusicThumbnail>>,
}

/// `BrowseResponse.kt:134-137`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicThumbnail {
    #[serde(default)]
    pub url: Option<String>,
}

/// `BrowseResponse.kt:145-153`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Microformat {
    #[serde(default)]
    pub microformat_data_renderer: Option<MicroformatDataRenderer>,
}

/// `BrowseResponse.kt:149-152`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MicroformatDataRenderer {
    #[serde(default)]
    pub url_canonical: Option<String>,
}

// keeps the unused-import guard honest (MusicPlaylistShelfRenderer is
// referenced by dependent modules through this module's re-exports)
#[allow(dead_code)]
fn _wired(
    _a: &MusicPlaylistShelfRenderer,
    _b: &MusicShelfRenderer,
    _c: &Thumbnails,
    _d: &MusicResponsiveHeaderRenderer,
    _e: &GridRenderer,
) {
}
