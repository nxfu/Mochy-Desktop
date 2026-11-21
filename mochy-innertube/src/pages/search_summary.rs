//! `pages/SearchSummaryPage.kt` (419 lines).

use crate::models::music_renderers::{MusicCardShelfRenderer, MusicResponsiveListItemRenderer};
use crate::models::runs::{Run, clean, looks_like_timestamp, odd_elements, split_by_separator};
use crate::models::ytitem::{
    Album, AlbumItem, Artist, ArtistItem, PlaylistItem, SongItem, YTItem, filter_explicit,
    filter_video_songs, filter_youtube_shorts,
};
use crate::pages::home::explicit_badge;
use crate::pages::page_helper::PageHelper;
use crate::pages::search::find_menu_watch;
use crate::utils::parse_time;

/// `SearchSummaryPage.kt:22-25`.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchSummary {
    pub title: String,
    pub items: Vec<YTItem>,
}

/// `SearchSummaryPage.kt:27-29`.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchSummaryPage {
    pub summaries: Vec<SearchSummary>,
}

/// `songArtists` (SearchSummaryPage.kt:212-227) — a top-result card's
/// songs read "song • 3:02" with no artist, so take the segment that
/// links to an artist, never a timestamp, and let the card supply its own
/// artist.
fn song_artists(segments: &[Vec<Run>], fallback: Option<Artist>) -> Vec<Artist> {
    let linked = segments.iter().find(|segment| {
        segment.iter().any(|run| {
            run.navigation_endpoint
                .as_ref()
                .and_then(|endpoint| endpoint.browse_endpoint.as_ref())
                .map(|browse| browse.is_artist_endpoint())
                .unwrap_or(false)
        })
    });
    if linked.is_none()
        && let Some(fallback) = fallback
    {
        return vec![fallback];
    }
    let Some(segment) = linked.or_else(|| segments.first()) else {
        return Vec::new();
    };
    odd_elements(segment)
        .iter()
        .filter(|run| !looks_like_timestamp(&run.text))
        .map(|run| Artist {
            name: run.text.clone(),
            id: run
                .navigation_endpoint
                .as_ref()
                .and_then(|endpoint| endpoint.browse_endpoint.as_ref())
                .map(|browse| browse.browse_id.clone()),
        })
        .collect()
}

pub struct SearchSummaryPageParser;

