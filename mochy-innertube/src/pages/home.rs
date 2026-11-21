//! `pages/HomePage.kt` (197 lines).

use crate::models::endpoint::{BrowseEndpoint, WatchEndpoint};
use crate::models::music_renderers::{MusicCarouselShelfRenderer, MusicTwoRowItemRenderer};
use crate::models::runs::{Run, odd_elements};
use crate::models::section_list::{ChipCloudChipRenderer, SectionListRenderer};
use crate::models::ytitem::{
    Album, AlbumItem, Artist, ArtistItem, PlaylistItem, SongItem, YTItem, filter_explicit,
    filter_video_songs,
};

/// `HomePage.kt:20-24`.
#[derive(Debug, Clone, PartialEq)]
pub struct HomePage {
    pub chips: Option<Vec<Chip>>,
    pub sections: Vec<HomeSection>,
    pub continuation: Option<String>,
}

/// `HomePage.Chip` (HomePage.kt:25-40).
#[derive(Debug, Clone, PartialEq)]
pub struct Chip {
    pub title: String,
    pub endpoint: Option<BrowseEndpoint>,
    pub deselect_endpoint: Option<BrowseEndpoint>,
}

impl Chip {
    /// `Chip.fromChipCloudChipRenderer` (HomePage.kt:31-39).
    pub fn from_chip_cloud_chip_renderer(renderer: &ChipCloudChipRenderer) -> Option<Chip> {
        Some(Chip {
            title: renderer.text.as_ref()?.first()?.text.clone(),
            endpoint: renderer.navigation_endpoint.browse_endpoint.clone(),
            deselect_endpoint: renderer
                .on_deselected_command
                .as_ref()
                .and_then(|command| command.browse_endpoint.clone()),
        })
    }
}

/// `HomePage.Section` (HomePage.kt:42-51).
#[derive(Debug, Clone, PartialEq)]
pub struct HomeSection {
    pub title: String,
    pub label: Option<String>,
    pub thumbnail: Option<String>,
    pub endpoint: Option<BrowseEndpoint>,
    pub items: Vec<YTItem>,
    pub is_speed_dial: bool,
}

