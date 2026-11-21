//! `models/GridRenderer.kt`.

use serde::{Deserialize, Serialize};

use crate::models::common::Continuation;
use crate::models::music_headers::MusicNavigationButtonRenderer;
use crate::models::music_renderers::MusicTwoRowItemRenderer;
use crate::models::runs::Runs;

/// `GridRenderer.kt:6-10`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GridRenderer {
    #[serde(default)]
    pub header: Option<GridHeader>,
    #[serde(default)]
    pub items: Vec<GridItem>,
    #[serde(default)]
    pub continuations: Option<Vec<Continuation>>,
}

/// `GridRenderer.kt:12-15`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GridHeader {
    pub grid_header_renderer: GridHeaderRenderer,
}

/// `GridRenderer.kt:17-20`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GridHeaderRenderer {
    pub title: Runs,
}

/// `GridRenderer.kt:22-25`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GridItem {
    #[serde(default)]
    pub music_navigation_button_renderer: Option<MusicNavigationButtonRenderer>,
    #[serde(default)]
    pub music_two_row_item_renderer: Option<MusicTwoRowItemRenderer>,
}
