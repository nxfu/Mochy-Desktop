//! `models/Tabs.kt`.

use serde::{Deserialize, Serialize};

use crate::models::endpoint::NavigationEndpoint;
use crate::models::music_renderers::MusicQueueRenderer;
use crate::models::section_list::SectionListRenderer;

/// `Tabs.kt:6-8`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Tabs {
    #[serde(default)]
    pub tabs: Vec<Tab>,
}

/// `Tabs.kt:10-12`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Tab {
    #[serde(default)]
    pub tab_renderer: Option<TabRenderer>,
}

/// `Tabs.kt:14-17`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TabRenderer {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub content: Option<TabContent>,
    #[serde(default)]
    pub endpoint: Option<NavigationEndpoint>,
}

/// `Tabs.kt:19-23`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TabContent {
    #[serde(default)]
    pub section_list_renderer: Option<SectionListRenderer>,
    #[serde(default)]
    pub music_queue_renderer: Option<MusicQueueRenderer>,
}

impl Tabs {
    /// `tabs.firstOrNull()?.tabRenderer` — the walk every caller starts
    /// with.
    pub fn first(&self) -> Option<&TabRenderer> {
        self.tabs.first().and_then(|tab| tab.tab_renderer.as_ref())
    }
}
