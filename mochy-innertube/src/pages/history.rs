//! `pages/HistoryPage.kt` (83 lines).

use crate::models::music_renderers::{
    MusicResponsiveListItemRenderer, MusicShelfContent, MusicShelfRenderer,
};
use crate::models::runs::{looks_like_timestamp, odd_elements, split_by_separator};
use crate::models::ytitem::{Album, Artist, SongItem};
use crate::pages::home::explicit_badge;
use crate::pages::page_helper::PageHelper;
use crate::utils::parse_time;

/// `HistoryPage.kt:14-16`.
#[derive(Debug, Clone, PartialEq)]
pub struct HistoryPage {
    pub sections: Option<Vec<HistorySection>>,
}

/// `HistoryPage.kt:17-20`.
#[derive(Debug, Clone, PartialEq)]
pub struct HistorySection {
    pub title: String,
    pub songs: Vec<SongItem>,
}

pub struct HistoryPageParser;

impl HistoryPageParser {
    /// `fromMusicShelfRenderer` (HistoryPage.kt:24-27) — null for a shelf
    /// with no title; one shelf like that used to fail the whole history.
    pub fn from_music_shelf_renderer(renderer: &MusicShelfRenderer) -> Option<HistorySection> {
        let title = renderer
            .title
            .as_ref()?
            .runs
            .as_ref()?
            .iter()
            .map(|run| run.text.as_str())
            .collect::<Vec<_>>()
            .join("");
        if title.trim().is_empty() {
            return None;
        }
        Some(HistorySection {
            title,
            songs: Self::songs_of(renderer.contents.as_deref().unwrap_or_default()),
        })
    }

    /// `songsOf` (HistoryPage.kt:29-30).
    pub fn songs_of(contents: &[MusicShelfContent]) -> Vec<SongItem> {
        crate::models::music_renderers::shelf_items(contents)
            .iter()
            .filter_map(|renderer| Self::from_music_responsive_list_item_renderer(renderer))
            .collect()
    }

    /// `fromMusicResponsiveListItemRenderer` (HistoryPage.kt:32-81) — a
    /// video in the history can carry its id only on its play button, and
    /// a missing cover is no reason to lose the play.
    fn from_music_responsive_list_item_renderer(
        renderer: &MusicResponsiveListItemRenderer,
    ) -> Option<SongItem> {
        let video_id = renderer
            .playlist_item_data
            .as_ref()
            .map(|data| data.video_id.clone())
            .or_else(|| {
                renderer
                    .overlay
                    .as_ref()?
                    .music_item_thumbnail_overlay_renderer
                    .content
                    .music_play_button_renderer
                    .play_navigation_endpoint
                    .as_ref()?
                    .watch_endpoint
                    .as_ref()?
                    .video_id
                    .clone()
            })
            .or_else(|| {
                renderer
                    .navigation_endpoint
                    .as_ref()?
                    .watch_endpoint
                    .as_ref()?
                    .video_id
                    .clone()
            })?;

        let library_tokens = PageHelper::extract_library_tokens_from_menu_items(
            renderer
                .menu
                .as_ref()
                .and_then(|menu| menu.menu_renderer.items.as_deref()),
        );

        let secondary_line_runs = renderer
            .flex_columns
            .get(1)
            .and_then(|column| {
                column
                    .music_responsive_list_item_flex_column_renderer
                    .text
                    .as_ref()
            })
            .and_then(|text| text.runs.as_ref())
            .map(|runs| split_by_separator(runs));

        Some(SongItem {
            id: video_id,
            title: renderer
                .flex_columns
                .first()?
                .music_responsive_list_item_flex_column_renderer
                .text
                .as_ref()?
                .first()?
                .text
                .clone(),
            artists: secondary_line_runs
                .as_ref()
                .and_then(|segments| segments.first())
                .map(|segment| {
                    odd_elements(segment)
                        .iter()
                        .map(|run| Artist {
                            name: run.text.clone(),
                            id: run
                                .navigation_endpoint
                                .as_ref()
                                .and_then(|endpoint| endpoint.browse_endpoint.as_ref())
                                .map(|browse| browse.browse_id.clone()),
                        })
                        .filter(|artist| !looks_like_timestamp(&artist.name))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default(),
            album: renderer
                .flex_columns
                .get(3)
                .and_then(|column| {
                    column
                        .music_responsive_list_item_flex_column_renderer
                        .text
                        .as_ref()
                })
                .and_then(|text| text.first())
                .and_then(|run| {
                    run.navigation_endpoint
                        .as_ref()
                        .and_then(|endpoint| endpoint.browse_endpoint.as_ref())
                        .map(|browse| Album {
                            name: run.text.clone(),
                            id: browse.browse_id.clone(),
                        })
                }),
            duration: renderer
                .fixed_columns
                .as_ref()
                .and_then(|columns| columns.first())
                .and_then(|column| {
                    column
                        .music_responsive_list_item_flex_column_renderer
                        .text
                        .as_ref()
                })
                .and_then(|text| text.first())
                .and_then(|run| parse_time(&run.text)),
            music_video_type: renderer.music_video_type(),
            thumbnail: renderer
                .thumbnail
                .as_ref()
                .and_then(|thumbnail| thumbnail.music_thumbnail_renderer.as_ref())
                .and_then(|music| music.get_thumbnail_url().map(str::to_owned))
                .unwrap_or_default(),
            explicit: explicit_badge(&renderer.badges),
            endpoint: renderer
                .overlay
                .as_ref()
                .and_then(|overlay| {
                    overlay
                        .music_item_thumbnail_overlay_renderer
                        .content
                        .music_play_button_renderer
                        .play_navigation_endpoint
                        .as_ref()
                })
                .and_then(|endpoint| endpoint.watch_endpoint.clone()),
            library_add_token: library_tokens.add_token,
            library_remove_token: library_tokens.remove_token,
            history_remove_token: renderer
                .menu
                .as_ref()
                .and_then(|menu| menu.menu_renderer.items.as_ref())
                .and_then(|items| {
                    items.iter().find_map(|item| {
                        let service = item.menu_service_item_renderer.as_ref()?;
                        (service.icon.icon_type == "REMOVE_FROM_HISTORY")
                            .then(|| {
                                service
                                    .service_endpoint
                                    .feedback_endpoint
                                    .as_ref()
                                    .map(|endpoint| endpoint.feedback_token.clone())
                            })
                            .flatten()
                    })
                }),
            ..Default::default()
        })
    }
}
