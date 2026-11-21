//! `pages/SearchPage.kt` (201 lines).

use crate::models::music_renderers::MusicResponsiveListItemRenderer;
use crate::models::runs::{odd_elements, split_by_separator};
use crate::models::ytitem::{Album, AlbumItem, Artist, ArtistItem, PlaylistItem, SongItem, YTItem};
use crate::pages::home::explicit_badge;
use crate::pages::page_helper::PageHelper;
use crate::utils::parse_time;

/// `SearchPage.kt:15-18`.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchResult {
    pub items: Vec<YTItem>,
    pub continuation: Option<String>,
}

pub struct SearchPage;

impl SearchPage {
    /// `toYTItem` (SearchPage.kt:21-200).
    pub fn to_yt_item(renderer: &MusicResponsiveListItemRenderer) -> Option<YTItem> {
        let secondary_line = split_by_separator(
            renderer
                .flex_columns
                .get(1)?
                .music_responsive_list_item_flex_column_renderer
                .text
                .as_ref()?
                .runs
                .as_ref()?,
        );

        if renderer.is_song() {
            let library_tokens = PageHelper::extract_library_tokens_from_menu_items(
                renderer
                    .menu
                    .as_ref()
                    .and_then(|menu| menu.menu_renderer.items.as_deref()),
            );
            Some(YTItem::Song(SongItem {
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
                artists: odd_elements(secondary_line.first()?)
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
                album: secondary_line.get(1).and_then(|segment| {
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
                duration: secondary_line
                    .last()
                    .and_then(|segment| segment.first())
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
                shuffle_endpoint: find_menu_watch(renderer, "MUSIC_SHUFFLE"),
                radio_endpoint: find_menu_watch(renderer, "MIX"),
                ..Default::default()
            }))
        } else if renderer.is_album() {
            Some(YTItem::Album(AlbumItem {
                browse_id: renderer
                    .navigation_endpoint
                    .as_ref()?
                    .browse_endpoint
                    .as_ref()?
                    .browse_id
                    .clone(),
                playlist_id: renderer
                    .overlay
                    .as_ref()?
                    .music_item_thumbnail_overlay_renderer
                    .content
                    .music_play_button_renderer
                    .play_navigation_endpoint
                    .as_ref()?
                    .any_watch_endpoint()?
                    .playlist_id
                    .clone()?,
                title: renderer
                    .flex_columns
                    .first()?
                    .music_responsive_list_item_flex_column_renderer
                    .text
                    .as_ref()?
                    .first()?
                    .text
                    .clone(),
                artists: Some(
                    odd_elements(secondary_line.get(1)?)
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
                ),
                year: secondary_line
                    .get(2)
                    .and_then(|segment| segment.first())
                    .and_then(|run| run.text.parse().ok()),
                thumbnail: renderer
                    .thumbnail
                    .as_ref()?
                    .music_thumbnail_renderer
                    .as_ref()?
                    .get_thumbnail_url()?
                    .to_owned(),
                explicit: explicit_badge(&renderer.badges),
                ..Default::default()
            }))
        } else if renderer.is_playlist() {
            let browse_id = renderer
                .navigation_endpoint
                .as_ref()?
                .browse_endpoint
                .as_ref()?
                .browse_id
                .clone();
            Some(YTItem::Playlist(PlaylistItem {
                id: browse_id
                    .strip_prefix("VL")
                    .map(str::to_owned)
                    .unwrap_or(browse_id),
                title: renderer
                    .flex_columns
                    .first()?
                    .music_responsive_list_item_flex_column_renderer
                    .text
                    .as_ref()?
                    .first()?
                    .text
                    .clone(),
                author: Some(Artist {
                    name: secondary_line.first()?.first()?.text.clone(),
                    id: secondary_line
                        .first()?
                        .first()?
                        .navigation_endpoint
                        .as_ref()
                        .and_then(|endpoint| endpoint.browse_endpoint.as_ref())
                        .map(|browse| browse.browse_id.clone()),
                }),
                song_count_text: Some(
                    renderer
                        .flex_columns
                        .get(1)?
                        .music_responsive_list_item_flex_column_renderer
                        .text
                        .as_ref()?
                        .runs
                        .as_ref()?
                        .last()?
                        .text
                        .clone(),
                ),
                thumbnail: Some(
                    renderer
                        .thumbnail
                        .as_ref()?
                        .music_thumbnail_renderer
                        .as_ref()?
                        .get_thumbnail_url()?
                        .to_owned(),
                ),
                play_endpoint: renderer
                    .overlay
                    .as_ref()?
                    .music_item_thumbnail_overlay_renderer
                    .content
                    .music_play_button_renderer
                    .play_navigation_endpoint
                    .as_ref()?
                    .watch_playlist_endpoint
                    .clone(),
                shuffle_endpoint: find_menu_watch(renderer, "MUSIC_SHUFFLE"),
                radio_endpoint: find_menu_watch(renderer, "MIX"),
                is_editable: false,
            }))
        } else {
            None
        }
    }
}

/// The menu-icon watch-endpoint walk the search/summary converters use on
/// list items (SearchPage.kt:88-101 and siblings).
pub(crate) fn find_menu_watch(
    renderer: &MusicResponsiveListItemRenderer,
    icon: &str,
) -> Option<crate::models::endpoint::WatchEndpoint> {
    renderer
        .menu
        .as_ref()?
        .menu_renderer
        .items
        .as_ref()?
        .iter()
        .find_map(|item| {
            let navigation = item.menu_navigation_item_renderer.as_ref()?;
            (navigation.icon.icon_type == icon)
                .then(|| {
                    navigation
                        .navigation_endpoint
                        .watch_playlist_endpoint
                        .clone()
                })
                .flatten()
        })
}