impl SearchSummaryPageParser {
    /// `fromMusicCardShelfRenderer` (SearchSummaryPage.kt:82-210).
    pub fn from_music_card_shelf_renderer(renderer: &MusicCardShelfRenderer) -> Option<YTItem> {
        let subtitle = split_by_separator(renderer.subtitle.runs.as_deref()?);
        if renderer.on_tap.watch_endpoint.is_some() {
            Some(YTItem::Song(SongItem {
                id: renderer.on_tap.watch_endpoint.as_ref()?.video_id.clone()?,
                title: renderer.title.first()?.text.clone(),
                artists: odd_elements(subtitle.get(1)?)
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
                album: subtitle.get(2).and_then(|segment| {
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
                duration: subtitle
                    .last()
                    .and_then(|segment| segment.first())
                    .and_then(|run| parse_time(&run.text)),
                music_video_type: renderer.on_tap.music_video_type().map(str::to_owned),
                thumbnail: renderer
                    .thumbnail
                    .music_thumbnail_renderer
                    .as_ref()?
                    .get_thumbnail_url()?
                    .to_owned(),
                explicit: explicit_badge(&renderer.subtitle_badges),
                ..Default::default()
            }))
        } else if renderer
            .on_tap
            .browse_endpoint
            .as_ref()
            .map(|browse| browse.is_artist_endpoint())
            .unwrap_or(false)
        {
            Some(YTItem::Artist(ArtistItem {
                id: renderer.on_tap.browse_endpoint.as_ref()?.browse_id.clone(),
                title: renderer.title.first()?.text.clone(),
                thumbnail: Some(
                    renderer
                        .thumbnail
                        .music_thumbnail_renderer
                        .as_ref()?
                        .get_thumbnail_url()?
                        .to_owned(),
                ),
                shuffle_endpoint: card_button_watch(renderer, "MUSIC_SHUFFLE", false),
                radio_endpoint: card_button_watch(renderer, "MIX", false),
                ..Default::default()
            }))
        } else if renderer
            .on_tap
            .browse_endpoint
            .as_ref()
            .map(|browse| browse.is_album_endpoint())
            .unwrap_or(false)
        {
            Some(YTItem::Album(AlbumItem {
                browse_id: renderer.on_tap.browse_endpoint.as_ref()?.browse_id.clone(),
                playlist_id: renderer
                    .buttons
                    .first()?
                    .button_renderer
                    .command
                    .as_ref()?
                    .any_watch_endpoint()?
                    .playlist_id
                    .clone()?,
                title: renderer.title.first()?.text.clone(),
                artists: Some(
                    odd_elements(subtitle.get(1)?)
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
                year: None,
                thumbnail: renderer
                    .thumbnail
                    .music_thumbnail_renderer
                    .as_ref()?
                    .get_thumbnail_url()?
                    .to_owned(),
                explicit: explicit_badge(&renderer.subtitle_badges),
                ..Default::default()
            }))
        } else if renderer
            .on_tap
            .browse_endpoint
            .as_ref()
            .map(|browse| browse.is_playlist_endpoint())
            .unwrap_or(false)
        {
            Some(YTItem::Playlist(PlaylistItem {
                id: renderer
                    .on_tap
                    .browse_endpoint
                    .as_ref()?
                    .browse_id
                    .strip_prefix("VL")
                    .map(str::to_owned)
                    .unwrap_or_else(|| {
                        renderer
                            .on_tap
                            .browse_endpoint
                            .as_ref()
                            .unwrap()
                            .browse_id
                            .clone()
                    }),
                title: renderer
                    .header
                    .as_ref()?
                    .music_card_shelf_header_basic_renderer
                    .title
                    .text(),
                author: Some(Artist {
                    id: None,
                    name: renderer
                        .subtitle
                        .runs
                        .as_ref()?
                        .iter()
                        .map(|run| run.text.as_str())
                        .collect::<Vec<_>>()
                        .join(" "),
                }),
                song_count_text: None,
                thumbnail: Some(
                    renderer
                        .thumbnail
                        .music_thumbnail_renderer
                        .as_ref()?
                        .get_thumbnail_url()?
                        .to_owned(),
                ),
                play_endpoint: card_button_watch(renderer, "PLAY_ARROW", false),
                shuffle_endpoint: card_button_watch(renderer, "MUSIC_SHUFFLE", false),
                radio_endpoint: None,
                is_editable: false,
            }))
        } else {
            None
        }
    }

    /// `fromMusicResponsiveListItemRenderer` (SearchSummaryPage.kt:229-417).
    pub fn from_music_responsive_list_item_renderer(
        renderer: &MusicResponsiveListItemRenderer,
        fallback_artist: Option<Artist>,
    ) -> Option<YTItem> {
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
        let third_line = renderer
            .flex_columns
            .get(2)
            .and_then(|column| {
                column
                    .music_responsive_list_item_flex_column_renderer
                    .text
                    .as_ref()
            })
            .and_then(|text| text.runs.as_ref())
            .map(|runs| split_by_separator(runs))
            .unwrap_or_default();
        let list_run = clean(&[secondary_line.clone(), third_line].concat());

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
                artists: song_artists(&list_run, fallback_artist),
                album: list_run.get(1).and_then(|segment| {
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
                    .watch_playlist_endpoint
                    .as_ref()?
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
                    name: secondary_line.get(1)?.first()?.text.clone(),
                    id: secondary_line
                        .get(1)?
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

/// A card button's watch endpoint by icon (SearchSummaryPage.kt:129-140).
fn card_button_watch(
    renderer: &MusicCardShelfRenderer,
    icon: &str,
    _from_command: bool,
) -> Option<crate::models::endpoint::WatchEndpoint> {
    renderer.buttons.iter().find_map(|button| {
        let button = &button.button_renderer;
        (button.icon.as_ref()?.icon_type == icon).then(|| {
            button
                .command
                .as_ref()
                .and_then(|command| command.watch_playlist_endpoint.clone())
        })?
    })
}

impl SearchSummaryPage {
    /// `filterExplicit` (SearchSummaryPage.kt:30-45) — empty summaries
    /// drop.
    pub fn filter_explicit(self, enabled: bool) -> Self {
        Self::filtered(self, |items| filter_explicit(items, enabled))
    }

    /// `filterVideoSongs` (SearchSummaryPage.kt:47-62).
    pub fn filter_video_songs(self, disable_videos: bool) -> Self {
        Self::filtered(self, |items| filter_video_songs(items, disable_videos))
    }

    /// `filterYoutubeShorts` (SearchSummaryPage.kt:64-79).
    pub fn filter_youtube_shorts(self, enabled: bool) -> Self {
        Self::filtered(self, |items| filter_youtube_shorts(items, enabled))
    }

    fn filtered(this: Self, filter: impl Fn(&[YTItem]) -> Vec<YTItem>) -> Self {
        SearchSummaryPage {
            summaries: this
                .summaries
                .into_iter()
                .filter_map(|summary| {
                    let items = filter(&summary.items);
                    if items.is_empty() {
                        None
                    } else {
                        Some(SearchSummary {
                            title: summary.title,
                            items,
                        })
                    }
                })
                .collect(),
        }
    }
}
