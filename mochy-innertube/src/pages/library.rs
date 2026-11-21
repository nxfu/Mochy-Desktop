//! `pages/LibraryPage.kt` (191 lines) +
//! `pages/LibraryContinuationPage.kt`.
//!
//! Upstream carries `[UPLOAD_DEBUG]` println noise through this parser
//! (debug-build spam); the port drops the prints and keeps the behavior —
//! including the empty-string fallbacks for missing artist/album ids,
//! which are real behavior (LibraryPage.kt:104-125).

use crate::models::music_renderers::{MusicResponsiveListItemRenderer, MusicTwoRowItemRenderer};
use crate::models::runs::{Run, odd_elements};
use crate::models::ytitem::{Album, AlbumItem, Artist, ArtistItem, PlaylistItem, SongItem, YTItem};
use crate::pages::home::{explicit_badge, find_radio_watch_endpoint, find_shuffle_watch_endpoint};
use crate::pages::page_helper::PageHelper;
use crate::utils::parse_time;

/// `LibraryPage.kt:16-19`.
#[derive(Debug, Clone, PartialEq)]
pub struct LibraryPage {
    pub items: Vec<YTItem>,
    pub continuation: Option<String>,
}

/// `LibraryContinuationPage.kt:5-8`.
#[derive(Debug, Clone, PartialEq)]
pub struct LibraryContinuationPage {
    pub items: Vec<YTItem>,
    pub continuation: Option<String>,
}

pub struct LibraryPageParser;

