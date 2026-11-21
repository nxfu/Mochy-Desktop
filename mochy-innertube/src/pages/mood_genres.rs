//! `pages/MoodAndGenres.kt` (50 lines).

use crate::models::endpoint::BrowseEndpoint;
use crate::models::music_headers::MusicNavigationButtonRenderer;
use crate::models::section_list::Content as SectionContent;

/// `MoodAndGenres.kt:10-13`.
#[derive(Debug, Clone, PartialEq)]
pub struct MoodAndGenres {
    pub title: String,
    pub items: Vec<MoodAndGenresItem>,
}

/// `MoodAndGenres.Item` (MoodAndGenres.kt:15-19).
#[derive(Debug, Clone, PartialEq)]
pub struct MoodAndGenresItem {
    pub title: String,
    pub stripe_color: i64,
    pub endpoint: BrowseEndpoint,
}

pub struct MoodAndGenresParser;

impl MoodAndGenresParser {
    /// `fromSectionListRendererContent` (MoodAndGenres.kt:22-37).
    pub fn from_section_list_renderer_content(content: &SectionContent) -> Option<MoodAndGenres> {
        let grid = content.grid_renderer.as_ref()?;
        Some(MoodAndGenres {
            title: grid
                .header
                .as_ref()?
                .grid_header_renderer
                .title
                .first()?
                .text
                .clone(),
            items: grid
                .items
                .iter()
                .filter_map(|item| item.music_navigation_button_renderer.as_ref())
                .filter_map(Self::from_music_navigation_button_renderer)
                .collect(),
        })
    }

    /// `fromMusicNavigationButtonRenderer` (MoodAndGenres.kt:39-48).
    pub fn from_music_navigation_button_renderer(
        renderer: &MusicNavigationButtonRenderer,
    ) -> Option<MoodAndGenresItem> {
        Some(MoodAndGenresItem {
            title: renderer.button_text.first()?.text.clone(),
            stripe_color: renderer.solid.as_ref()?.left_stripe_color,
            endpoint: renderer.click_command.browse_endpoint.clone()?,
        })
    }
}
