//! `pages/AlbumPage.kt` (122 lines).

use crate::models::music_headers::MusicResponsiveHeaderRenderer;
use crate::models::music_renderers::{MusicResponsiveListItemRenderer, MusicShelfContent};
use crate::models::response::BrowseResponse;
use crate::models::runs::{Run, odd_elements};
use crate::models::ytitem::{Album, AlbumItem, Artist, SongItem};
use crate::pages::page_helper::PageHelper;
use crate::utils::parse_time;

/// `AlbumPage.kt:15-22`.
#[derive(Debug, Clone, PartialEq)]
pub struct AlbumPage {
    pub album: AlbumItem,
    pub songs: Vec<SongItem>,
    pub other_versions: Vec<AlbumItem>,
    pub releases_for_you: Vec<AlbumItem>,
    pub description: Option<String>,
    pub description_runs: Option<Vec<Run>>,
}

/// The album-side tabs accessor: single-column first, then two-column
/// (AlbumPage.kt:66-67 / 75-76).
/// The album-side tabs accessor: single-column first, then two-column
/// (AlbumPage.kt:66-67 / 75-76). The two-column variant carries nullable
/// tabs (`List<Tabs.Tab?>?`), so both shapes funnel through here.
pub(crate) fn first_album_tab(response: &BrowseResponse) -> Option<&crate::models::tabs::Tab> {
    let contents = response.contents.as_ref()?;
    if let Some(single) = contents.single_column_browse_results_renderer.as_ref() {
        return single.tabs.first();
    }
    contents
        .two_column_browse_results_renderer
        .as_ref()?
        .tabs
        .as_ref()?
        .iter()
        .flatten()
        .next()
}

pub struct AlbumPageParser;

impl AlbumPageParser {
    /// `getPlaylistId` (AlbumPage.kt:24-32) — the canonical url's last
    /// parameter, else the header play button's playlist.
    pub fn get_playlist_id(response: &BrowseResponse) -> Option<String> {
        if let Some(playlist_id) = response
            .microformat
            .as_ref()?
            .microformat_data_renderer
            .as_ref()?
            .url_canonical
            .as_ref()
        {
            return Some(
                playlist_id
                    .rsplit('=')
                    .next()
                    .unwrap_or_default()
                    .to_owned(),
            );
        }
        response
            .header
            .as_ref()?
            .music_detail_header_renderer
            .as_ref()?
            .menu
            .menu_renderer
            .top_level_buttons
            .as_ref()?
            .first()?
            .button_renderer
            .as_ref()?
            .navigation_endpoint
            .watch_playlist_endpoint
            .as_ref()?
            .playlist_id
            .clone()
    }

    /// `getTitle` (AlbumPage.kt:34-37).
    pub fn get_title(response: &BrowseResponse) -> Option<String> {
        let title = Self::get_header(response)
            .map(|header| header.title.clone())
            .or_else(|| {
                response
                    .header
                    .as_ref()?
                    .music_detail_header_renderer
                    .as_ref()
                    .map(|header| header.title.clone())
            })?;
        title.first().map(|run| run.text.clone())
    }

    /// `getYear` (AlbumPage.kt:39-42).
    pub fn get_year(response: &BrowseResponse) -> Option<i32> {
        let subtitle = Self::get_header(response)
            .map(|header| header.subtitle.clone())
            .or_else(|| {
                response
                    .header
                    .as_ref()?
                    .music_detail_header_renderer
                    .as_ref()
                    .map(|header| header.subtitle.clone())
            })?;
        subtitle.last()?.text.parse().ok()
    }

    /// `getThumbnail` (AlbumPage.kt:44-47).
    pub fn get_thumbnail(response: &BrowseResponse) -> Option<String> {
        response
            .background
            .as_ref()?
            .music_thumbnail_renderer
            .as_ref()
            .and_then(|music| music.get_thumbnail_url().map(str::to_owned))
            .or_else(|| {
                response
                    .header
                    .as_ref()?
                    .music_detail_header_renderer
                    .as_ref()?
                    .thumbnail
                    .cropped_square_thumbnail_renderer
                    .as_ref()
                    .and_then(|music| music.get_thumbnail_url().map(str::to_owned))
            })
    }

