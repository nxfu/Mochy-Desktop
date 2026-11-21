//! `models/PlaylistPanelRenderer.kt`, `models/PlaylistPanelVideoRenderer.kt`,
//! `models/AutomixPreviewVideoRenderer.kt` — the watch-next queue panel.

use serde::{Deserialize, Serialize};

use crate::models::common::{Badges, Continuation};
use crate::models::endpoint::NavigationEndpoint;
use crate::models::menu::Menu;
use crate::models::runs::Runs;
use crate::models::thumbnails::Thumbnails;

/// `PlaylistPanelRenderer.kt:6-15`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistPanelRenderer {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub title_text: Option<Runs>,
    #[serde(default)]
    pub short_byline_text: Option<Runs>,
    #[serde(default)]
    pub contents: Vec<PlaylistPanelContent>,
    #[serde(default)]
    pub is_infinite: Option<bool>,
    #[serde(default)]
    pub num_items_to_show: Option<i64>,
    #[serde(default)]
    pub playlist_id: Option<String>,
    #[serde(default)]
    pub continuations: Option<Vec<Continuation>>,
}

/// `PlaylistPanelRenderer.kt:17-20`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistPanelContent {
    #[serde(default)]
    pub playlist_panel_video_renderer: Option<PlaylistPanelVideoRenderer>,
    #[serde(default)]
    pub automix_preview_video_renderer: Option<AutomixPreviewVideoRenderer>,
}

/// `PlaylistPanelVideoRenderer.kt:6-19`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistPanelVideoRenderer {
    #[serde(default)]
    pub title: Option<Runs>,
    #[serde(default)]
    pub length_text: Option<Runs>,
    #[serde(default)]
    pub long_byline_text: Option<Runs>,
    #[serde(default)]
    pub short_byline_text: Option<Runs>,
    #[serde(default)]
    pub badges: Option<Vec<Badges>>,
    #[serde(default)]
    pub video_id: Option<String>,
    #[serde(default)]
    pub playlist_set_video_id: Option<String>,
    pub selected: bool,
    pub thumbnail: Thumbnails,
    #[serde(default)]
    pub unplayable_text: Option<Runs>,
    #[serde(default)]
    pub menu: Option<Menu>,
    pub navigation_endpoint: NavigationEndpoint,
}

/// `AutomixPreviewVideoRenderer.kt`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomixPreviewVideoRenderer {
    pub content: AutomixContent,
}

/// `AutomixPreviewVideoRenderer.kt:6-9`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomixContent {
    pub automix_playlist_video_renderer: AutomixPlaylistVideoRenderer,
}

/// `AutomixPreviewVideoRenderer.kt:11-14`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomixPlaylistVideoRenderer {
    pub navigation_endpoint: NavigationEndpoint,
}
