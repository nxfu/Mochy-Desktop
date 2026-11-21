//! `pages/SearchSuggestionPage.kt` (151 lines).

use crate::models::music_renderers::MusicResponsiveListItemRenderer;
use crate::models::runs::{clean, odd_elements, split_by_separator};
use crate::models::ytitem::{Album, AlbumItem, ArtistItem, SongItem, YTItem};
use crate::pages::home::explicit_badge;
use crate::pages::search::find_menu_watch;

pub struct SearchSuggestionPage;

impl SearchSuggestionPage {
    /// `fromMusicResponsiveListItemRenderer` (SearchSuggestionPage.kt:15-151).
    pub fn from_music_responsive_list_item_renderer(
        renderer: &MusicResponsiveListItemRenderer,
    ) -> Option<YTItem> {
        if renderer.is_song() {
            let secondary_line = clean(&split_by_separator(
                renderer
                    .flex_columns
                    .get(1)?
                    .music_responsive_list_item_flex_column_renderer
                    .text
                    .as_ref()?
                    .runs
                    .as_ref()?,
            ));
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
                    .map(|run| crate::models::ytitem::Artist {
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
                        // `id = browseId ?: return null` — a suggestion
                        // song without an album link drops the album
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
            Some(YTItem::Album(AlbumItem {
                browse_id: renderer
                    .navigation_endpoint
                    .as_ref()?
                    .browse_endpoint
                    .as_ref()?
                    .browse_id
                    .clone(),
                // the suggestion album's playlist rides the shuffle menu
                // entry (SearchSuggestionPage.kt:110-119)
                playlist_id: find_menu_watch(renderer, "MUSIC_SHUFFLE")?
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
                        .map(|run| crate::models::ytitem::Artist {
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
                    .last()
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
        } else {
            None
        }
    }
}