    /// `getArtists` (AlbumPage.kt:49-63) — the responsive header's
    /// strapline, else the detail header's second subtitle segment.
    pub fn get_artists(response: &BrowseResponse) -> Vec<Artist> {
        let from_strapline = Self::get_header(response)
            .and_then(|header| header.strapline_text_one.as_ref()?.runs.as_ref().cloned());
        if let Some(runs) = from_strapline {
            return odd_elements(&runs)
                .iter()
                .map(|run| Artist {
                    name: run.text.clone(),
                    id: run
                        .navigation_endpoint
                        .as_ref()
                        .and_then(|endpoint| endpoint.browse_endpoint.as_ref())
                        .map(|browse| browse.browse_id.clone()),
                })
                .collect();
        }
        response
            .header
            .as_ref()
            .and_then(|header| header.music_detail_header_renderer.as_ref())
            .and_then(|detail| detail.subtitle.runs.as_ref().cloned())
            .and_then(|runs| {
                crate::models::runs::split_by_separator(&runs)
                    .get(1)
                    .cloned()
            })
            .map(|segment| {
                odd_elements(&segment)
                    .iter()
                    .map(|run| Artist {
                        name: run.text.clone(),
                        id: run
                            .navigation_endpoint
                            .as_ref()
                            .and_then(|endpoint| endpoint.browse_endpoint.as_ref())
                            .map(|browse| browse.browse_id.clone()),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// `getHeader` (AlbumPage.kt:65-72).
    fn get_header(response: &BrowseResponse) -> Option<&MusicResponsiveHeaderRenderer> {
        first_album_tab(response)?
            .tab_renderer
            .as_ref()?
            .content
            .as_ref()?
            .section_list_renderer
            .as_ref()?
            .contents
            .as_ref()?
            .first()?
            .music_responsive_header_renderer
            .as_ref()
    }

    /// `getSongs` (AlbumPage.kt:74-83).
    pub fn get_songs(response: &BrowseResponse, album: Option<&AlbumItem>) -> Vec<SongItem> {
        let shelf = Self::songs_shelf(response);
        shelf
            .map(|contents| {
                crate::models::music_renderers::shelf_items(contents)
                    .iter()
                    .filter_map(|renderer| Self::get_song(renderer, album))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The songs shelf: the tab content's musicShelf, else the
    /// two-column secondary contents (AlbumPage.kt:75-77).
    fn songs_shelf(response: &BrowseResponse) -> Option<&Vec<MusicShelfContent>> {
        let from_tabs = first_album_tab(response)
            .and_then(|tab| tab.tab_renderer.as_ref())
            .and_then(|tab| tab.content.as_ref())
            .and_then(|content| content.section_list_renderer.as_ref())
            .and_then(|section| section.contents.as_ref())
            .and_then(|contents| contents.first())
            .and_then(|content| content.music_shelf_renderer.as_ref())
            .and_then(|shelf| shelf.contents.as_ref());
        from_tabs.or(response
            .contents
            .as_ref()?
            .two_column_browse_results_renderer
            .as_ref()?
            .secondary_contents
            .as_ref()?
            .section_list_renderer
            .as_ref()?
            .contents
            .as_ref()?
            .first()?
            .music_shelf_renderer
            .as_ref()?
            .contents
            .as_ref())
    }

    /// `getSong` (AlbumPage.kt:85-120).
    ///
    /// Upstream force-unwraps the album and thumbnail fallbacks (`!!`);
    /// the port degrades to `None`/empty instead of failing the page
    /// (HANDOFF ## Phase 05 deviation).
    pub fn get_song(
        renderer: &MusicResponsiveListItemRenderer,
        album: Option<&AlbumItem>,
    ) -> Option<SongItem> {
        let library_tokens = PageHelper::extract_library_tokens_from_menu_items(
            renderer
                .menu
                .as_ref()
                .and_then(|menu| menu.menu_renderer.items.as_deref()),
        );

        let album_from_column = renderer
            .flex_columns
            .get(2)?
            .music_responsive_list_item_flex_column_renderer
            .text
            .as_ref()?
            .first()
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
            });

        let album_field = album
            .map(|album| Album {
                name: album.title.clone(),
                id: album.browse_id.clone(),
            })
            .or(album_from_column);

        let mut artists =
            PageHelper::extract_runs(&renderer.flex_columns, "MUSIC_PAGE_TYPE_ARTIST")
                .iter()
                .map(|run| Artist {
                    name: run.text.clone(),
                    id: run
                        .navigation_endpoint
                        .as_ref()
                        .and_then(|endpoint| endpoint.browse_endpoint.as_ref())
                        .map(|browse| browse.browse_id.clone()),
                })
                .collect::<Vec<_>>();
        if artists.is_empty() {
            artists = album
                .map(|album| album.artists.clone().unwrap_or_default())
                .unwrap_or_default();
        }

        Some(SongItem {
            id: renderer.playlist_item_data.as_ref()?.video_id.clone(),
            title: PageHelper::extract_runs(&renderer.flex_columns, "MUSIC_VIDEO")
                .first()?
                .text
                .clone(),
            artists,
            album: album_field,
            duration: renderer
                .fixed_columns
                .as_ref()?
                .first()?
                .music_responsive_list_item_flex_column_renderer
                .text
                .as_ref()?
                .first()
                .and_then(|run| parse_time(&run.text)),
            music_video_type: renderer.music_video_type(),
            thumbnail: renderer
                .thumbnail
                .as_ref()?
                .music_thumbnail_renderer
                .as_ref()?
                .get_thumbnail_url()
                .map(str::to_owned)
                .or_else(|| album.map(|album| album.thumbnail.clone()))?,
            explicit: crate::pages::home::explicit_badge(&renderer.badges),
            library_add_token: library_tokens.add_token,
            library_remove_token: library_tokens.remove_token,
            ..Default::default()
        })
    }
}