/// The `MUSIC_EXPLICIT_BADGE` check every converter repeats.
pub(crate) fn explicit_badge(badges: &Option<Vec<crate::models::common::Badges>>) -> bool {
    badges
        .as_ref()
        .map(|badges| {
            badges.iter().any(|badge| {
                badge
                    .music_inline_badge_renderer
                    .as_ref()
                    .map(|renderer| renderer.icon.icon_type == "MUSIC_EXPLICIT_BADGE")
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

/// The `MUSIC_SHUFFLE` menu walk every two-row converter repeats
/// (HomePage.kt:155-157 and its siblings).
pub(crate) fn find_shuffle_watch_endpoint(
    renderer: &MusicTwoRowItemRenderer,
) -> Option<WatchEndpoint> {
    renderer
        .menu
        .as_ref()?
        .menu_renderer
        .items
        .as_ref()?
        .iter()
        .find_map(|item| {
            let navigation = item.menu_navigation_item_renderer.as_ref()?;
            (navigation.icon.icon_type == "MUSIC_SHUFFLE")
                .then(|| {
                    navigation
                        .navigation_endpoint
                        .watch_playlist_endpoint
                        .clone()
                })
                .flatten()
        })
}

/// The `MIX` menu walk (HomePage.kt:158-160 and its siblings).
pub(crate) fn find_radio_watch_endpoint(
    renderer: &MusicTwoRowItemRenderer,
) -> Option<WatchEndpoint> {
    renderer
        .menu
        .as_ref()?
        .menu_renderer
        .items
        .as_ref()?
        .iter()
        .find_map(|item| {
            let navigation = item.menu_navigation_item_renderer.as_ref()?;
            (navigation.icon.icon_type == "MIX")
                .then(|| {
                    navigation
                        .navigation_endpoint
                        .watch_playlist_endpoint
                        .clone()
                })
                .flatten()
        })
}

/// The artist filter shared by the home/artist two-row converters
/// (HomePage.kt:90-103 / ArtistPage.kt:126-138): a channel id, or any
/// browse link that is not an album link.
pub(crate) fn artist_like(run: &Run) -> bool {
    run.navigation_endpoint
        .as_ref()
        .and_then(|endpoint| endpoint.browse_endpoint.as_ref())
        .map(|browse| browse.browse_id.starts_with("UC") || !browse.browse_id.starts_with("MPREb_"))
        .unwrap_or(false)
}

pub(crate) fn run_artist(run: &Run) -> Artist {
    Artist {
        name: run.text.clone(),
        id: run
            .navigation_endpoint
            .as_ref()
            .and_then(|endpoint| endpoint.browse_endpoint.as_ref())
            .map(|browse| browse.browse_id.clone()),
    }
}

/// The `ifEmpty` fallback of the artist lists: the first subtitle run
/// becomes a nameless-id artist (HomePage.kt:99-102).
pub(crate) fn artists_or_first(subtitle_runs: &[Run], filtered: Vec<Artist>) -> Vec<Artist> {
    if filtered.is_empty() {
        subtitle_runs
            .first()
            .map(|run| {
                vec![Artist {
                    name: run.text.clone(),
                    id: None,
                }]
            })
            .unwrap_or_default()
    } else {
        filtered
    }
}

impl HomeSection {
    /// `Section.fromMusicCarouselShelfRenderer` (HomePage.kt:52-69) — a
    /// shelf with no two-row items is dropped entirely.
    pub fn from_music_carousel_shelf_renderer(
        renderer: &MusicCarouselShelfRenderer,
    ) -> Option<HomeSection> {
        let carousel_header = renderer.header.as_ref()?;
        let header = &carousel_header.music_carousel_shelf_basic_header_renderer;
        let title = header.title.first()?.text.clone();
        let items: Vec<YTItem> = renderer
            .contents
            .iter()
            .filter_map(|content| content.music_two_row_item_renderer.as_ref())
            .filter_map(from_music_two_row_item_renderer)
            .collect();
        if items.is_empty() {
            return None;
        }
        Some(HomeSection {
            title: title.clone(),
            label: header
                .strapline
                .as_ref()
                .and_then(|strapline| strapline.first())
                .map(|run| run.text.clone()),
            thumbnail: header
                .thumbnail
                .as_ref()
                .and_then(|thumbnail| thumbnail.music_thumbnail_renderer.as_ref())
                .and_then(|music| music.get_thumbnail_url().map(str::to_owned)),
            endpoint: header
                .more_content_button
                .as_ref()
                .and_then(|button| button.button_renderer.navigation_endpoint.as_ref())
                .and_then(|endpoint| endpoint.browse_endpoint.clone()),
            items,
            is_speed_dial: is_speed_dial_shelf(renderer, header, &title),
        })
    }
}

/// `isSpeedDialShelf` (HomePage.kt:71-81) — the title is translated, so a
/// three-row grid of covers under your own avatar counts too.
fn is_speed_dial_shelf(
    renderer: &MusicCarouselShelfRenderer,
    header: &crate::models::music_renderers::MusicCarouselShelfBasicHeaderRenderer,
    title: &str,
) -> bool {
    if title.eq_ignore_ascii_case("Speed dial") {
        return true;
    }
    (renderer.num_items_per_column.unwrap_or(1) >= 3)
        && header.thumbnail.is_some()
        && renderer
            .contents
            .iter()
            .all(|content| content.music_two_row_item_renderer.is_some())
}

/// `Section.fromMusicTwoRowItemRenderer` (HomePage.kt:83-180) — the
/// home-flavored converter (the artist title reads the LAST run).
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
                .filter(|run| artist_like(run))
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
            album: subtitle_runs
                .iter()
                .find(|run| {
                    run.navigation_endpoint
                        .as_ref()
                        .and_then(|endpoint| endpoint.browse_endpoint.as_ref())
                        .map(|browse| browse.browse_id.starts_with("MPREb_"))
                        .unwrap_or(false)
                })
                .and_then(|run| {
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
                .watch_playlist_endpoint
                .as_ref()?
                .playlist_id
                .clone()?,
            title: renderer.title.first()?.text.clone(),
            artists: Some(
                odd_elements(renderer.subtitle.as_ref()?.runs.as_ref()?)
                    .iter()
                    .skip(1)
                    .map(run_artist)
                    .collect(),
            ),
            year: None,
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
            shuffle_endpoint: find_shuffle_watch_endpoint(renderer),
            radio_endpoint: find_radio_watch_endpoint(renderer),
            ..Default::default()
        }))
    } else {
        None
    }
}

impl HomePage {
    /// `filterExplicit` (HomePage.kt:184-189) — sections stay even when
    /// they end up empty (only BrowseResult drops sections).
    pub fn filter_explicit(self, enabled: bool) -> Self {
        HomePage {
            chips: self.chips,
            sections: self
                .sections
                .into_iter()
                .map(|section| HomeSection {
                    items: filter_explicit(&section.items, enabled),
                    ..section
                })
                .collect(),
            continuation: self.continuation,
        }
    }

    /// `filterVideoSongs` (HomePage.kt:191-196).
    pub fn filter_video_songs(self, disable_videos: bool) -> Self {
        HomePage {
            chips: self.chips,
            sections: self
                .sections
                .into_iter()
                .map(|section| HomeSection {
                    items: filter_video_songs(&section.items, disable_videos),
                    ..section
                })
                .collect(),
            continuation: self.continuation,
        }
    }
}

/// The chip-cloud walk of the facade's `home()` (HomePage chips,
/// HomePage.kt:707).
pub(crate) fn home_chips(section_list: &SectionListRenderer) -> Option<Vec<Chip>> {
    Some(
        section_list
            .header
            .as_ref()?
            .chip_cloud_renderer
            .as_ref()?
            .chips
            .iter()
            .filter_map(|chip| {
                chip.chip_cloud_chip_renderer
                    .as_ref()
                    .and_then(Chip::from_chip_cloud_chip_renderer)
            })
            .collect(),
    )
}
