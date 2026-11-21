//! `pages/PlaylistPage.kt` (63 lines) +
//! `pages/PlaylistContinuationPage.kt`.

use crate::models::music_renderers::MusicResponsiveListItemRenderer;
use crate::models::runs::{looks_like_timestamp, odd_elements, split_by_separator};
use crate::models::ytitem::{Album, Artist, PlaylistItem, SongItem};
use crate::pages::home::explicit_badge;
use crate::pages::page_helper::PageHelper;
use crate::utils::parse_time;

/// `PlaylistPage.kt:14-20`.
#[derive(Debug, Clone, PartialEq)]
pub struct PlaylistPage {
    pub playlist: PlaylistItem,
    pub songs: Vec<SongItem>,
    pub songs_continuation: Option<String>,
    pub continuation: Option<String>,
    pub related: Option<Vec<crate::models::ytitem::YTItem>>,
}

/// `PlaylistContinuationPage.kt:5-8`.
#[derive(Debug, Clone, PartialEq)]
pub struct PlaylistContinuationPage {
    pub songs: Vec<SongItem>,
    pub continuation: Option<String>,
}

pub struct PlaylistPageParser;

impl PlaylistPageParser {
    /// `fromMusicResponsiveListItemRenderer` (PlaylistPage.kt:22-61) — a
    /// playlist song REQUIRES its setVideoId (the move/remove API needs
    /// it), so songs without one drop.
    pub fn from_music_responsive_list_item_renderer(
        renderer: &MusicResponsiveListItemRenderer,
    ) -> Option<SongItem> {
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
            id: renderer.playlist_item_data.as_ref()?.video_id.clone(),
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
                .get(2)
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
                .as_ref()?
                .music_thumbnail_renderer
                .as_ref()?
                .get_thumbnail_url()?
                .to_owned(),
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
            set_video_id: Some(
                renderer
                    .playlist_item_data
                    .as_ref()?
                    .playlist_set_video_id
                    .clone()?,
            ),
            library_add_token: library_tokens.add_token,
            library_remove_token: library_tokens.remove_token,
            ..Default::default()
        })
    }
}
