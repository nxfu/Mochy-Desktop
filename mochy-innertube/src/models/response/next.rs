//! `models/response/NextResponse.kt` (62 lines).

use serde::{Deserialize, Serialize};

use crate::models::endpoint::NavigationEndpoint;
use crate::models::playlist_panel::PlaylistPanelRenderer;
use crate::models::tabs::Tab;
use crate::models::youtube_data_page;

/// `NextResponse.kt:10-15`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct NextResponse {
    #[serde(default)]
    pub contents: Option<Contents>,
    #[serde(default)]
    pub engagement_panels: Option<Vec<EngagementPanel>>,
    #[serde(default)]
    pub continuation_contents: Option<ContinuationContents>,
    #[serde(default)]
    pub current_video_endpoint: Option<NavigationEndpoint>,
}

/// `NextResponse.kt:17-35` — the engagement-panel wrapper the comments
/// token walk reads (kept even though the comment stack itself is dead:
/// `getMediaInfo` shares the two-column walk through `contents`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct EngagementPanel {
    #[serde(default)]
    pub engagement_panel_section_list_renderer: Option<EngagementPanelSectionListRenderer>,
}

/// NextResponse.kt:20-24.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct EngagementPanelSectionListRenderer {
    #[serde(default)]
    pub panel_identifier: Option<String>,
    #[serde(default)]
    pub content: Option<PanelContent>,
}

/// NextResponse.kt:25-33.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PanelContent {
    #[serde(default)]
    pub section_list_renderer: Option<PanelSectionListRenderer>,
}

/// NextResponse.kt:29-32.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PanelSectionListRenderer {
    #[serde(default)]
    pub contents: Option<Vec<Option<youtube_data_page::Content>>>,
}

/// `NextResponse.Contents` (NextResponse.kt:37-56).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Contents {
    #[serde(default)]
    pub single_column_music_watch_next_results_renderer:
        Option<SingleColumnMusicWatchNextResultsRenderer>,
    #[serde(default)]
    pub two_column_watch_next_results: Option<youtube_data_page::TwoColumnWatchNextResults>,
}

/// NextResponse.kt:42-44.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SingleColumnMusicWatchNextResultsRenderer {
    #[serde(default)]
    pub tabbed_renderer: Option<TabbedRenderer>,
}

/// NextResponse.kt:46-48.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TabbedRenderer {
    #[serde(default)]
    pub watch_next_tabbed_results_renderer: Option<WatchNextTabbedResultsRenderer>,
}

/// NextResponse.kt:50-54.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct WatchNextTabbedResultsRenderer {
    #[serde(default)]
    pub tabs: Vec<Tab>,
}

/// NextResponse.kt:58-61.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ContinuationContents {
    #[serde(default)]
    pub playlist_panel_continuation: Option<PlaylistPanelRenderer>,
}
