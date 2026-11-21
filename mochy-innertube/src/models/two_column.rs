//! `models/TwoColumnBrowseResultsRenderer.kt`.

use serde::{Deserialize, Serialize};

use crate::models::common::Continuation;
use crate::models::music_renderers::MusicShelfRenderer;

/// `TwoColumnBrowseResultsRenderer.kt:6-9`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TwoColumnBrowseResultsRenderer {
    #[serde(default)]
    pub secondary_contents: Option<SecondaryContents>,
    #[serde(default)]
    pub tabs: Option<Vec<crate::models::tabs::Tab>>,
}

/// `TwoColumnBrowseResultsRenderer.kt:11-13`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SecondaryContents {
    #[serde(default)]
    pub section_list_renderer: Option<SecondarySectionListRenderer>,
}

/// `TwoColumnBrowseResultsRenderer.kt:16-19`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SecondarySectionListRenderer {
    #[serde(default)]
    pub contents: Option<Vec<SecondaryContent>>,
    #[serde(default)]
    pub continuations: Option<Vec<Continuation>>,
}

/// `TwoColumnBrowseResultsRenderer.kt:21-24`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SecondaryContent {
    #[serde(default)]
    pub music_playlist_shelf_renderer:
        Option<crate::models::music_renderers::MusicPlaylistShelfRenderer>,
    #[serde(default)]
    pub music_shelf_renderer: Option<MusicShelfRenderer>,
}
