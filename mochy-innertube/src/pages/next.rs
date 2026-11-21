//! `pages/NextPage.kt` (75 lines) — the watch-next queue panel parser.

use crate::models::endpoint::{BrowseEndpoint, WatchEndpoint};
use crate::models::playlist_panel::PlaylistPanelVideoRenderer;
use crate::models::runs::{looks_like_timestamp, odd_elements, split_by_separator};
use crate::models::ytitem::{Album, Artist, SongItem};
use crate::pages::home::explicit_badge;
use crate::pages::page_helper::PageHelper;
use crate::utils::parse_time;

/// `NextPage.kt:14-22`.
#[derive(Debug, Clone, PartialEq)]
pub struct NextResult {
    pub title: Option<String>,
    pub items: Vec<SongItem>,
    pub current_index: Option<usize>,
    pub lyrics_endpoint: Option<BrowseEndpoint>,
    pub related_endpoint: Option<BrowseEndpoint>,
    pub continuation: Option<String>,
    pub endpoint: WatchEndpoint,
}

pub struct NextPage;

impl NextPage {
    /// `fromPlaylistPanelVideoRenderer` (NextPage.kt:25-74).
    pub fn from_playlist_panel_video_renderer(
        renderer: &PlaylistPanelVideoRenderer,
    ) -> Option<SongItem> {
        let long_by_line_runs = renderer
            .long_byline_text
            .as_ref()
            .or(renderer.short_byline_text.as_ref())?
            .runs
            .as_ref()
            .map(|runs| split_by_separator(runs))?;

        let library_tokens = PageHelper::extract_library_tokens_from_menu_items(
            renderer
                .menu
                .as_ref()
                .and_then(|menu| menu.menu_renderer.items.as_deref()),
        );

        Some(SongItem {
            id: renderer.video_id.clone()?,
            title: renderer.title.as_ref()?.first()?.text.clone(),
            artists: long_by_line_runs.first().map(|segment| {
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
            })?,
            album: long_by_line_runs.get(1).and_then(|segment| {
                segment.first().and_then(|run| {
                    run.navigation_endpoint
                        .as_ref()
                        .and_then(|endpoint| endpoint.browse_endpoint.as_ref())
                        .map(|browse| Album {
                            name: run.text.clone(),
                            id: browse.browse_id.clone(),
                        })
                })
            }),
            duration: renderer
                .length_text
                .as_ref()
                .and_then(|text| text.first())
                .and_then(|run| parse_time(&run.text)),
            music_video_type: renderer
                .navigation_endpoint
                .music_video_type()
                .map(str::to_owned),
            thumbnail: renderer.thumbnail.thumbnails.last()?.url.clone(),
            explicit: explicit_badge(&renderer.badges),
            library_add_token: library_tokens.add_token,
            library_remove_token: library_tokens.remove_token,
            ..Default::default()
        })
    }
}