impl LibraryPageParser {
    /// `fromMusicTwoRowItemRenderer` (LibraryPage.kt:21-78).
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
                    .watch_playlist_endpoint
                    .as_ref()?
                    .playlist_id
                    .clone()?,
                title: renderer.title.first()?.text.clone(),
                artists: Some(parse_artists(
                    renderer
                        .subtitle
                        .as_ref()
                        .and_then(|runs| runs.runs.as_deref()),
                )),
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
                author: renderer.subtitle.as_ref()?.first().map(|run| Artist {
                    name: run.text.clone(),
                    id: run
                        .navigation_endpoint
                        .as_ref()
                        .and_then(|endpoint| endpoint.browse_endpoint.as_ref())
                        .map(|browse| browse.browse_id.clone()),
                }),
                song_count_text: renderer
                    .subtitle
                    .as_ref()?
                    .last()
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
                is_editable: renderer
                    .menu
                    .as_ref()?
                    .menu_renderer
                    .items
                    .as_ref()?
                    .iter()
                    .any(|item| {
                        item.menu_navigation_item_renderer
                            .as_ref()
                            .map(|navigation| navigation.icon.icon_type == "EDIT")
                            .unwrap_or(false)
                    }),
            }))
        } else if renderer.is_artist() {
            Some(YTItem::Artist(ArtistItem {
                id: renderer
                    .navigation_endpoint
                    .browse_endpoint
                    .as_ref()?
                    .browse_id
                    .clone(),
                title: renderer.title.last()?.text.clone(),
                thumbnail: Some(
                    renderer
                        .thumbnail_renderer
                        .music_thumbnail_renderer
                        .as_ref()?
                        .get_thumbnail_url()?
                        .to_owned(),
                ),
                shuffle_endpoint: find_shuffle_watch_endpoint(renderer),
                radio_endpoint: find_radio_watch_endpoint(renderer),
                ..Default::default()
            }))
        } else {
            None
        }
    }

    /// `fromMusicResponsiveListItemRenderer` (LibraryPage.kt:80-171) —
    /// the library songs walk, with the empty-string id fallbacks.
    pub fn from_music_responsive_list_item_renderer(
        renderer: &MusicResponsiveListItemRenderer,
    ) -> Option<YTItem> {
        let library_tokens = PageHelper::extract_library_tokens_from_menu_items(
            renderer
                .menu
                .as_ref()
                .and_then(|menu| menu.menu_renderer.items.as_deref()),
        );

        if renderer.is_song() {
            let video_id = renderer.playlist_item_data.as_ref()?.video_id.clone();
            let title = renderer
                .flex_columns
                .first()?
                .music_responsive_list_item_flex_column_renderer
                .text
                .as_ref()?
                .first()?
                .text
                .clone();

            let artist_runs = renderer
                .flex_columns
                .get(1)
                .and_then(|column| {
                    column
                        .music_responsive_list_item_flex_column_renderer
                        .text
                        .as_ref()
                })
                .and_then(|text| text.runs.as_ref())
                .map(|runs| odd_elements(runs));

            let artists = artist_runs
                .map(|runs| {
                    runs.iter()
                        .map(|run| {
                            // "has no browseId, using empty string"
                            // (LibraryPage.kt:106-108) — an empty id, not
                            // a missing one
                            let browse_id = run
                                .navigation_endpoint
                                .as_ref()
                                .and_then(|endpoint| endpoint.browse_endpoint.as_ref())
                                .map(|browse| browse.browse_id.clone())
                                .unwrap_or_default();
                            Artist {
                                name: run.text.clone(),
                                id: Some(browse_id),
                            }
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();

            let album = renderer
                .flex_columns
                .get(2)
                .and_then(|column| {
                    column
                        .music_responsive_list_item_flex_column_renderer
                        .text
                        .as_ref()
                })
                .and_then(|text| text.first())
                .map(|run| {
                    let browse_id = run
                        .navigation_endpoint
                        .as_ref()
                        .and_then(|endpoint| endpoint.browse_endpoint.as_ref())
                        .map(|browse| browse.browse_id.clone())
                        .unwrap_or_default();
                    Album {
                        name: run.text.clone(),
                        id: browse_id,
                    }
                });

            let thumbnail_url = renderer
                .thumbnail
                .as_ref()?
                .music_thumbnail_renderer
                .as_ref()?
                .get_thumbnail_url()?
                .to_owned();

            Some(YTItem::Song(SongItem {
                id: video_id,
                title,
                artists,
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
                thumbnail: thumbnail_url,
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
            }))
        } else if renderer.is_artist() {
            Some(YTItem::Artist(ArtistItem {
                id: renderer
                    .navigation_endpoint
                    .as_ref()?
                    .browse_endpoint
                    .as_ref()?
                    .browse_id
                    .clone(),
                title: renderer
                    .flex_columns
                    .first()?
                    .music_responsive_list_item_flex_column_renderer
                    .text
                    .as_ref()?
                    .first()?
                    .text
                    .clone(),
                thumbnail: Some(
                    renderer
                        .thumbnail
                        .as_ref()?
                        .music_thumbnail_renderer
                        .as_ref()?
                        .get_thumbnail_url()?
                        .to_owned(),
                ),
                shuffle_endpoint: renderer
                    .menu
                    .as_ref()
                    .and_then(|menu| menu.menu_renderer.items.as_deref())
                    .and_then(|items| {
                        items.iter().find_map(|item| {
                            let navigation = item.menu_navigation_item_renderer.as_ref()?;
                            (navigation.icon.icon_type == "MUSIC_SHUFFLE").then(|| {
                                navigation
                                    .navigation_endpoint
                                    .watch_playlist_endpoint
                                    .clone()
                            })?
                        })
                    }),
                radio_endpoint: renderer
                    .menu
                    .as_ref()
                    .and_then(|menu| menu.menu_renderer.items.as_deref())
                    .and_then(|items| {
                        items.iter().find_map(|item| {
                            let navigation = item.menu_navigation_item_renderer.as_ref()?;
                            (navigation.icon.icon_type == "MIX").then(|| {
                                navigation
                                    .navigation_endpoint
                                    .watch_playlist_endpoint
                                    .clone()
                            })?
                        })
                    }),
                ..Default::default()
            }))
        } else {
            None
        }
    }
}

/// `parseArtists` (LibraryPage.kt:173-189) — only linked runs count.
fn parse_artists(runs: Option<&[Run]>) -> Vec<Artist> {
    runs.unwrap_or_default()
        .iter()
        .filter(|run| run.navigation_endpoint.is_some())
        .map(|run| Artist {
            id: run
                .navigation_endpoint
                .as_ref()
                .and_then(|endpoint| endpoint.browse_endpoint.as_ref())
                .map(|browse| browse.browse_id.clone()),
            name: run.text.clone(),
        })
        .collect()
}
