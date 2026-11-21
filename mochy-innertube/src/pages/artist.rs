//! `pages/ArtistPage.kt` (210 lines).

use crate::models::endpoint::BrowseEndpoint;
use crate::models::music_renderers::{
    MusicCarouselShelfRenderer, MusicResponsiveListItemRenderer, MusicShelfRenderer,
    MusicTwoRowItemRenderer,
};
use crate::models::runs::{Run, odd_elements, split_by_separator};
use crate::models::section_list::Content as SectionContent;
use crate::models::ytitem::{Album, AlbumItem, Artist, ArtistItem, PlaylistItem, SongItem, YTItem};
use crate::pages::home::{
    artists_or_first, explicit_badge, find_radio_watch_endpoint, find_shuffle_watch_endpoint,
    run_artist,
};
use crate::pages::page_helper::PageHelper;

/// `ArtistPage.kt:21-25`.
#[derive(Debug, Clone, PartialEq)]
pub struct ArtistSection {
    pub title: String,
    pub items: Vec<YTItem>,
    pub more_endpoint: Option<BrowseEndpoint>,
}

/// `ArtistPage.kt:27-36`.
#[derive(Debug, Clone, PartialEq)]
pub struct ArtistPage {
    pub artist: ArtistItem,
    pub sections: Vec<ArtistSection>,
    pub description: Option<String>,
    pub subscriber_count_text: Option<String>,
    pub monthly_listener_count: Option<String>,
    pub description_runs: Option<Vec<Run>>,
    /// Whether the signed-in account follows this channel; None when
    /// signed out or not said.
    pub subscribed: Option<bool>,
}

/// The flexColumns→(artists, album) walk the artist parsers repeat
/// (ArtistPage.kt:70-95).
fn secondary_artists_album(
    renderer: &MusicResponsiveListItemRenderer,
) -> (Option<Vec<Artist>>, Option<Album>) {
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

    let artists = secondary_line_runs.as_ref().and_then(|segments| {
        segments.first().map(|segment| {
            odd_elements(segment)
                .iter()
                .map(run_artist)
                .collect::<Vec<_>>()
        })
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
                .map(|browse| Album {
                    name: run.text.clone(),
                    id: browse.browse_id.clone(),
                })
        });

    (artists, album)
}

pub struct ArtistPageParser;

impl ArtistPageParser {
    /// `fromSectionListRendererContent` (ArtistPage.kt:38-44).
    pub fn from_section_list_renderer_content(content: &SectionContent) -> Option<ArtistSection> {
        if let Some(shelf) = content.music_shelf_renderer.as_ref() {
            Self::from_music_shelf_renderer(shelf)
        } else if let Some(carousel) = content.music_carousel_shelf_renderer.as_ref() {
            Self::from_music_carousel_shelf_renderer(carousel)
        } else {
            None
        }
    }

    /// `fromMusicShelfRenderer` (ArtistPage.kt:46-54).
    pub fn from_music_shelf_renderer(renderer: &MusicShelfRenderer) -> Option<ArtistSection> {
        let items = renderer
            .contents
            .as_ref()?
            .iter()
            .filter_map(|content| content.music_responsive_list_item_renderer.as_ref())
            .filter_map(Self::from_music_responsive_list_item_renderer)
            .map(YTItem::Song)
            .collect::<Vec<_>>();
        if items.is_empty() {
            return None;
        }
        Some(ArtistSection {
            title: renderer
                .title
                .as_ref()
                .and_then(|title| title.first())
                .map(|run| run.text.clone())
                .unwrap_or_default(),
            items,
            more_endpoint: renderer
                .title
                .as_ref()
                .and_then(|title| title.first())
                .and_then(|run| {
                    run.navigation_endpoint
                        .as_ref()
                        .and_then(|endpoint| endpoint.browse_endpoint.clone())
                }),
        })
    }

