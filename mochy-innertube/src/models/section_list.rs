//! `models/SectionListRenderer.kt` (with its nested chip cloud) and
//! `models/ItemSectionRenderer` (top level in the same file).

use serde::{Deserialize, Serialize};

use crate::models::common::Continuation;
use crate::models::endpoint::NavigationEndpoint;
use crate::models::grid::GridRenderer;
use crate::models::music_headers::{
    MusicEditablePlaylistDetailHeaderRenderer, MusicResponsiveHeaderRenderer,
};
use crate::models::music_renderers::{
    MusicCardShelfRenderer, MusicCarouselShelfRenderer, MusicDescriptionShelfRenderer,
    MusicPlaylistShelfRenderer, MusicResponsiveListItemRenderer, MusicShelfRenderer,
};
use crate::models::runs::Runs;

/// `SectionListRenderer.kt:8-12`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SectionListRenderer {
    #[serde(default)]
    pub header: Option<Header>,
    #[serde(default)]
    pub contents: Option<Vec<Content>>,
    #[serde(default)]
    pub continuations: Option<Vec<Continuation>>,
}

/// `SectionListRenderer.kt:14-16`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Header {
    #[serde(default)]
    pub chip_cloud_renderer: Option<ChipCloudRenderer>,
}

/// `SectionListRenderer.kt:18-21`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChipCloudRenderer {
    #[serde(default)]
    pub chips: Vec<Chip>,
}

/// `SectionListRenderer.kt:23-26`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Chip {
    #[serde(default)]
    pub chip_cloud_chip_renderer: Option<ChipCloudChipRenderer>,
}

/// `SectionListRenderer.kt:28-33`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChipCloudChipRenderer {
    pub is_selected: bool,
    pub navigation_endpoint: NavigationEndpoint,
    #[serde(default)]
    pub on_deselected_command: Option<NavigationEndpoint>,
    #[serde(default)]
    pub text: Option<Runs>,
    #[serde(default)]
    pub unique_id: Option<String>,
}

/// `SectionListRenderer.Content` (`SectionListRenderer.kt:38-51`) —
/// `musicCarouselShelfRenderer` is `@JsonNames(
/// "musicImmersiveCarouselShelfRenderer")` upstream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Content {
    #[serde(default, alias = "musicImmersiveCarouselShelfRenderer")]
    pub music_carousel_shelf_renderer: Option<MusicCarouselShelfRenderer>,
    #[serde(default)]
    pub music_shelf_renderer: Option<MusicShelfRenderer>,
    #[serde(default)]
    pub music_card_shelf_renderer: Option<MusicCardShelfRenderer>,
    #[serde(default)]
    pub music_playlist_shelf_renderer: Option<MusicPlaylistShelfRenderer>,
    #[serde(default)]
    pub music_description_shelf_renderer: Option<MusicDescriptionShelfRenderer>,
    #[serde(default)]
    pub music_responsive_header_renderer: Option<MusicResponsiveHeaderRenderer>,
    #[serde(default)]
    pub music_editable_playlist_detail_header_renderer:
        Option<MusicEditablePlaylistDetailHeaderRenderer>,
    #[serde(default)]
    pub grid_renderer: Option<GridRenderer>,
    #[serde(default)]
    pub item_section_renderer: Option<ItemSectionRenderer>,
}

/// `SectionListRenderer.kt:55-62`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ItemSectionRenderer {
    #[serde(default)]
    pub contents: Option<Vec<ItemSectionContent>>,
}

/// `SectionListRenderer.kt:58-61`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ItemSectionContent {
    #[serde(default)]
    pub music_responsive_list_item_renderer: Option<MusicResponsiveListItemRenderer>,
}
