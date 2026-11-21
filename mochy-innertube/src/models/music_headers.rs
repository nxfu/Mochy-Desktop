//! The header renderers: `models/MusicEditablePlaylistDetailHeaderRenderer.kt`
//! (with `MusicDetailHeaderRenderer` + `MusicPlaylistEditHeaderRenderer`),
//! `models/MusicResponsiveHeaderRenderer.kt`,
//! `models/MusicMultiRowImageItemRenderer.kt`,
//! `models/MusicNavigationButtonRenderer.kt`.

use serde::{Deserialize, Serialize};

use crate::models::common::Badges;
use crate::models::endpoint::NavigationEndpoint;
use crate::models::menu::Menu;
use crate::models::runs::Runs;
use crate::models::thumbnails::ThumbnailRenderer;

/// `MusicEditablePlaylistDetailHeaderRenderer.kt:8-10`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicEditablePlaylistDetailHeaderRenderer {
    pub header: EditableHeader,
    pub edit_header: EditHeader,
}

/// `MusicEditablePlaylistDetailHeaderRenderer.kt:12-14`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditableHeader {
    #[serde(default)]
    pub music_detail_header_renderer: Option<MusicDetailHeaderRenderer>,
    #[serde(default)]
    pub music_responsive_header_renderer: Option<MusicResponsiveHeaderRenderer>,
}

/// `MusicEditablePlaylistDetailHeaderRenderer.kt:16-19`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct EditHeader {
    #[serde(default)]
    pub music_playlist_edit_header_renderer: Option<MusicPlaylistEditHeaderRenderer>,
}

/// `MusicEditablePlaylistDetailHeaderRenderer.kt:22-30`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicDetailHeaderRenderer {
    pub title: Runs,
    pub subtitle: Runs,
    pub second_subtitle: Runs,
    #[serde(default)]
    pub description: Option<Runs>,
    pub thumbnail: ThumbnailRenderer,
    pub menu: Menu,
}

/// `MusicEditablePlaylistDetailHeaderRenderer.kt:33-35`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MusicPlaylistEditHeaderRenderer {
    #[serde(default)]
    pub edit_title: Option<Runs>,
}

/// `MusicResponsiveHeaderRenderer.kt:6-14`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicResponsiveHeaderRenderer {
    #[serde(default)]
    pub thumbnail: Option<ThumbnailRenderer>,
    pub buttons: Vec<ResponsiveButton>,
    pub title: Runs,
    pub subtitle: Runs,
    #[serde(default)]
    pub second_subtitle: Option<Runs>,
    #[serde(default)]
    pub strapline_text_one: Option<Runs>,
    #[serde(default)]
    pub description: Option<Runs>,
}

/// `MusicResponsiveHeaderRenderer.kt:16-19` — the button holds a
/// `MenuRenderer` directly (not the `Menu` wrapper).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ResponsiveButton {
    #[serde(default)]
    pub music_play_button_renderer: Option<HeaderMusicPlayButtonRenderer>,
    #[serde(default)]
    pub menu_renderer: Option<crate::models::menu::MenuRenderer>,
}

/// `MusicResponsiveHeaderRenderer.kt:21-23`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct HeaderMusicPlayButtonRenderer {
    #[serde(default)]
    pub play_navigation_endpoint: Option<NavigationEndpoint>,
}

/// `MusicMultiRowImageItemRenderer.kt`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicMultiRowImageItemRenderer {
    pub title: Runs,
    pub subtitle: Runs,
    pub thumbnail: ThumbnailRenderer,
    pub on_tap: NavigationEndpoint,
}

/// `MusicNavigationButtonRenderer.kt`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicNavigationButtonRenderer {
    pub button_text: Runs,
    #[serde(default)]
    pub solid: Option<Solid>,
    #[serde(default)]
    pub icon_style: Option<IconStyle>,
    pub click_command: NavigationEndpoint,
}

/// `MusicNavigationButtonRenderer.kt:13-15` — the stripe color is a
/// 32-bit ARGB value the JSON carries as a number.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Solid {
    pub left_stripe_color: i64,
}

/// `MusicNavigationButtonRenderer.kt:17-20`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IconStyle {
    pub icon: crate::models::common::Icon,
}

/// Keeps unused imports honest when the badge type is only referenced by
/// dependent modules.
#[allow(dead_code)]
fn _badges_wired(_b: &Badges) {}
