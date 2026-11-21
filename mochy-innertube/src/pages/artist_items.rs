//! `pages/ArtistItemsPage.kt` (130 lines) +
//! `pages/ArtistItemsContinuationPage.kt`.

use crate::models::music_renderers::{MusicResponsiveListItemRenderer, MusicTwoRowItemRenderer};
use crate::models::runs::{looks_like_timestamp, odd_elements, split_by_separator};
use crate::models::ytitem::{AlbumItem, Artist, PlaylistItem, SongItem, YTItem};
use crate::pages::home::{explicit_badge, find_radio_watch_endpoint, find_shuffle_watch_endpoint};
use crate::pages::page_helper::PageHelper;
use crate::utils::parse_time;

/// `ArtistItemsPage.kt:16-20`.
#[derive(Debug, Clone, PartialEq)]
pub struct ArtistItemsPage {
    pub title: String,
    pub items: Vec<YTItem>,
    pub continuation: Option<String>,
}

/// `ArtistItemsContinuationPage.kt:5-8`.
#[derive(Debug, Clone, PartialEq)]
pub struct ArtistItemsContinuationPage {
    pub items: Vec<YTItem>,
    pub continuation: Option<String>,
}

pub struct ArtistItemsPageParser;

impl ArtistItemsPageParser {
    /// `fromMusicResponsiveListItemRenderer` (ArtistItemsPage.kt:22-71).
    pub fn from_music_responsive_list_item_renderer(
        renderer: &MusicResponsiveListItemRenderer,
    ) -> Option<SongItem> {
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

        let artists = secondary_line_runs.as_ref()?.first().map(|segment| {
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
        });

        let album = renderer
            .flex_columns
            .last()
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
                    .map(|browse| crate::models::ytitem::Album {
                        name: run.text.clone(),
                        id: browse.browse_id.clone(),
                    })
            });

        let library_tokens = PageHelper::extract_library_tokens_from_menu_items(
            renderer
                .menu
                .as_ref()
                .and_then(|menu| menu.menu_renderer.items.as_deref()),
        );

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
            artists: artists?,
            album,
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
            library_add_token: library_tokens.add_token,
            library_remove_token: library_tokens.remove_token,
            ..Default::default()
        })
    }

    /// `fromMusicTwoRowItemRenderer` (ArtistItemsPage.kt:73-128) — album
    /// first, then song, then playlist (the artist branch is absent
    /// here, unlike the home/artist converters).
    pub fn from_music_two_row_item_renderer(renderer: &MusicTwoRowItemRenderer) -> Option<YTItem> {
        if renderer.is_album() {
            Some(YTItem::Album(AlbumItem {
                browse_id: renderer
                    .navigation_endpoint
                    .browse_endpoint
                    .as_ref()?
                    .browse_id
                    .clone(),
                playlist_id: renderer
                    .thumbnail_overlay
                    .as_ref()?
                    .music_item_thumbnail_overlay_renderer
                    .content
                    .music_play_button_renderer
                    .play_navigation_endpoint
                    .as_ref()?
                    .any_watch_endpoint()?
                    .playlist_id
                    .clone()?,
                title: renderer.title.first()?.text.clone(),
                artists: None,
                year: renderer
                    .subtitle
                    .as_ref()?
                    .last()
                    .and_then(|run| run.text.parse().ok()),
                thumbnail: renderer
                    .thumbnail_renderer
                    .music_thumbnail_renderer
                    .as_ref()?
                    .get_thumbnail_url()?
                    .to_owned(),
                explicit: explicit_badge(&renderer.subtitle_badges),
                ..Default::default()
            }))
        } else if renderer.is_song() {
            Some(YTItem::Song(SongItem {
                id: renderer
                    .navigation_endpoint
                    .watch_endpoint
                    .as_ref()?
                    .video_id
                    .clone()?,
                title: renderer.title.first()?.text.clone(),
                artists: odd_elements(
                    split_by_separator(renderer.subtitle.as_ref()?.runs.as_ref()?).first()?,
                )
                .iter()
                .map(|run| Artist {
                    name: run.text.clone(),
                    id: run
                        .navigation_endpoint
                        .as_ref()
                        .and_then(|endpoint| endpoint.browse_endpoint.as_ref())
                        .map(|browse| browse.browse_id.clone()),
                })
                .collect(),
                album: None,
                duration: None,
                music_video_type: renderer.music_video_type(),
                thumbnail: renderer
                    .thumbnail_renderer
                    .music_thumbnail_renderer
                    .as_ref()?
                    .get_thumbnail_url()?
                    .to_owned(),
                endpoint: renderer.navigation_endpoint.watch_endpoint.clone(),
                ..Default::default()
            }))
        } else if renderer.is_playlist() {
            let browse_id = renderer
                .navigation_endpoint
                .browse_endpoint
                .as_ref()?
                .browse_id
                .clone();
            Some(YTItem::Playlist(PlaylistItem {
                id: browse_id
                    .strip_prefix("VL")
                    .map(str::to_owned)
                    .unwrap_or(browse_id),
                title: renderer.title.first()?.text.clone(),
                author: renderer.subtitle.as_ref()?.first().map(run_artist_id),
                song_count_text: renderer
                    .subtitle
                    .as_ref()?
                    .runs
                    .as_ref()?
                    .get(4)
                    .map(|run| run.text.clone()),
                thumbnail: Some(
                    renderer
                        .thumbnail_renderer
                        .music_thumbnail_renderer
                        .as_ref()?
                        .get_thumbnail_url()?
                        .to_owned(),
                ),
                play_endpoint: renderer
                    .thumbnail_overlay
                    .as_ref()?
                    .music_item_thumbnail_overlay_renderer
                    .content
                    .music_play_button_renderer
                    .play_navigation_endpoint
                    .as_ref()?
                    .watch_playlist_endpoint
                    .clone(),
                shuffle_endpoint: find_shuffle_watch_endpoint(renderer),
                radio_endpoint: find_radio_watch_endpoint(renderer),
                is_editable: false,
            }))
        } else {
            None
        }
    }
}

/// The artist run mapping with the browse id (ArtistItemsPage.kt:107-112).
fn run_artist_id(run: &crate::models::runs::Run) -> Artist {
    Artist {
        name: run.text.clone(),
        id: run
            .navigation_endpoint
            .as_ref()
            .and_then(|endpoint| endpoint.browse_endpoint.as_ref())
            .map(|browse| browse.browse_id.clone()),
    }
}
