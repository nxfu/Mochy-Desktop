//! `models/response/SearchResponse.kt` (33 lines).

use serde::{Deserialize, Serialize};

use crate::models::common::Continuation;
use crate::models::music_renderers::MusicResponsiveListItemRenderer;
use crate::models::tabs::Tabs;

/// `SearchResponse.kt:9-12`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SearchResponse {
    #[serde(default)]
    pub contents: Option<Contents>,
    #[serde(default)]
    pub continuation_contents: Option<ContinuationContents>,
}

/// `SearchResponse.kt:14-16`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Contents {
    #[serde(default)]
    pub tabbed_search_results_renderer: Option<Tabs>,
}

/// `SearchResponse.kt:18-32`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ContinuationContents {
    #[serde(default)]
    pub music_shelf_continuation: Option<MusicShelfContinuation>,
}

/// SearchResponse.kt:22-31.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MusicShelfContinuation {
    #[serde(default)]
    pub contents: Vec<Content>,
    #[serde(default)]
    pub continuations: Option<Vec<Continuation>>,
}

/// SearchResponse.kt:27-30.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Content {
    #[serde(default)]
    pub music_responsive_list_item_renderer: Option<MusicResponsiveListItemRenderer>,
}
