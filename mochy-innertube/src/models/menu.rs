//! `models/Menu.kt` — the overflow menu of a list item.

use serde::{Deserialize, Serialize};

use crate::models::common::Icon;
use crate::models::endpoint::{DefaultServiceEndpoint, NavigationEndpoint, ToggledServiceEndpoint};
use crate::models::runs::Runs;

/// `Menu.kt:6-8`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Menu {
    pub menu_renderer: MenuRenderer,
}

/// `Menu.kt:10-14`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MenuRenderer {
    #[serde(default)]
    pub items: Option<Vec<MenuItem>>,
    #[serde(default)]
    pub top_level_buttons: Option<Vec<TopLevelButton>>,
}

/// `Menu.MenuRenderer.Item` (`Menu.kt:15-19`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MenuItem {
    #[serde(default)]
    pub menu_navigation_item_renderer: Option<MenuNavigationItemRenderer>,
    #[serde(default)]
    pub menu_service_item_renderer: Option<MenuServiceItemRenderer>,
    #[serde(default)]
    pub toggle_menu_service_item_renderer: Option<ToggleMenuServiceRenderer>,
}

/// `Menu.kt:20-25`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MenuNavigationItemRenderer {
    pub text: Runs,
    pub icon: Icon,
    pub navigation_endpoint: NavigationEndpoint,
}

/// `Menu.kt:27-31`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MenuServiceItemRenderer {
    pub text: Runs,
    pub icon: Icon,
    pub service_endpoint: NavigationEndpoint,
}

/// `Menu.kt:33-38`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToggleMenuServiceRenderer {
    pub default_icon: Icon,
    pub default_service_endpoint: DefaultServiceEndpoint,
    #[serde(default)]
    pub toggled_service_endpoint: Option<ToggledServiceEndpoint>,
}

/// `Menu.kt:41-50`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TopLevelButton {
    #[serde(default)]
    pub button_renderer: Option<MenuButtonRenderer>,
}

/// `Menu.MenuRenderer.TopLevelButton.ButtonRenderer` (`Menu.kt:45-49`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MenuButtonRenderer {
    pub icon: Icon,
    pub navigation_endpoint: NavigationEndpoint,
}