    /// `fromMusicCarouselShelfRenderer` (ArtistPage.kt:56-68).
    pub fn from_music_carousel_shelf_renderer(
        renderer: &MusicCarouselShelfRenderer,
    ) -> Option<ArtistSection> {
        let carousel_header = renderer.header.as_ref()?;
        let header = &carousel_header.music_carousel_shelf_basic_header_renderer;
        let items = renderer
            .contents
            .iter()
            .filter_map(|content| {
                content
                    .music_two_row_item_renderer
                    .as_ref()
                    .and_then(Self::from_music_two_row_item_renderer)
                    .or_else(|| {
                        content
                            .music_responsive_list_item_renderer
                            .as_ref()
                            .and_then(|renderer| {
                                Self::from_music_responsive_list_item_renderer(renderer)
                                    .map(YTItem::Song)
                            })
                    })
            })
            .collect::<Vec<_>>();
        // `ifEmpty { null } ?: return null` (ArtistPage.kt:64-65)
        if items.is_empty() {
            return None;
        }
        Some(ArtistSection {
            title: header.title.first()?.text.clone(),
            items,
            more_endpoint: header
                .more_content_button
                .as_ref()
                .and_then(|button| button.button_renderer.navigation_endpoint.as_ref())
                .and_then(|endpoint| endpoint.browse_endpoint.clone()),
        })
    }

    /// `fromMusicResponsiveListItemRenderer` (ArtistPage.kt:70-117).
    pub fn from_music_responsive_list_item_renderer(
        renderer: &MusicResponsiveListItemRenderer,
    ) -> Option<SongItem> {
        let (artists, album) = secondary_artists_album(renderer);
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

    /// `fromMusicTwoRowItemRenderer` (ArtistPage.kt:119-208).
    pub fn from_music_two_row_item_renderer(renderer: &MusicTwoRowItemRenderer) -> Option<YTItem> {
        if renderer.is_song() {
            let subtitle_runs = odd_elements(renderer.subtitle.as_ref()?.runs.as_ref()?);
            if subtitle_runs.is_empty() {
                return None;
            }
            let artists = artists_or_first(
                &subtitle_runs,
                subtitle_runs
                    .iter()
                    .filter(|run| {
                        run.navigation_endpoint
                            .as_ref()
                            .and_then(|endpoint| endpoint.browse_endpoint.as_ref())
                            .is_some()
                    })
                    .map(run_artist)
                    .collect(),
            );
            Some(YTItem::Song(SongItem {
                id: renderer
                    .navigation_endpoint
                    .watch_endpoint
                    .as_ref()?
                    .video_id
                    .clone()?,
                title: renderer.title.first()?.text.clone(),
                artists,
                album: None,
                duration: None,
                music_video_type: renderer.music_video_type(),
                thumbnail: renderer
                    .thumbnail_renderer
                    .music_thumbnail_renderer
                    .as_ref()?
                    .get_thumbnail_url()?
                    .to_owned(),
                explicit: explicit_badge(&renderer.subtitle_badges),
                ..Default::default()
            }))
        } else if renderer.is_album() {
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
                    name: renderer.subtitle.as_ref()?.first()?.text.clone(),
                    id: None,
                }),
                song_count_text: None,
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
                title: renderer.title.last()?.text.clone(),
                thumbnail: Some(
                    renderer
                        .thumbnail_renderer
                        .music_thumbnail_renderer
                        .as_ref()?
                        .get_thumbnail_url()?
                        .to_owned(),
                ),
                channel_id: renderer
                    .menu
                    .as_ref()?
                    .menu_renderer
                    .items
                    .as_ref()?
                    .iter()
                    .find_map(|item| {
                        let toggle = item.toggle_menu_service_item_renderer.as_ref()?;
                        (toggle.default_icon.icon_type == "SUBSCRIBE").then(|| {
                            toggle
                                .default_service_endpoint
                                .subscribe_endpoint
                                .as_ref()
                                .and_then(|subscribe| subscribe.channel_ids.first().cloned())
                        })?
                    }),
                shuffle_endpoint: find_shuffle_watch_endpoint(renderer),
                radio_endpoint: find_radio_watch_endpoint(renderer),
                ..Default::default()
            }))
        } else {
            None
        }
    }
}
