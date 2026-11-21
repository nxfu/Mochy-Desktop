//! `pages/RelatedPage.kt` (176 lines).

use crate::models::music_renderers::{MusicResponsiveListItemRenderer, MusicTwoRowItemRenderer};
use crate::models::runs::{odd_elements, split_by_separator};
use crate::models::ytitem::{Album, AlbumItem, Artist, ArtistItem, PlaylistItem, SongItem, YTItem};
use crate::pages::home::{explicit_badge, find_radio_watch_endpoint, find_shuffle_watch_endpoint};
use crate::pages::page_helper::PageHelper;

/// `RelatedPage.kt:15-20`.
#[derive(Debug, Clone, PartialEq)]
pub struct RelatedPage {
    pub songs: Vec<SongItem>,
    pub albums: Vec<AlbumItem>,
    pub artists: Vec<ArtistItem>,
    pub playlists: Vec<PlaylistItem>,
}

pub struct RelatedPageParser;

impl RelatedPageParser {
    /// `fromMusicResponsiveListItemRenderer` (RelatedPage.kt:22-67).
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
            artists: odd_elements(secondary_line_runs.as_ref()?.first()?)
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
                    // `id = browseId ?: return null` — the album drops
                    // when the run carries no browse link
                    Some(Album {
                        name: run.text.clone(),
                        id: run
                            .navigation_endpoint
                            .as_ref()?
                            .browse_endpoint
                            .as_ref()?
                            .browse_id
                            .clone(),
                    })
                }),
            duration: None,
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
        })
    }

    /// `fromMusicTwoRowItemRenderer` (RelatedPage.kt:69-174) — the
    /// related-flavored converter (playlist song-count is digit-bearing
    /// and not a view count).
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
                artists: Some(
                    split_by_separator(renderer.subtitle.as_ref()?.runs.as_ref()?)
                        .get(1)
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
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default(),
                ),
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
                author: Some(Artist {
                    name: renderer.subtitle.as_ref()?.last()?.text.clone(),
                    id: None,
                }),
                // a digit-bearing run that is not a view count
                // (RelatedPage.kt:118-120)
                song_count_text: renderer
                    .subtitle
                    .as_ref()?
                    .runs
                    .as_ref()?
                    .iter()
                    .rev()
                    .find(|run| {
                        run.text.chars().any(|c| c.is_ascii_digit())
                            && !run.text.to_lowercase().contains("view")
                    })
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
        } else if renderer.is_artist() {
            Some(YTItem::Artist(ArtistItem {
                id: renderer
                    .navigation_endpoint
                    .browse_endpoint
                    .as_ref()?
                    .browse_id
                    .clone(),
                title: renderer.title.first()?.text.clone(),
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
}
