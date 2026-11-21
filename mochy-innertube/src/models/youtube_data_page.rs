//! `models/YouTubeDataPage.kt` — the WEB watch-next page shape the
//! comments/getMediaInfo walks read (`twoColumnWatchNextResults`).
//!
//! Upstream nests six levels of data classes; the port keeps the same
//! JSON names but flattens the type names.

use serde::{Deserialize, Serialize};

use crate::models::endpoint::NavigationEndpoint;
use crate::models::runs::Run;
use crate::models::thumbnails::Thumbnail;

/// `YouTubeDataPage.kt:7-9` (`contents`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct YouTubeDataPage {
    #[serde(default)]
    pub contents: Option<YdpContents>,
}

/// `YouTubeDataPage.Contents` (`twoColumnWatchNextResults`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct YdpContents {
    #[serde(default)]
    pub two_column_watch_next_results: Option<TwoColumnWatchNextResults>,
}

/// `TwoColumnWatchNextResults` (`results`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TwoColumnWatchNextResults {
    #[serde(default)]
    pub results: Option<Results>,
}

/// `Results` (`results` again).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Results {
    #[serde(default)]
    pub results: Option<ResultsInner>,
}

/// `Results.Results` (`contents`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ResultsInner {
    #[serde(default)]
    pub content: Option<Vec<Option<Content>>>,
}

/// `Results.Results.Content` (YouTubeDataPage.kt:32-40).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Content {
    #[serde(default)]
    pub video_primary_info_renderer: Option<VideoPrimaryInfoRenderer>,
    #[serde(default)]
    pub video_secondary_info_renderer: Option<VideoSecondaryInfoRenderer>,
    #[serde(default)]
    pub item_section_renderer: Option<ContentItemSectionRenderer>,
    #[serde(default)]
    pub continuation_item_renderer: Option<ContentContinuationItemRenderer>,
}

/// `Content.ItemSectionRenderer` (YouTubeDataPage.kt:43-47).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ContentItemSectionRenderer {
    #[serde(default)]
    pub contents: Option<Vec<Option<ContentItemSectionContent>>>,
    #[serde(default)]
    pub section_identifier: Option<String>,
}

/// `Content.ItemSectionRenderer.Content` (YouTubeDataPage.kt:50-52).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ContentItemSectionContent {
    #[serde(default)]
    pub continuation_item_renderer: Option<ContentContinuationItemRenderer>,
}

/// `Content.ItemSectionRenderer.Content.ContinuationItemRenderer`
/// (YouTubeDataPage.kt:54-59).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ContentContinuationItemRenderer {
    #[serde(default)]
    pub trigger: Option<String>,
    #[serde(default)]
    pub continuation_endpoint: Option<ContentContinuationEndpoint>,
}

/// YouTubeDataPage.kt:61-75.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ContentContinuationEndpoint {
    #[serde(default)]
    pub click_tracking_params: Option<String>,
    #[serde(default)]
    pub continuation_command: Option<ContentContinuationCommand>,
}

/// YouTubeDataPage.kt:68-74.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ContentContinuationCommand {
    pub token: Option<String>,
    pub request: Option<String>,
}

/// `Content.VideoPrimaryInfoRenderer` (YouTubeDataPage.kt:81-124).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct VideoPrimaryInfoRenderer {
    #[serde(default)]
    pub title: Option<PrimaryTitle>,
    #[serde(default)]
    pub view_count: Option<PrimaryViewCount>,
    #[serde(default)]
    pub date_text: Option<PrimaryDateText>,
}

/// YouTubeDataPage.kt:89-99.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PrimaryTitle {
    #[serde(default)]
    pub runs: Option<Vec<PrimaryRun>>,
}

/// YouTubeDataPage.kt:94-98 — the run carries only text here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PrimaryRun {
    pub text: Option<String>,
}

/// YouTubeDataPage.kt:101-117.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PrimaryViewCount {
    #[serde(default)]
    pub video_view_count_renderer: Option<VideoViewCountRenderer>,
}

/// YouTubeDataPage.kt:106-110.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct VideoViewCountRenderer {
    #[serde(default)]
    pub view_count: Option<ViewCountSimpleText>,
}

/// YouTubeDataPage.kt:112-115.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ViewCountSimpleText {
    #[serde(default)]
    pub simple_text: Option<String>,
}

/// YouTubeDataPage.kt:119-123.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PrimaryDateText {
    #[serde(default)]
    pub simple_text: Option<String>,
}

/// `Content.VideoSecondaryInfoRenderer` (YouTubeDataPage.kt:127-180).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct VideoSecondaryInfoRenderer {
    #[serde(default)]
    pub owner: Option<Owner>,
    #[serde(default)]
    pub attributed_description: Option<AttributedDescription>,
}

/// YouTubeDataPage.kt:133-137.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AttributedDescription {
    #[serde(default)]
    pub content: Option<String>,
}

/// YouTubeDataPage.kt:139-143.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Owner {
    #[serde(default)]
    pub video_owner_renderer: Option<VideoOwnerRenderer>,
}

/// YouTubeDataPage.kt:144-178.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct VideoOwnerRenderer {
    #[serde(default)]
    pub thumbnail: Option<OwnerThumbnail>,
    #[serde(default)]
    pub subscriber_count_text: Option<SubscriberCountText>,
    #[serde(default)]
    pub title: Option<OwnerTitle>,
    #[serde(default)]
    pub navigation_endpoint: Option<NavigationEndpoint>,
}

/// YouTubeDataPage.kt:155-159 — a bare `thumbnails` list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct OwnerThumbnail {
    #[serde(default)]
    pub thumbnails: Option<Vec<Thumbnail>>,
}

/// YouTubeDataPage.kt:161-165.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SubscriberCountText {
    #[serde(default)]
    pub simple_text: Option<String>,
}

/// YouTubeDataPage.kt:167-177.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct OwnerTitle {
    #[serde(default)]
    pub runs: Option<Vec<PrimaryRun>>,
}

/// A text-only run alias kept for symmetry with upstream's nested shapes.
#[allow(dead_code)]
pub type TextRun = Run;
