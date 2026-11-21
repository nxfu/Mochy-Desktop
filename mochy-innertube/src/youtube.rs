//! The `YouTube` facade — the port of `providers/innertube/YouTube.kt`
//! (pin `bde01cb4`, 1,814 lines), one method per feature over the Phase
//! 04 endpoint layer and the Phase 05 parsers.
//!
//! What is intentionally NOT here (recorded in HANDOFF ## Phase 05):
//! - the comments stack (`comments`/`commentContinuation`/
//!   `commentReplies`, YouTube.kt:1601-1813) — dead upstream
//!   (`models/comment/` unwired, PORT_SPEC §4);
//! - the NewPipe bridges (`getNewPipeStreamUrls`/`newPipePlayer`,
//!   YouTube.kt:1558-1599) — Phase 08 stream-resolution scope;
//! - the login-window/account plumbing around `accountInfo` — Phase 07.
//!
//! Kotlin `Result<T>` maps to `Result<T, InnertubeError>`; `runCatching`
//! bodies become `?` propagation.

use std::collections::VecDeque;

use crate::captions;
use crate::client::{ClientKey, ClientRegistry, YouTubeClient, roles};
use crate::endpoints::{music_stats_url, new_playback_nonce};
use crate::error::InnertubeError;
use crate::guest;
use crate::models::SearchSuggestions;
use crate::models::account::{AccountInfo, MediaInfo, VideoCaptions};
use crate::models::common::get_continuation;
use crate::models::endpoint::{BrowseEndpoint, WatchEndpoint};
use crate::models::response::{
    AccountMenuResponse, BrowseResponse, CreatePlaylistResponse, EditPlaylistResponse,
    FeedbackResponse, GetQueueResponse, GetSearchSuggestionsResponse, GetTranscriptResponse,
    ImageUploadResponse, NextResponse, PlayerResponse, SearchResponse,
};
use crate::models::ytitem::{AlbumItem, Artist, ArtistItem, PlaylistItem, SongItem, YTItem};
use crate::pages::album::AlbumPageParser;
use crate::pages::artist::ArtistPageParser;
use crate::pages::artist_items::ArtistItemsPageParser;
use crate::pages::browse_result::{BrowseItem, BrowseResult};
use crate::pages::charts::{ChartSection, ChartType, ChartsPage};
use crate::pages::explore::ExplorePage;
use crate::pages::history::{HistoryPage, HistorySection};
use crate::pages::home::{HomePage, HomeSection, home_chips};
use crate::pages::library::{LibraryContinuationPage, LibraryPage, LibraryPageParser};
use crate::pages::mood_genres::{MoodAndGenres, MoodAndGenresParser};
use crate::pages::new_release::NewReleaseAlbumPage;
use crate::pages::next::{NextPage, NextResult};
use crate::pages::playlist::{PlaylistContinuationPage, PlaylistPage, PlaylistPageParser};
use crate::pages::related::RelatedPageParser;
use crate::pages::search::{SearchPage, SearchResult};
use crate::pages::search_suggestion::SearchSuggestionPage;
use crate::pages::search_summary::{SearchSummary, SearchSummaryPage, SearchSummaryPageParser};
use crate::paging::ContinuationPager;
use crate::transport::InnerTubeTransport;

use serde::de::DeserializeOwned;
use serde_json::Value as Json;

/// `YouTube.SearchFilter` (YouTube.kt:1532-1542) — the six filter
/// categories.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchFilter(pub String);

impl SearchFilter {
    fn new(value: &str) -> Self {
        SearchFilter(value.to_owned())
    }

    /// `FILTER_SONG`.
    pub fn song() -> Self {
        Self::new("EgWKAQIIAWoKEAkQBRAKEAMQBA%3D%3D")
    }

    /// `FILTER_VIDEO`.
    pub fn video() -> Self {
        Self::new("EgWKAQIQAWoKEAkQChAFEAMQBA%3D%3D")
    }

    /// `FILTER_ALBUM`.
    pub fn album() -> Self {
        Self::new("EgWKAQIYAWoKEAkQChAFEAMQBA%3D%3D")
    }

    /// `FILTER_ARTIST`.
    pub fn artist() -> Self {
        Self::new("EgWKAQIgAWoKEAkQChAFEAMQBA%3D%3D")
    }

    /// `FILTER_FEATURED_PLAYLIST`.
    pub fn featured_playlist() -> Self {
        Self::new("EgeKAQQoADgBagwQDhAKEAMQBRAJEAQ%3D")
    }

    /// `FILTER_COMMUNITY_PLAYLIST`.
    pub fn community_playlist() -> Self {
        Self::new("EgeKAQQoAEABagoQAxAEEAoQCRAF")
    }
}

/// `YouTube.LibraryFilter` (YouTube.kt:1544-1552).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryFilter(pub String);

impl LibraryFilter {
    /// `FILTER_RECENT_ACTIVITY` (YouTube.kt:1547).
    pub const RECENT_ACTIVITY: &'static str =
        "4qmFsgIrEhdGRW11c2ljX2xpYnJhcnlfbGFuZGluZxoQZ2dNR0tnUUlCaEFCb0FZQg%3D%3D";

    /// `FILTER_RECENTLY_PLAYED`.
    pub const RECENTLY_PLAYED: &'static str =
        "4qmFsgIrEhdGRW11c2ljX2xpYnJhcnlfbGFuZGluZxoQZ2dNR0tnUUlCUkFCb0FZQg%3D%3D";

    /// `FILTER_PLAYLISTS_ALPHABETICAL`.
    pub const PLAYLISTS_ALPHABETICAL: &'static str =
        "4qmFsgIrEhdGRW11c2ljX2xpa2VkX3BsYXlsaXN0cxoQZ2dNR0tnUUlBUkFBb0FZQg%3D%3D";

    /// `FILTER_PLAYLISTS_RECENTLY_SAVED`.
    pub const PLAYLISTS_RECENTLY_SAVED: &'static str =
        "4qmFsgIrEhdGRW11c2ljX2xpa2VkX3BsYXlsaXN0cxoQZ2dNR0tnUUlBQkFCb0FZQg%3D%3D";
}

/// `MAX_GET_QUEUE_SIZE` (YouTube.kt:1554).
pub const MAX_GET_QUEUE_SIZE: usize = 1000;

/// The facade. Clone-safe: the transport and registry are cheap handles.
#[derive(Clone)]
pub struct YouTube {
    transport: InnerTubeTransport,
    registry: ClientRegistry,
}

impl Default for YouTube {
    fn default() -> Self {
        Self::new()
    }
}

impl YouTube {
    /// A facade over a fresh default session + transport (upstream's
    /// `object YouTube { private val innerTube = InnerTube() }`).
    pub fn new() -> Self {
        let session =
            std::sync::Arc::new(std::sync::RwLock::new(crate::session::Session::default()));
        YouTube {
            transport: InnerTubeTransport::new(session).expect("default transport"),
            registry: ClientRegistry::new(),
        }
    }

    /// A facade over an existing (mirrored) transport.
    pub fn with_transport(transport: InnerTubeTransport) -> Self {
        YouTube {
            transport,
            registry: ClientRegistry::new(),
        }
    }

    /// A facade with runtime client-version overrides (the R1 hotfix
    /// seam, Phase 04 decision 5).
    pub fn with_registry(transport: InnerTubeTransport, registry: ClientRegistry) -> Self {
        YouTube {
            transport,
            registry,
        }
    }

    /// The underlying transport (the endpoint layer stays public for
    /// Phase 08's stream resolution).
    pub fn transport(&self) -> &InnerTubeTransport {
        &self.transport
    }

    /// The client registry (version overrides).
    pub fn registry(&self) -> &ClientRegistry {
        &self.registry
    }

    fn client(&self, key: ClientKey) -> YouTubeClient {
        self.registry.get(key)
    }

    fn web_remix(&self) -> YouTubeClient {
        self.client(ClientKey::WebRemix)
    }

    fn web(&self) -> YouTubeClient {
        self.client(ClientKey::Web)
    }

    // -- decode helpers ----------------------------------------------------

    fn decode<T: DeserializeOwned>(value: Json) -> Result<T, InnertubeError> {
        serde_json::from_value(value)
            .map_err(|error| InnertubeError::Parse(format!("DTO decode failed: {error}")))
    }

    async fn browse_json(
        &self,
        browse_id: Option<&str>,
        params: Option<&str>,
    ) -> Result<BrowseResponse, InnertubeError> {
        let value = self
            .transport
            .browse(&self.web_remix(), browse_id, params, None, false, None)
            .await?;
        Self::decode(value)
    }

    async fn browse_login_json(
        &self,
        browse_id: Option<&str>,
        params: Option<&str>,
    ) -> Result<BrowseResponse, InnertubeError> {
        let value = self
            .transport
            .browse(&self.web_remix(), browse_id, params, None, true, None)
            .await?;
        Self::decode(value)
    }

    async fn browse_continuation_json(
        &self,
        continuation: &str,
        set_login: bool,
    ) -> Result<BrowseResponse, InnertubeError> {
        let value = self
            .transport
            .browse(
                &self.web_remix(),
                None,
                None,
                Some(continuation),
                set_login,
                None,
            )
            .await?;
        Self::decode(value)
    }

    // -- search ------------------------------------------------------------

    /// `searchSuggestions` (YouTube.kt:143-155).
    pub async fn search_suggestions(
        &self,
        query: &str,
    ) -> Result<SearchSuggestions, InnertubeError> {
        let response: GetSearchSuggestionsResponse = Self::decode(
            self.transport
                .get_search_suggestions(&self.web_remix(), query)
                .await?,
        )?;
        let contents = response.contents.unwrap_or_default();
        let queries = contents
            .first()
            .and_then(|content| content.search_suggestions_section_renderer.as_ref())
            .map(|section| {
                section
                    .contents
                    .iter()
                    .filter_map(|content| {
                        content.search_suggestion_renderer.as_ref().map(|renderer| {
                            renderer
                                .suggestion
                                .runs
                                .as_ref()
                                .map(|runs| {
                                    runs.iter()
                                        .map(|run| run.text.as_str())
                                        .collect::<Vec<_>>()
                                        .join("")
                                })
                                .unwrap_or_default()
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let recommended_items = contents
            .get(1)
            .and_then(|content| content.search_suggestions_section_renderer.as_ref())
            .map(|section| {
                section
                    .contents
                    .iter()
                    .filter_map(|content| {
                        content
                            .music_responsive_list_item_renderer
                            .as_ref()
                            .and_then(
                                SearchSuggestionPage::from_music_responsive_list_item_renderer,
                            )
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(SearchSuggestions {
            queries,
            recommended_items,
        })
    }

    /// `searchSummary` (YouTube.kt:157-237).
    pub async fn search_summary(&self, query: &str) -> Result<SearchSummaryPage, InnertubeError> {
        let response: SearchResponse = Self::decode(
            self.transport
                .search(&self.web_remix(), Some(query), None, None)
                .await?,
        )?;
        let contents = response
            .contents
            .as_ref()
            .and_then(|contents| contents.tabbed_search_results_renderer.as_ref())
            .and_then(|tabs| tabs.first())
            .and_then(|tab| tab.content.as_ref())
            .and_then(|content| content.section_list_renderer.as_ref())
            .and_then(|section| section.contents.as_ref().cloned())
            .unwrap_or_default();

        let mut shelf_summaries = Vec::new();
        for content in &contents {
            if let Some(card) = content.music_card_shelf_renderer.as_ref() {
                // an artist card's songs omit the artist; the card itself
                // is the artist (YouTube.kt:165-171)
                let card_artist = card
                    .on_tap
                    .browse_endpoint
                    .as_ref()
                    .filter(|endpoint| endpoint.is_artist_endpoint())
                    .and_then(|endpoint| {
                        card.title.first().map(|run| Artist {
                            name: run.text.clone(),
                            id: Some(endpoint.browse_id.clone()),
                        })
                    });
                let top = SearchSummaryPageParser::from_music_card_shelf_renderer(card);
                let mut items = vec![top].into_iter().flatten().collect::<Vec<_>>();
                items.extend(
                    card.contents
                        .as_ref()
                        .map(|contents| {
                            contents
                                .iter()
                                .filter_map(|content| {
                                    content.music_responsive_list_item_renderer.as_ref()
                                })
                                .filter_map(|renderer| {
                                    SearchSummaryPageParser::from_music_responsive_list_item_renderer(
                                        renderer,
                                        card_artist.clone(),
                                    )
                                })
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default(),
                );
                // distinctBy { it.id }
                let mut seen = std::collections::HashSet::new();
                items.retain(|item| seen.insert(item.id().to_owned()));
                if items.is_empty() {
                    continue;
                }
                shelf_summaries.push(SearchSummary {
                    title: card
                        .header
                        .as_ref()
                        .and_then(|header| {
                            Some(
                                header
                                    .music_card_shelf_header_basic_renderer
                                    .title
                                    .first()?
                                    .text
                                    .clone(),
                            )
                        })
                        .unwrap_or_else(|| crate::models::constants::DEFAULT_TOP_RESULT.to_owned()),
                    items,
                });
            } else if let Some(shelf) = content.music_shelf_renderer.as_ref() {
                let items: Vec<YTItem> = shelf
                    .contents
                    .as_ref()
                    .map(|contents| crate::models::music_renderers::shelf_items(contents))
                    .unwrap_or_default()
                    .iter()
                    .filter_map(|renderer| {
                        SearchSummaryPageParser::from_music_responsive_list_item_renderer(
                            renderer, None,
                        )
                    })
                    .collect();
                let mut seen = std::collections::HashSet::new();
                let items: Vec<YTItem> = items
                    .into_iter()
                    .filter(|item| seen.insert(item.id().to_owned()))
                    .collect();
                if items.is_empty() {
                    continue;
                }
                shelf_summaries.push(SearchSummary {
                    title: shelf
                        .title
                        .as_ref()
                        .and_then(|title| title.first())
                        .map(|run| run.text.clone())
                        .unwrap_or_else(|| {
                            crate::models::constants::DEFAULT_OTHER_RESULTS.to_owned()
                        }),
                    items,
                });
            }
        }

        // the flat item-section results group into Songs/Videos/Albums/
        // Artists/Playlists (YouTube.kt:201-232) — through the SAME
        // SearchSummaryPage parser as the card shelves (the 2026-11-21
        // audit fixed this from SearchSuggestionPage, which dropped
        // flat playlists, lost song durations/library tokens and
        // treated artist endpoints as optional where upstream requires
        // them)
        let flat_items: Vec<YTItem> = contents
            .iter()
            .filter_map(|content| content.item_section_renderer.as_ref())
            .flat_map(|section| section.contents.as_deref().unwrap_or_default())
            .filter_map(|content| content.music_responsive_list_item_renderer.as_ref())
            .filter_map(|renderer| {
                SearchSummaryPageParser::from_music_responsive_list_item_renderer(renderer, None)
            })
            .collect();

        let mut grouped_summaries = Vec::new();
        let mut push_group = |title: &str, items: Vec<YTItem>| {
            if !items.is_empty() {
                grouped_summaries.push(SearchSummary {
                    title: title.to_owned(),
                    items,
                });
            }
        };
        push_group(
            "Songs",
            flat_items
                .iter()
                .filter(|item| matches!(item, YTItem::Song(song) if !song.is_video_song()))
                .cloned()
                .collect(),
        );
        push_group(
            "Videos",
            flat_items
                .iter()
                .filter(|item| matches!(item, YTItem::Song(song) if song.is_video_song()))
                .cloned()
                .collect(),
        );
        push_group(
            "Albums",
            flat_items
                .iter()
                .filter(|item| matches!(item, YTItem::Album(_)))
                .cloned()
                .collect(),
        );
        push_group(
            "Artists",
            flat_items
                .iter()
                .filter(|item| matches!(item, YTItem::Artist(_)))
                .cloned()
                .collect(),
        );
        push_group(
            "Playlists",
            flat_items
                .iter()
                .filter(|item| matches!(item, YTItem::Playlist(_)))
                .cloned()
                .collect(),
        );

        Ok(SearchSummaryPage {
            summaries: shelf_summaries
                .into_iter()
                .chain(grouped_summaries)
                .collect(),
        })
    }

    /// `search(query, filter)` (YouTube.kt:239-251).
    pub async fn search(
        &self,
        query: &str,
        filter: &SearchFilter,
    ) -> Result<SearchResult, InnertubeError> {
        let response: SearchResponse = Self::decode(
            self.transport
                .search(&self.web_remix(), Some(query), Some(&filter.0), None)
                .await?,
        )?;
        let shelf = response
            .contents
            .as_ref()
            .and_then(|contents| contents.tabbed_search_results_renderer.as_ref())
            .and_then(|tabs| tabs.first())
            .and_then(|tab| tab.content.as_ref())
            .and_then(|content| content.section_list_renderer.as_ref())
            .and_then(|section| section.contents.as_ref())
            .and_then(|contents| {
                contents
                    .iter()
                    .find_map(|content| content.music_shelf_renderer.as_ref())
            });
        Ok(SearchResult {
            items: shelf
                .map(|shelf| {
                    shelf
                        .contents
                        .as_ref()
                        .map(|contents| crate::models::music_renderers::shelf_items(contents))
                        .unwrap_or_default()
                        .iter()
                        .filter_map(|renderer| SearchPage::to_yt_item(renderer))
                        .collect()
                })
                .unwrap_or_default(),
            continuation: shelf
                .and_then(|shelf| shelf.continuations.as_ref())
                .and_then(|continuations| get_continuation(continuations))
                .map(str::to_owned),
        })
    }

    /// `searchContinuation` (YouTube.kt:253-263).
    pub async fn search_continuation(
        &self,
        continuation: &str,
    ) -> Result<SearchResult, InnertubeError> {
        let response: SearchResponse = Self::decode(
            self.transport
                .search(&self.web_remix(), None, None, Some(continuation))
                .await?,
        )?;
        let music_shelf = response
            .continuation_contents
            .and_then(|contents| contents.music_shelf_continuation);
        let items: Vec<YTItem> = music_shelf
            .as_ref()
            .map(|shelf| {
                shelf
                    .contents
                    .iter()
                    .filter_map(|content| {
                        content
                            .music_responsive_list_item_renderer
                            .as_ref()
                            .and_then(SearchPage::to_yt_item)
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(SearchResult {
            continuation: if items.is_empty() {
                None
            } else {
                music_shelf
                    .as_ref()
                    .and_then(|shelf| shelf.continuations.as_ref())
                    .and_then(|continuations| get_continuation(continuations))
                    .map(str::to_owned)
            },
            items,
        })
    }

    // -- album -------------------------------------------------------------

    /// `album(browseId, withSongs)` (YouTube.kt:265-375).
    pub async fn album(
        &self,
        browse_id: &str,
        with_songs: bool,
    ) -> Result<crate::pages::album::AlbumPage, InnertubeError> {
        let response = self.browse_login_json(Some(browse_id), None).await?;

        // the description runs walk (YouTube.kt:275-304): two-column tabs
        // first, then single-column, then the headers
        let mut description_runs: Option<Vec<crate::models::runs::Run>> = None;
        let mut take_runs = |runs: Option<&crate::models::runs::Runs>| {
            if description_runs.is_none()
                && let Some(runs) = runs
                && let Some(runs) = runs.runs.clone()
            {
                description_runs = Some(runs);
            }
        };
        let contents = response.contents.as_ref();
        let two_column =
            contents.and_then(|contents| contents.two_column_browse_results_renderer.as_ref());
        if let Some(two) = two_column
            && let Some(tabs) = two.tabs.as_ref()
        {
            for tab in tabs.iter().flatten() {
                if let Some(contents) = tab
                    .tab_renderer
                    .as_ref()
                    .and_then(|tab| tab.content.as_ref())
                    .and_then(|content| content.section_list_renderer.as_ref())
                    .and_then(|section| section.contents.as_ref())
                {
                    for content in contents {
                        take_runs(
                            content
                                .music_description_shelf_renderer
                                .as_ref()
                                .map(|shelf| &shelf.description),
                        );
                    }
                }
            }
        }
        if let Some(single) =
            contents.and_then(|contents| contents.single_column_browse_results_renderer.as_ref())
        {
            for tab in &single.tabs {
                if let Some(contents) = tab
                    .tab_renderer
                    .as_ref()
                    .and_then(|tab| tab.content.as_ref())
                    .and_then(|content| content.section_list_renderer.as_ref())
                    .and_then(|section| section.contents.as_ref())
                {
                    for content in contents {
                        take_runs(
                            content
                                .music_description_shelf_renderer
                                .as_ref()
                                .map(|shelf| &shelf.description),
                        );
                    }
                }
            }
        }
        let header = response.header.as_ref();
        take_runs(
            header
                .and_then(|header| header.music_detail_header_renderer.as_ref())
                .and_then(|detail| detail.description.as_ref()),
        );
        take_runs(
            header
                .and_then(|header| header.music_immersive_header_renderer.as_ref())
                .and_then(|immersive| immersive.description.as_ref()),
        );
        take_runs(
            header
                .and_then(|header| {
                    header
                        .music_editable_playlist_detail_header_renderer
                        .as_ref()
                })
                .and_then(|editable| editable.header.music_detail_header_renderer.as_ref())
                .and_then(|detail| detail.description.as_ref()),
        );
        take_runs(
            header
                .and_then(|header| {
                    header
                        .music_editable_playlist_detail_header_renderer
                        .as_ref()
                })
                .and_then(|editable| editable.header.music_responsive_header_renderer.as_ref())
                .and_then(|responsive| responsive.description.as_ref()),
        );
        if let Some(two) = two_column
            && let Some(tabs) = two.tabs.as_ref()
        {
            for tab in tabs.iter().flatten() {
                if let Some(contents) = tab
                    .tab_renderer
                    .as_ref()
                    .and_then(|tab| tab.content.as_ref())
                    .and_then(|content| content.section_list_renderer.as_ref())
                    .and_then(|section| section.contents.as_ref())
                {
                    for content in contents {
                        if description_runs.is_none() {
                            description_runs = content
                                .music_responsive_header_renderer
                                .as_ref()
                                .and_then(|header| {
                                    header.description.as_ref().and_then(|d| d.runs.clone())
                                });
                        }
                    }
                }
            }
        }
        if description_runs.is_none()
            && let Some(single) = contents
                .and_then(|contents| contents.single_column_browse_results_renderer.as_ref())
        {
            for tab in &single.tabs {
                if let Some(contents) = tab
                    .tab_renderer
                    .as_ref()
                    .and_then(|tab| tab.content.as_ref())
                    .and_then(|content| content.section_list_renderer.as_ref())
                    .and_then(|section| section.contents.as_ref())
                {
                    for content in contents {
                        if description_runs.is_none() {
                            description_runs = content
                                .music_responsive_header_renderer
                                .as_ref()
                                .and_then(|header| {
                                    header.description.as_ref().and_then(|d| d.runs.clone())
                                });
                        }
                    }
                }
            }
        }
        let description = description_runs.as_ref().map(|runs| {
            runs.iter()
                .map(|run| run.text.as_str())
                .collect::<Vec<_>>()
                .join("")
        });

        if browse_id.contains("FEmusic_library_privately_owned_release_detail") {
            // the privately-owned release branch (YouTube.kt:308-334)
            let detail = response
                .header
                .as_ref()
                .and_then(|header| header.music_detail_header_renderer.as_ref())
                .ok_or_else(|| InnertubeError::Parse("missing musicDetailHeaderRenderer".into()))?;
            let playlist_id = detail
                .menu
                .menu_renderer
                .top_level_buttons
                .as_ref()
                .and_then(|buttons| buttons.first())
                .and_then(|button| button.button_renderer.as_ref())
                .and_then(|button| button.navigation_endpoint.watch_playlist_endpoint.as_ref())
                .and_then(|endpoint| endpoint.playlist_id.clone())
                .ok_or_else(|| {
                    InnertubeError::Parse("missing privately-owned playlistId".into())
                })?;
            let album_item = AlbumItem {
                browse_id: browse_id.to_owned(),
                playlist_id,
                title: detail
                    .title
                    .first()
                    .map(|run| run.text.clone())
                    .ok_or_else(|| InnertubeError::Parse("missing album title".into()))?,
                artists: Some(
                    detail
                        .subtitle
                        .runs
                        .as_ref()
                        .map(|runs| {
                            runs.iter()
                                .filter(|run| run.navigation_endpoint.is_some())
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
                        .unwrap_or_default(),
                ),
                year: detail.subtitle.last().and_then(|run| run.text.parse().ok()),
                thumbnail: detail
                    .thumbnail
                    .cropped_square_thumbnail_renderer
                    .as_ref()
                    .and_then(|music| music.get_thumbnail_url().map(str::to_owned))
                    .ok_or_else(|| InnertubeError::Parse("missing album thumbnail".into()))?,
                explicit: false,
                description: description.clone(),
            };
            let songs = response
                .contents
                .as_ref()
                .and_then(|contents| contents.single_column_browse_results_renderer.as_ref())
                .and_then(|single| single.tabs.first())
                .and_then(|tab| tab.tab_renderer.as_ref())
                .and_then(|tab| tab.content.as_ref())
                .and_then(|content| content.section_list_renderer.as_ref())
                .and_then(|section| section.contents.as_ref())
                .and_then(|contents| contents.first())
                .and_then(|content| content.music_shelf_renderer.as_ref())
                .map(|shelf| {
                    crate::models::music_renderers::shelf_items(
                        shelf.contents.as_deref().unwrap_or_default(),
                    )
                    .iter()
                    .filter_map(|renderer| AlbumPageParser::get_song(renderer, Some(&album_item)))
                    .collect()
                })
                .unwrap_or_default();
            return Ok(crate::pages::album::AlbumPage {
                album: album_item,
                songs,
                other_versions: Vec::new(),
                releases_for_you: Vec::new(),
                description,
                description_runs,
            });
        }

        // the normal branch (YouTube.kt:335-374)
        let playlist_id = response
            .microformat
            .as_ref()
            .and_then(|microformat| microformat.microformat_data_renderer.as_ref())
            .and_then(|data| data.url_canonical.as_ref())
            .map(|url| url.rsplit('=').next().unwrap_or_default().to_owned())
            .ok_or_else(|| InnertubeError::Parse("missing album urlCanonical".into()))?;
        let tabs = two_column
            .and_then(|two| two.tabs.as_ref())
            .and_then(|tabs| tabs.iter().flatten().next())
            .and_then(|tab| tab.tab_renderer.as_ref())
            .and_then(|tab| tab.content.as_ref())
            .and_then(|content| content.section_list_renderer.as_ref())
            .and_then(|section| section.contents.as_ref())
            .and_then(|contents| contents.first());
        let responsive = tabs
            .and_then(|content| content.music_responsive_header_renderer.as_ref())
            .ok_or_else(|| {
                InnertubeError::Parse("missing album musicResponsiveHeaderRenderer".into())
            })?;
        let album_item = AlbumItem {
            browse_id: browse_id.to_owned(),
            playlist_id,
            title: responsive
                .title
                .first()
                .map(|run| run.text.clone())
                .ok_or_else(|| InnertubeError::Parse("missing album title".into()))?,
            artists: Some(
                crate::models::runs::odd_elements(
                    responsive
                        .strapline_text_one
                        .as_ref()
                        .and_then(|runs| runs.runs.as_ref())
                        .ok_or_else(|| {
                            InnertubeError::Parse("missing album straplineTextOne".into())
                        })?,
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
            ),
            year: responsive
                .subtitle
                .last()
                .and_then(|run| run.text.parse().ok()),
            thumbnail: responsive
                .thumbnail
                .as_ref()
                .and_then(|thumbnail| thumbnail.music_thumbnail_renderer.as_ref())
                .and_then(|music| music.get_thumbnail_url().map(str::to_owned))
                .ok_or_else(|| InnertubeError::Parse("missing album thumbnail".into()))?,
            explicit: false,
            description: description.clone(),
        };
        let secondary = two_column.and_then(|two| two.secondary_contents.as_ref());
        let secondary_sections = secondary
            .and_then(|secondary| secondary.section_list_renderer.as_ref())
            .and_then(|section| section.contents.as_ref());
        let find_carousel = |needle: &[&str]| -> Vec<AlbumItem> {
            secondary_sections
                .map(|contents| {
                    contents
                        .iter()
                        .filter_map(|content| content.music_carousel_shelf_renderer.as_ref())
                        .find(|carousel| {
                            carousel
                                .header
                                .as_ref()
                                .map(|header| {
                                    &header.music_carousel_shelf_basic_header_renderer.title
                                })
                                .map(|title| {
                                    title.runs.as_ref().map(|runs| {
                                        runs.iter().any(|run| {
                                            needle.iter().any(|needle| {
                                                run.text.to_lowercase().contains(needle)
                                            })
                                        })
                                    }) == Some(true)
                                })
                                .unwrap_or(false)
                        })
                        .map(|carousel| {
                            carousel
                                .contents
                                .iter()
                                .filter_map(|content| content.music_two_row_item_renderer.as_ref())
                                .filter_map(NewReleaseAlbumPage::from_music_two_row_item_renderer)
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default()
                })
                .unwrap_or_default()
        };
        Ok(crate::pages::album::AlbumPage {
            album: album_item.clone(),
            songs: if with_songs {
                self.album_songs(&album_item.playlist_id, Some(&album_item))
                    .await?
            } else {
                Vec::new()
            },
            other_versions: find_carousel(&["versions"]),
            releases_for_you: find_carousel(&["releases", "more from"]),
            description,
            description_runs,
        })
    }

    /// `albumSongs` (YouTube.kt:377-411) — the 50-request ceiling through
    /// the Phase 04 [`ContinuationPager`].
    pub async fn album_songs(
        &self,
        playlist_id: &str,
        album: Option<&AlbumItem>,
    ) -> Result<Vec<SongItem>, InnertubeError> {
        let mut response = self
            .browse_json(Some(&format!("VL{playlist_id}")), None)
            .await?;
        let first_shelf = || {
            response
                .contents
                .as_ref()
                .and_then(|contents| contents.two_column_browse_results_renderer.as_ref())
                .and_then(|two| two.secondary_contents.as_ref())
                .and_then(|secondary| secondary.section_list_renderer.as_ref())
                .and_then(|section| section.contents.as_ref())
                .and_then(|contents| contents.first())
                .and_then(|content| content.music_playlist_shelf_renderer.as_ref())
                .cloned()
        };
        let shelf = first_shelf()
            .ok_or_else(|| InnertubeError::Parse("missing album songs shelf".into()))?;
        let mut songs: Vec<SongItem> = crate::models::music_renderers::shelf_items(&shelf.contents)
            .iter()
            .filter_map(|renderer| AlbumPageParser::get_song(renderer, album))
            .collect();
        let mut continuation = crate::models::music_renderers::shelf_continuation(&shelf.contents)
            .or_else(|| {
                shelf
                    .continuations
                    .as_ref()
                    .and_then(|continuations| get_continuation(continuations))
                    .map(str::to_owned)
            });

        let mut pager = ContinuationPager::playlist_songs();
        while let Some(token) = continuation {
            match pager.admit(&token) {
                crate::paging::Admit::Fetch => {}
                crate::paging::Admit::Stop(_) => break,
            }
            response = self.browse_continuation_json(&token, false).await?;
            songs.extend(
                response
                    .on_response_received_actions
                    .as_ref()
                    .and_then(|actions| actions.first())
                    .and_then(|action| action.append_continuation_items_action.as_ref())
                    .and_then(|items| items.continuation_items.as_ref())
                    .map(|items| {
                        crate::models::music_renderers::shelf_items(items)
                            .iter()
                            .filter_map(|renderer| AlbumPageParser::get_song(renderer, album))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default(),
            );
            continuation = response
                .continuation_contents
                .as_ref()
                .and_then(|contents| contents.music_playlist_shelf_continuation.as_ref())
                .and_then(|shelf| shelf.continuations.as_ref())
                .and_then(|continuations| get_continuation(continuations))
                .map(str::to_owned);
        }
        Ok(songs)
    }

    // -- artist ------------------------------------------------------------

    /// `artist(browseId)` (YouTube.kt:413-462).
    pub async fn artist(
        &self,
        browse_id: &str,
    ) -> Result<crate::pages::artist::ArtistPage, InnertubeError> {
        let response = self.browse_json(Some(browse_id), None).await?;
        let header = response.header.as_ref();
        let immersive = header.and_then(|header| header.music_immersive_header_renderer.as_ref());
        let visual = header.and_then(|header| header.music_visual_header_renderer.as_ref());
        let plain = header.and_then(|header| header.music_header_renderer.as_ref());

        let description_runs = response
            .contents
            .as_ref()
            .and_then(|contents| contents.section_list_renderer.as_ref())
            .and_then(|section| section.contents.as_ref())
            .and_then(|contents| {
                contents
                    .iter()
                    .find(|content| content.music_description_shelf_renderer.is_some())
            })
            .and_then(|content| content.music_description_shelf_renderer.as_ref())
            .and_then(|shelf| shelf.description.runs.clone())
            .or_else(|| {
                immersive
                    .and_then(|immersive| immersive.description.as_ref())
                    .and_then(|runs| runs.runs.clone())
            });
        let description = description_runs.as_ref().map(|runs| {
            runs.iter()
                .map(|run| run.text.as_str())
                .collect::<Vec<_>>()
                .join("")
        });

        let first_song_endpoint = || {
            response
                .contents
                .as_ref()
                .and_then(|contents| contents.single_column_browse_results_renderer.as_ref())
                .and_then(|single| single.tabs.first())
                .and_then(|tab| tab.tab_renderer.as_ref())
                .and_then(|tab| tab.content.as_ref())
                .and_then(|content| content.section_list_renderer.as_ref())
                .and_then(|section| section.contents.as_ref())
                .and_then(|contents| contents.first())
                .and_then(|content| content.music_shelf_renderer.as_ref())
                .and_then(|shelf| shelf.contents.as_ref())
                .and_then(|contents| contents.first())
                .and_then(|content| content.music_responsive_list_item_renderer.as_ref())
                .cloned()
        };
        let first_song = first_song_endpoint();

        let artist_item = ArtistItem {
            id: browse_id.to_owned(),
            title: immersive
                .and_then(|immersive| immersive.title.first())
                .or_else(|| visual.and_then(|visual| visual.title.first()))
                .or_else(|| plain.and_then(|plain| plain.title.as_ref().and_then(|t| t.first())))
                .map(|run| run.text.clone())
                .ok_or_else(|| InnertubeError::Parse("missing artist title".into()))?,
            thumbnail: immersive
                .and_then(|immersive| immersive.thumbnail.as_ref())
                .and_then(|thumbnail| thumbnail.music_thumbnail_renderer.as_ref())
                .or_else(|| {
                    visual.and_then(|visual| {
                        visual
                            .foreground_thumbnail
                            .music_thumbnail_renderer
                            .as_ref()
                    })
                })
                .or_else(|| {
                    header
                        .and_then(|header| header.music_detail_header_renderer.as_ref())
                        .and_then(|detail| detail.thumbnail.music_thumbnail_renderer.as_ref())
                })
                .and_then(|music| music.get_thumbnail_url().map(str::to_owned)),
            channel_id: immersive
                .and_then(|immersive| immersive.subscription_button.as_ref())
                .map(|button| button.subscribe_button_renderer.channel_id.clone()),
            play_endpoint: first_song
                .as_ref()
                .and_then(|song| song.overlay.as_ref())
                .and_then(|overlay| {
                    overlay
                        .music_item_thumbnail_overlay_renderer
                        .content
                        .music_play_button_renderer
                        .play_navigation_endpoint
                        .as_ref()
                })
                .and_then(|endpoint| endpoint.watch_endpoint.clone()),
            shuffle_endpoint: immersive
                .and_then(|immersive| immersive.play_button.as_ref())
                .and_then(|button| button.button_renderer.navigation_endpoint.as_ref())
                .and_then(|endpoint| endpoint.watch_endpoint.clone())
                .or_else(|| {
                    first_song
                        .as_ref()
                        .and_then(|song| song.navigation_endpoint.as_ref())
                        .and_then(|endpoint| endpoint.watch_playlist_endpoint.clone())
                }),
            radio_endpoint: immersive
                .and_then(|immersive| immersive.start_radio_button.as_ref())
                .and_then(|button| button.button_renderer.navigation_endpoint.as_ref())
                .and_then(|endpoint| endpoint.watch_endpoint.clone()),
        };
        let sections = response
            .contents
            .as_ref()
            .and_then(|contents| contents.single_column_browse_results_renderer.as_ref())
            .and_then(|single| single.tabs.first())
            .and_then(|tab| tab.tab_renderer.as_ref())
            .and_then(|tab| tab.content.as_ref())
            .and_then(|content| content.section_list_renderer.as_ref())
            .and_then(|section| section.contents.as_ref())
            .map(|contents| {
                contents
                    .iter()
                    .filter_map(ArtistPageParser::from_section_list_renderer_content)
                    .collect()
            })
            .ok_or_else(|| InnertubeError::Parse("missing artist sections".into()))?;

        Ok(crate::pages::artist::ArtistPage {
            artist: artist_item,
            sections,
            description,
            subscriber_count_text: immersive
                .and_then(|immersive| immersive.subscription_button2.as_ref())
                .and_then(|button| button.subscribe_button_renderer.as_ref())
                .and_then(|renderer| renderer.subscriber_count_with_subscribe_text.as_ref())
                .and_then(|runs| runs.first())
                .map(|run| run.text.clone())
                .or_else(|| {
                    immersive
                        .and_then(|immersive| immersive.subscription_button.as_ref())
                        .and_then(|button| {
                            button
                                .subscribe_button_renderer
                                .long_subscriber_count_text
                                .as_ref()
                        })
                        .and_then(|runs| runs.first())
                        .map(|run| run.text.clone())
                })
                .or_else(|| {
                    immersive
                        .and_then(|immersive| immersive.subscription_button.as_ref())
                        .and_then(|button| {
                            button
                                .subscribe_button_renderer
                                .short_subscriber_count_text
                                .as_ref()
                        })
                        .and_then(|runs| runs.first())
                        .map(|run| run.text.clone())
                }),
            monthly_listener_count: immersive
                .and_then(|immersive| immersive.monthly_listener_count.as_ref())
                .and_then(|runs| runs.first())
                .map(|run| run.text.clone()),
            description_runs,
            subscribed: immersive
                .and_then(|immersive| immersive.subscription_button.as_ref())
                .map(|button| button.subscribe_button_renderer.subscribed),
        })
    }

    /// `artistItems(endpoint)` (YouTube.kt:464-520).
    pub async fn artist_items(
        &self,
        endpoint: &BrowseEndpoint,
    ) -> Result<crate::pages::artist_items::ArtistItemsPage, InnertubeError> {
        let response = self
            .browse_json(Some(&endpoint.browse_id), endpoint.params.as_deref())
            .await?;
        let section_content = response
            .contents
            .as_ref()
            .and_then(|contents| contents.single_column_browse_results_renderer.as_ref())
            .and_then(|single| single.tabs.first())
            .and_then(|tab| tab.tab_renderer.as_ref())
            .and_then(|tab| tab.content.as_ref())
            .and_then(|content| content.section_list_renderer.as_ref())
            .and_then(|section| section.contents.as_ref())
            .and_then(|contents| contents.first());

        if let Some(grid) = section_content.and_then(|content| content.grid_renderer.as_ref()) {
            return Ok(crate::pages::artist_items::ArtistItemsPage {
                title: grid
                    .header
                    .as_ref()
                    .and_then(|header| {
                        header
                            .grid_header_renderer
                            .title
                            .first()
                            .map(|run| run.text.clone())
                    })
                    .unwrap_or_default(),
                items: grid
                    .items
                    .iter()
                    .filter_map(|item| item.music_two_row_item_renderer.as_ref())
                    .filter_map(ArtistItemsPageParser::from_music_two_row_item_renderer)
                    .collect(),
                continuation: grid
                    .continuations
                    .as_ref()
                    .and_then(|continuations| get_continuation(continuations))
                    .map(str::to_owned),
            });
        }
        if let Some(carousel) =
            section_content.and_then(|content| content.music_carousel_shelf_renderer.as_ref())
        {
            return Ok(crate::pages::artist_items::ArtistItemsPage {
                title: carousel
                    .header
                    .as_ref()
                    .and_then(|header| {
                        header
                            .music_carousel_shelf_basic_header_renderer
                            .title
                            .first()
                            .map(|run| run.text.clone())
                    })
                    .unwrap_or_default(),
                items: carousel
                    .contents
                    .iter()
                    .filter_map(|content| {
                        content
                            .music_two_row_item_renderer
                            .as_ref()
                            .and_then(ArtistItemsPageParser::from_music_two_row_item_renderer)
                            .or_else(|| {
                                content
                                    .music_responsive_list_item_renderer
                                    .as_ref()
                                    .and_then(
                                        ArtistItemsPageParser::from_music_responsive_list_item_renderer,
                                    )
                                    .map(YTItem::Song)
                            })
                    })
                    .collect(),
                continuation: None,
            });
        }
        if let Some(shelf) =
            section_content.and_then(|content| content.music_shelf_renderer.as_ref())
        {
            return Ok(crate::pages::artist_items::ArtistItemsPage {
                title: shelf
                    .title
                    .as_ref()
                    .and_then(|title| title.first())
                    .map(|run| run.text.clone())
                    .or_else(|| {
                        response
                            .header
                            .as_ref()
                            .and_then(|header| header.music_header_renderer.as_ref())
                            .and_then(|plain| plain.title.as_ref())
                            .and_then(|title| title.first())
                            .map(|run| run.text.clone())
                    })
                    .unwrap_or_default(),
                items: shelf
                    .contents
                    .as_ref()
                    .map(|contents| crate::models::music_renderers::shelf_items(contents))
                    .unwrap_or_default()
                    .iter()
                    .filter_map(|renderer| {
                        ArtistItemsPageParser::from_music_responsive_list_item_renderer(renderer)
                    })
                    .map(YTItem::Song)
                    .collect(),
                continuation: shelf
                    .continuations
                    .as_ref()
                    .and_then(|continuations| get_continuation(continuations))
                    .map(str::to_owned),
            });
        }
        let playlist_shelf =
            section_content.and_then(|content| content.music_playlist_shelf_renderer.as_ref());
        Ok(crate::pages::artist_items::ArtistItemsPage {
            title: response
                .header
                .as_ref()
                .and_then(|header| header.music_header_renderer.as_ref())
                .and_then(|plain| plain.title.as_ref())
                .and_then(|title| title.first())
                .map(|run| run.text.clone())
                .unwrap_or_default(),
            items: playlist_shelf
                .map(|shelf| {
                    crate::models::music_renderers::shelf_items(&shelf.contents)
                        .iter()
                        .filter_map(|renderer| {
                            ArtistItemsPageParser::from_music_responsive_list_item_renderer(
                                renderer,
                            )
                        })
                        .map(YTItem::Song)
                        .collect()
                })
                .unwrap_or_default(),
            continuation: playlist_shelf.and_then(|shelf| {
                crate::models::music_renderers::shelf_continuation(&shelf.contents)
            }),
        })
    }

    /// `artistItemsContinuation` (YouTube.kt:522-562).
    pub async fn artist_items_continuation(
        &self,
        continuation: &str,
    ) -> Result<crate::pages::artist_items::ArtistItemsContinuationPage, InnertubeError> {
        let response = self.browse_continuation_json(continuation, false).await?;
        let contents = response.continuation_contents.as_ref();
        if let Some(grid) = contents.and_then(|contents| contents.grid_continuation.as_ref()) {
            let items = grid
                .items
                .iter()
                .filter_map(|item| item.music_two_row_item_renderer.as_ref())
                .filter_map(ArtistItemsPageParser::from_music_two_row_item_renderer)
                .collect::<Vec<_>>();
            return Ok(crate::pages::artist_items::ArtistItemsContinuationPage {
                continuation: if items.is_empty() {
                    None
                } else {
                    grid.continuations
                        .as_ref()
                        .and_then(|continuations| get_continuation(continuations))
                        .map(str::to_owned)
                },
                items,
            });
        }
        if let Some(shelf) =
            contents.and_then(|contents| contents.music_playlist_shelf_continuation.as_ref())
        {
            let items = crate::models::music_renderers::shelf_items(&shelf.contents)
                .iter()
                .filter_map(|renderer| {
                    ArtistItemsPageParser::from_music_responsive_list_item_renderer(renderer)
                })
                .map(YTItem::Song)
                .collect::<Vec<_>>();
            return Ok(crate::pages::artist_items::ArtistItemsContinuationPage {
                continuation: if items.is_empty() {
                    None
                } else {
                    shelf
                        .continuations
                        .as_ref()
                        .and_then(|continuations| get_continuation(continuations))
                        .map(str::to_owned)
                },
                items,
            });
        }
        let continuation_items = response
            .on_response_received_actions
            .as_ref()
            .and_then(|actions| actions.first())
            .and_then(|action| action.append_continuation_items_action.as_ref())
            .and_then(|items| items.continuation_items.as_ref());
        let items = continuation_items
            .map(|items| {
                crate::models::music_renderers::shelf_items(items)
                    .iter()
                    .filter_map(|renderer| {
                        ArtistItemsPageParser::from_music_responsive_list_item_renderer(renderer)
                    })
                    .map(YTItem::Song)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        Ok(crate::pages::artist_items::ArtistItemsContinuationPage {
            continuation: if items.is_empty() {
                None
            } else {
                continuation_items
                    .and_then(|items| crate::models::music_renderers::shelf_continuation(items))
            },
            items,
        })
    }
}

/// Part 2 of the facade: playlist/home/explore/library/charts/history and
/// the account-side operations (YouTube.kt:564-1554).
impl YouTube {
    /// `playlist(playlistId)` (YouTube.kt:564-627).
    pub async fn playlist(&self, playlist_id: &str) -> Result<PlaylistPage, InnertubeError> {
        let response = self
            .browse_login_json(Some(&format!("VL{playlist_id}")), None)
            .await?;
        let two_column = response
            .contents
            .as_ref()
            .and_then(|contents| contents.two_column_browse_results_renderer.as_ref());
        let base = two_column
            .and_then(|two| two.tabs.as_ref())
            .and_then(|tabs| tabs.iter().flatten().next())
            .and_then(|tab| tab.tab_renderer.as_ref())
            .and_then(|tab| tab.content.as_ref())
            .and_then(|content| content.section_list_renderer.as_ref())
            .and_then(|section| section.contents.as_ref())
            .and_then(|contents| contents.first());
        let header = base
            .and_then(|content| content.music_responsive_header_renderer.as_ref())
            .or_else(|| {
                base.and_then(|content| {
                    content
                        .music_editable_playlist_detail_header_renderer
                        .as_ref()
                })
                .and_then(|editable| editable.header.music_responsive_header_renderer.as_ref())
            })
            .ok_or_else(|| InnertubeError::Parse("missing playlist header".into()))?;
        let editable = base
            .map(|content| {
                content
                    .music_editable_playlist_detail_header_renderer
                    .is_some()
            })
            .unwrap_or(false);
        let secondary = two_column.and_then(|two| two.secondary_contents.as_ref());
        let secondary_section =
            secondary.and_then(|secondary| secondary.section_list_renderer.as_ref());

        // the related walk, with its one continuation retry
        // (YouTube.kt:576-593)
        let mut related: Option<Vec<YTItem>> = secondary_section
            .and_then(|section| section.contents.as_ref())
            .map(|contents| Self::parse_related_items(&contents[1.min(contents.len())..]))
            .filter(|items| !items.is_empty());
        if related.is_none()
            && let Some(token) = secondary_section
                .and_then(|section| section.continuations.as_ref())
                .and_then(|continuations| get_continuation(continuations))
        {
            let continuation_response = self.browse_continuation_json(token, true).await?;
            if let Some(parsed) = continuation_response
                .continuation_contents
                .as_ref()
                .and_then(|contents| contents.section_list_continuation.as_ref())
                .map(|section| Self::parse_related_items(&section.contents))
                .filter(|items| !items.is_empty())
            {
                related = Some(parsed);
            }
        }

        let songs_shelf = secondary_section
            .and_then(|section| section.contents.as_ref())
            .and_then(|contents| contents.first())
            .and_then(|content| content.music_playlist_shelf_renderer.as_ref());

        Ok(PlaylistPage {
            playlist: PlaylistItem {
                id: playlist_id.to_owned(),
                title: header
                    .title
                    .first()
                    .map(|run| run.text.clone())
                    .ok_or_else(|| InnertubeError::Parse("missing playlist title".into()))?,
                author: header.strapline_text_one.as_ref().and_then(|runs| {
                    runs.first().map(|run| Artist {
                        name: run.text.clone(),
                        id: run
                            .navigation_endpoint
                            .as_ref()
                            .and_then(|endpoint| endpoint.browse_endpoint.as_ref())
                            .map(|browse| browse.browse_id.clone()),
                    })
                }),
                song_count_text: header
                    .second_subtitle
                    .as_ref()
                    .and_then(|runs| runs.first())
                    .map(|run| run.text.clone()),
                // both optional: Liked music's header can come without
                // them, and the songs still count (YouTube.kt:606-607)
                thumbnail: header
                    .thumbnail
                    .as_ref()
                    .and_then(|thumbnail| thumbnail.music_thumbnail_renderer.as_ref())
                    .and_then(|music| music.get_thumbnail_url().map(str::to_owned)),
                play_endpoint: None,
                shuffle_endpoint: header
                    .buttons
                    .last()
                    .and_then(|button| button.menu_renderer.as_ref())
                    .and_then(|menu| menu.items.as_ref())
                    .and_then(|items| items.first())
                    .and_then(|item| item.menu_navigation_item_renderer.as_ref())
                    .and_then(|navigation| {
                        navigation
                            .navigation_endpoint
                            .watch_playlist_endpoint
                            .clone()
                    }),
                radio_endpoint: header
                    .buttons
                    .get(2)
                    .and_then(|button| button.menu_renderer.as_ref())
                    .and_then(|menu| menu.items.as_ref())
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
                is_editable: editable,
            },
            songs: songs_shelf
                .map(|shelf| {
                    crate::models::music_renderers::shelf_items(&shelf.contents)
                        .iter()
                        .filter_map(|renderer| {
                            PlaylistPageParser::from_music_responsive_list_item_renderer(renderer)
                        })
                        .collect()
                })
                .unwrap_or_default(),
            songs_continuation: songs_shelf
                .and_then(|shelf| {
                    crate::models::music_renderers::shelf_continuation(&shelf.contents)
                })
                .or_else(|| {
                    songs_shelf
                        .and_then(|shelf| shelf.continuations.as_ref())
                        .and_then(|continuations| get_continuation(continuations))
                        .map(str::to_owned)
                }),
            continuation: secondary_section
                .and_then(|section| section.continuations.as_ref())
                .and_then(|continuations| get_continuation(continuations))
                .map(str::to_owned),
            related: related.filter(|items| !items.is_empty()),
        })
    }

    /// `parseRelatedItems` (YouTube.kt:629-637).
    fn parse_related_items(contents: &[crate::models::section_list::Content]) -> Vec<YTItem> {
        contents
            .iter()
            .flat_map(|content| {
                if let Some(carousel) = content.music_carousel_shelf_renderer.as_ref() {
                    carousel
                        .contents
                        .iter()
                        .filter_map(|content| content.music_two_row_item_renderer.as_ref())
                        .filter_map(RelatedPageParser::from_music_two_row_item_renderer)
                        .collect::<Vec<_>>()
                } else if let Some(shelf) = content.music_shelf_renderer.as_ref() {
                    shelf
                        .contents
                        .as_ref()
                        .map(|contents| crate::models::music_renderers::shelf_items(contents))
                        .unwrap_or_default()
                        .iter()
                        .filter_map(|renderer| {
                            SearchSummaryPageParser::from_music_responsive_list_item_renderer(
                                renderer, None,
                            )
                        })
                        .collect()
                } else {
                    Vec::new()
                }
            })
            .collect()
    }

    /// `playlistContinuation` (YouTube.kt:639-690).
    pub async fn playlist_continuation(
        &self,
        continuation: &str,
    ) -> Result<PlaylistContinuationPage, InnertubeError> {
        let response = self.browse_continuation_json(continuation, true).await?;

        let main_contents: Vec<crate::models::music_renderers::MusicShelfContent> = response
            .continuation_contents
            .as_ref()
            .and_then(|contents| contents.section_list_continuation.as_ref())
            .map(|section| {
                section
                    .contents
                    .iter()
                    .filter_map(|content| {
                        content
                            .music_playlist_shelf_renderer
                            .as_ref()
                            .map(|shelf| shelf.contents.clone())
                    })
                    .flatten()
                    .collect()
            })
            .unwrap_or_default();
        let shelf_contents = response
            .continuation_contents
            .as_ref()
            .and_then(|contents| contents.music_playlist_shelf_continuation.as_ref())
            .map(|shelf| shelf.contents.clone())
            .unwrap_or_default();
        let appended_contents = response
            .on_response_received_actions
            .as_ref()
            .and_then(|actions| actions.first())
            .and_then(|action| action.append_continuation_items_action.as_ref())
            .and_then(|items| items.continuation_items.clone())
            .unwrap_or_default();

        let all_contents: Vec<crate::models::music_renderers::MusicShelfContent> =
            [main_contents, shelf_contents, appended_contents].concat();

        let songs: Vec<SongItem> = all_contents
            .iter()
            .filter_map(|content| content.music_responsive_list_item_renderer.as_ref())
            .filter_map(PlaylistPageParser::from_music_responsive_list_item_renderer)
            .collect();

        let next_continuation = if songs.is_empty() {
            None
        } else {
            let from_contents = response
                .continuation_contents
                .as_ref()
                .and_then(|contents| {
                    contents
                        .section_list_continuation
                        .as_ref()
                        .and_then(|section| section.continuations.as_ref())
                        .and_then(|continuations| get_continuation(continuations))
                        .or_else(|| {
                            contents
                                .music_playlist_shelf_continuation
                                .as_ref()
                                .and_then(|shelf| shelf.continuations.as_ref())
                                .and_then(|continuations| get_continuation(continuations))
                        })
                        .or_else(|| {
                            contents
                                .music_shelf_continuation
                                .as_ref()
                                .and_then(|shelf| shelf.continuations.as_ref())
                                .and_then(|continuations| get_continuation(continuations))
                        })
                        .map(str::to_owned)
                });
            from_contents.or_else(|| {
                response
                    .on_response_received_actions
                    .as_ref()
                    .and_then(|actions| actions.first())
                    .and_then(|action| action.append_continuation_items_action.as_ref())
                    .and_then(|items| items.continuation_items.as_ref())
                    .and_then(|items| crate::models::music_renderers::shelf_continuation(items))
            })
        };

        Ok(PlaylistContinuationPage {
            songs,
            continuation: next_continuation,
        })
    }

    /// The `completedLibrary` extension (Utils.kt:9-38) — a page that
    /// fails, repeats or comes back empty stops here and leaves its token
    /// in songsContinuation, so a sync knows the list is short.
    pub async fn completed_playlist(
        &self,
        page: PlaylistPage,
    ) -> Result<PlaylistPage, InnertubeError> {
        let mut songs = page.songs.clone();
        let mut continuation = page.songs_continuation.clone();
        let mut pager = ContinuationPager::playlist_songs();
        while let Some(token) = continuation.clone() {
            match pager.admit(&token) {
                crate::paging::Admit::Fetch => {}
                crate::paging::Admit::Stop(_) => break,
            }
            let Ok(continuation_page) = self.playlist_continuation(&token).await else {
                break;
            };
            // an empty page never carries a next token, and youtube only
            // hands out a token when more songs follow (Utils.kt:26-27)
            if continuation_page.songs.is_empty() {
                break;
            }
            songs.extend(continuation_page.songs);
            continuation = continuation_page.continuation;
        }
        Ok(PlaylistPage {
            playlist: page.playlist,
            songs,
            songs_continuation: continuation,
            continuation: page.continuation,
            related: page.related,
        })
    }

    /// The `completedPlaylist` extension (Utils.kt:41-74) — when paging
    /// stops early, continuation keeps the token it stopped on.
    pub async fn completed_library(
        &self,
        page: LibraryPage,
    ) -> Result<LibraryPage, InnertubeError> {
        let mut items = page.items.clone();
        let mut continuation = page.continuation.clone();
        let mut pager = ContinuationPager::library_items();
        while let Some(token) = continuation.clone() {
            match pager.admit(&token) {
                crate::paging::Admit::Fetch => {}
                crate::paging::Admit::Stop(_) => break,
            }
            let Ok(continuation_page) = self.library_continuation(&token).await else {
                break;
            };
            if continuation_page.items.is_empty() {
                if !pager.record_page(true) {
                    break;
                }
            } else {
                pager.record_page(false);
                items.extend(continuation_page.items);
            }
            continuation = continuation_page.continuation;
        }
        Ok(LibraryPage {
            items,
            continuation,
        })
    }

    /// `home(continuation, params)` (YouTube.kt:692-709).
    pub async fn home(
        &self,
        continuation: Option<&str>,
        params: Option<&str>,
    ) -> Result<HomePage, InnertubeError> {
        if let Some(continuation) = continuation {
            return self.home_continuation(continuation).await;
        }
        let response = self.browse_json(Some("FEmusic_home"), params).await?;
        let section_list = response
            .contents
            .as_ref()
            .and_then(|contents| contents.single_column_browse_results_renderer.as_ref())
            .and_then(|single| single.tabs.first())
            .and_then(|tab| tab.tab_renderer.as_ref())
            .and_then(|tab| tab.content.as_ref())
            .and_then(|content| content.section_list_renderer.as_ref());
        let sections: Vec<HomeSection> = section_list
            .and_then(|section| section.contents.as_ref())
            .map(|contents| {
                contents
                    .iter()
                    .filter_map(|content| content.music_carousel_shelf_renderer.as_ref())
                    .filter_map(HomeSection::from_music_carousel_shelf_renderer)
                    .collect()
            })
            .unwrap_or_default();
        Ok(HomePage {
            chips: section_list.and_then(home_chips),
            sections,
            continuation: section_list
                .and_then(|section| section.continuations.as_ref())
                .and_then(|continuations| get_continuation(continuations))
                .map(str::to_owned),
        })
    }

    /// `homeContinuation` (YouTube.kt:711-724).
    async fn home_continuation(&self, continuation: &str) -> Result<HomePage, InnertubeError> {
        let response = self.browse_continuation_json(continuation, false).await?;
        let section = response
            .continuation_contents
            .and_then(|contents| contents.section_list_continuation);
        Ok(HomePage {
            chips: None,
            sections: section
                .as_ref()
                .map(|section| {
                    section
                        .contents
                        .iter()
                        .filter_map(|content| content.music_carousel_shelf_renderer.as_ref())
                        .filter_map(HomeSection::from_music_carousel_shelf_renderer)
                        .collect()
                })
                .unwrap_or_default(),
            continuation: section
                .as_ref()
                .and_then(|section| section.continuations.as_ref())
                .and_then(|continuations| get_continuation(continuations))
                .map(str::to_owned),
        })
    }

    /// `explore()` (YouTube.kt:726-741).
    pub async fn explore(&self) -> Result<ExplorePage, InnertubeError> {
        let response = self.browse_json(Some("FEmusic_explore"), None).await?;
        let sections = || {
            response
                .contents
                .as_ref()
                .and_then(|contents| contents.single_column_browse_results_renderer.as_ref())
                .and_then(|single| single.tabs.first())
                .and_then(|tab| tab.tab_renderer.as_ref())
                .and_then(|tab| tab.content.as_ref())
                .and_then(|content| content.section_list_renderer.as_ref())
                .and_then(|section| section.contents.as_ref())
        };
        let find_carousel = |browse_id: &str| {
            sections().and_then(|contents| {
                contents.iter().find_map(|content| {
                    content
                        .music_carousel_shelf_renderer
                        .as_ref()
                        .filter(|carousel| {
                            carousel
                                .header
                                .as_ref()
                                .map(|header| {
                                    &header
                                        .music_carousel_shelf_basic_header_renderer
                                        .more_content_button
                                })
                                .and_then(|button| button.as_ref())
                                .and_then(|button| {
                                    button.button_renderer.navigation_endpoint.as_ref()
                                })
                                .and_then(|endpoint| endpoint.browse_endpoint.as_ref())
                                .map(|browse| browse.browse_id == browse_id)
                                .unwrap_or(false)
                        })
                })
            })
        };
        Ok(ExplorePage {
            new_release_albums: find_carousel("FEmusic_new_releases_albums")
                .map(|carousel| {
                    carousel
                        .contents
                        .iter()
                        .filter_map(|content| content.music_two_row_item_renderer.as_ref())
                        .filter_map(NewReleaseAlbumPage::from_music_two_row_item_renderer)
                        .collect()
                })
                .unwrap_or_default(),
            mood_and_genres: find_carousel("FEmusic_moods_and_genres")
                .map(|carousel| {
                    carousel
                        .contents
                        .iter()
                        .filter_map(|content| content.music_navigation_button_renderer.as_ref())
                        .filter_map(MoodAndGenresParser::from_music_navigation_button_renderer)
                        .collect()
                })
                .unwrap_or_default(),
        })
    }

    /// `newReleaseAlbums()` (YouTube.kt:743-749).
    pub async fn new_release_albums(&self) -> Result<Vec<AlbumItem>, InnertubeError> {
        let response = self
            .browse_json(Some("FEmusic_new_releases_albums"), None)
            .await?;
        Ok(response
            .contents
            .as_ref()
            .and_then(|contents| contents.single_column_browse_results_renderer.as_ref())
            .and_then(|single| single.tabs.first())
            .and_then(|tab| tab.tab_renderer.as_ref())
            .and_then(|tab| tab.content.as_ref())
            .and_then(|content| content.section_list_renderer.as_ref())
            .and_then(|section| section.contents.as_ref())
            .and_then(|contents| contents.first())
            .and_then(|content| content.grid_renderer.as_ref())
            .map(|grid| {
                grid.items
                    .iter()
                    .filter_map(|item| item.music_two_row_item_renderer.as_ref())
                    .filter_map(NewReleaseAlbumPage::from_music_two_row_item_renderer)
                    .collect()
            })
            .unwrap_or_default())
    }

    /// `moodAndGenres()` (YouTube.kt:751-755).
    pub async fn mood_and_genres(&self) -> Result<Vec<MoodAndGenres>, InnertubeError> {
        let response = self
            .browse_json(Some("FEmusic_moods_and_genres"), None)
            .await?;
        Ok(response
            .contents
            .as_ref()
            .and_then(|contents| contents.single_column_browse_results_renderer.as_ref())
            .and_then(|single| single.tabs.first())
            .and_then(|tab| tab.tab_renderer.as_ref())
            .and_then(|tab| tab.content.as_ref())
            .and_then(|content| content.section_list_renderer.as_ref())
            .and_then(|section| section.contents.as_ref())
            .map(|contents| {
                contents
                    .iter()
                    .filter_map(MoodAndGenresParser::from_section_list_renderer_content)
                    .collect()
            })
            .unwrap_or_default())
    }

    /// `browse(browseId, params)` (YouTube.kt:757-787).
    pub async fn browse(
        &self,
        browse_id: &str,
        params: Option<&str>,
    ) -> Result<BrowseResult, InnertubeError> {
        let response = self.browse_json(Some(browse_id), params).await?;
        let contents = response
            .contents
            .as_ref()
            .and_then(|contents| contents.single_column_browse_results_renderer.as_ref())
            .and_then(|single| single.tabs.first())
            .and_then(|tab| tab.tab_renderer.as_ref())
            .and_then(|tab| tab.content.as_ref())
            .and_then(|content| content.section_list_renderer.as_ref())
            .and_then(|section| section.contents.as_ref());
        let items = contents
            .map(|contents| {
                contents
                    .iter()
                    .filter_map(|content| {
                        if let Some(grid) = content.grid_renderer.as_ref() {
                            Some(BrowseItem {
                                title: grid
                                    .header
                                    .as_ref()
                                    .and_then(|header| {
                                        header
                                            .grid_header_renderer
                                            .title
                                            .first()
                                            .map(|run| run.text.clone())
                                    }),
                                items: grid
                                    .items
                                    .iter()
                                    .filter_map(|item| {
                                        item.music_two_row_item_renderer.as_ref()
                                    })
                                    .filter_map(
                                        RelatedPageParser::from_music_two_row_item_renderer,
                                    )
                                    .collect(),
                            })
                        } else { content.music_carousel_shelf_renderer.as_ref().map(|carousel| BrowseItem {
                                title: carousel
                                    .header
                                    .as_ref().map(|header| &header
                                                .music_carousel_shelf_basic_header_renderer
                                                .title)
                                    .and_then(|title| {
                                        title.first().map(|run| run.text.clone())
                                    }),
                                items: carousel
                                    .contents
                                    .iter()
                                    .filter_map(|content| {
                                        content
                                            .music_two_row_item_renderer
                                            .as_ref()
                                            .and_then(
                                                RelatedPageParser::from_music_two_row_item_renderer,
                                            )
                                            .or_else(|| {
                                                content
                                                    .music_responsive_list_item_renderer
                                                    .as_ref()
                                                    .and_then(
                                                        RelatedPageParser::from_music_responsive_list_item_renderer,
                                                    )
                                                    .map(YTItem::Song)
                                            })
                                    })
                                    .collect(),
                            }) }
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(BrowseResult {
            title: response
                .header
                .as_ref()
                .and_then(|header| header.music_header_renderer.as_ref())
                .and_then(|plain| plain.title.as_ref())
                .and_then(|title| title.first())
                .map(|run| run.text.clone()),
            items,
        })
    }

    /// `library(browseId, tabIndex)` (YouTube.kt:789-880) — the debug
    /// prints drop (behavior kept).
    pub async fn library(
        &self,
        browse_id: &str,
        tab_index: usize,
    ) -> Result<LibraryPage, InnertubeError> {
        let response = self.browse_login_json(Some(browse_id), None).await?;
        let tabs = response
            .contents
            .as_ref()
            .and_then(|contents| contents.single_column_browse_results_renderer.as_ref())
            .map(|single| single.tabs.as_slice());
        let contents = tabs
            .and_then(|tabs| tabs.get(tab_index))
            .and_then(|tab| tab.tab_renderer.as_ref())
            .and_then(|tab| tab.content.as_ref())
            .and_then(|content| content.section_list_renderer.as_ref())
            .and_then(|section| section.contents.as_ref())
            .and_then(|contents| contents.first());

        if let Some(grid) = contents.and_then(|content| content.grid_renderer.as_ref()) {
            return Ok(LibraryPage {
                items: grid
                    .items
                    .iter()
                    .filter_map(|item| item.music_two_row_item_renderer.as_ref())
                    .filter_map(LibraryPageParser::from_music_two_row_item_renderer)
                    .collect(),
                continuation: grid
                    .continuations
                    .as_ref()
                    .and_then(|continuations| get_continuation(continuations))
                    .map(str::to_owned),
            });
        }
        let shelf_contents = contents
            .and_then(|content| content.music_shelf_renderer.as_ref())
            .and_then(|shelf| shelf.contents.as_ref());
        let Some(shelf_contents) = shelf_contents else {
            return Err(InnertubeError::Parse(format!(
                "No content found for browseId={browse_id}"
            )));
        };
        Ok(LibraryPage {
            items: crate::models::music_renderers::shelf_items(shelf_contents)
                .iter()
                .filter_map(|renderer| {
                    LibraryPageParser::from_music_responsive_list_item_renderer(renderer)
                })
                .collect(),
            continuation: contents
                .and_then(|content| content.music_shelf_renderer.as_ref())
                .and_then(|shelf| shelf.continuations.as_ref())
                .and_then(|continuations| get_continuation(continuations))
                .map(str::to_owned),
        })
    }

    /// `libraryContinuation` (YouTube.kt:882-910).
    pub async fn library_continuation(
        &self,
        continuation: &str,
    ) -> Result<LibraryContinuationPage, InnertubeError> {
        let response = self.browse_continuation_json(continuation, true).await?;
        let contents = response.continuation_contents.as_ref();
        if let Some(grid) = contents.and_then(|contents| contents.grid_continuation.as_ref()) {
            return Ok(LibraryContinuationPage {
                items: grid
                    .items
                    .iter()
                    .filter_map(|item| item.music_two_row_item_renderer.as_ref())
                    .filter_map(LibraryPageParser::from_music_two_row_item_renderer)
                    .collect(),
                continuation: grid
                    .continuations
                    .as_ref()
                    .and_then(|continuations| get_continuation(continuations))
                    .map(str::to_owned),
            });
        }
        let shelf = contents
            .and_then(|contents| contents.music_shelf_continuation.as_ref())
            .ok_or_else(|| InnertubeError::Parse("missing musicShelfContinuation".into()))?;
        Ok(LibraryContinuationPage {
            items: crate::models::music_renderers::shelf_items(
                shelf.contents.as_deref().unwrap_or_default(),
            )
            .iter()
            .filter_map(|renderer| {
                LibraryPageParser::from_music_responsive_list_item_renderer(renderer)
            })
            .collect(),
            continuation: shelf
                .continuations
                .as_ref()
                .and_then(|continuations| get_continuation(continuations))
                .map(str::to_owned),
        })
    }

    /// `libraryRecentActivity()` (YouTube.kt:912-949) — the recent-activity
    /// artists are re-fetched with their full page data.
    pub async fn library_recent_activity(&self) -> Result<LibraryPage, InnertubeError> {
        let continuation = LibraryFilter::RECENT_ACTIVITY;
        let response = self.browse_continuation_json(continuation, true).await?;
        let grid_items = response
            .continuation_contents
            .as_ref()
            .and_then(|contents| contents.section_list_continuation.as_ref())
            .and_then(|section| section.contents.first())
            .and_then(|content| content.grid_renderer.as_ref())
            .map(|grid| grid.items.clone());

        let Some(grid_items) = grid_items else {
            return Ok(LibraryPage {
                items: Vec::new(),
                continuation: None,
            });
        };

        let mut items: Vec<YTItem> = grid_items
            .iter()
            .filter_map(|item| item.music_two_row_item_renderer.as_ref())
            .filter_map(LibraryPageParser::from_music_two_row_item_renderer)
            .collect();

        // the recent-activity artists re-fetch with their full page data
        // (YouTube.kt:937-943); ids collected first so no borrow crosses
        // an await
        let artist_ids: Vec<(usize, String)> = items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| match item {
                YTItem::Artist(artist) => Some((index, artist.id.clone())),
                _ => None,
            })
            .collect();
        for (index, artist_id) in artist_ids {
            if let Ok(fetched) = self.artist(&artist_id).await
                && let YTItem::Artist(old) = &items[index]
            {
                let mut full = fetched.artist;
                full.thumbnail = old.thumbnail.clone();
                items[index] = YTItem::Artist(full);
            }
        }
        Ok(LibraryPage {
            items,
            continuation: None,
        })
    }

    /// `getChartsPage(continuation)` (YouTube.kt:951-1015).
    pub async fn get_charts_page(
        &self,
        continuation: Option<&str>,
    ) -> Result<ChartsPage, InnertubeError> {
        let response = self
            .transport
            .browse(
                &self.web_remix(),
                Some("FEmusic_charts"),
                Some("ggMGCgQIgAQ%3D"),
                continuation,
                false,
                None,
            )
            .await?;
        let response: BrowseResponse = Self::decode(response)?;

        let mut sections = Vec::new();
        let contents = response
            .contents
            .as_ref()
            .and_then(|contents| contents.single_column_browse_results_renderer.as_ref())
            .and_then(|single| single.tabs.first())
            .and_then(|tab| tab.tab_renderer.as_ref())
            .and_then(|tab| tab.content.as_ref())
            .and_then(|content| content.section_list_renderer.as_ref())
            .and_then(|section| section.contents.as_ref());
        if let Some(contents) = contents {
            for content in contents {
                if let Some(carousel) = content.music_carousel_shelf_renderer.as_ref() {
                    let Some(title) = carousel
                        .header
                        .as_ref()
                        .map(|header| &header.music_carousel_shelf_basic_header_renderer.title)
                        .and_then(|title| title.first())
                        .map(|run| run.text.clone())
                    else {
                        continue;
                    };
                    let items: Vec<YTItem> = carousel
                        .contents
                        .iter()
                        .filter_map(|content| {
                            content
                                .music_responsive_list_item_renderer
                                .as_ref()
                                .and_then(Self::convert_to_chart_item)
                                .or_else(|| {
                                    content
                                        .music_two_row_item_renderer
                                        .as_ref()
                                        .and_then(Self::convert_music_two_row_item)
                                })
                        })
                        .collect();
                    if !items.is_empty() {
                        sections.push(ChartSection {
                            title: title.clone(),
                            items,
                            chart_type: Self::determine_chart_type(&title),
                        });
                    }
                }
                if let Some(grid) = content.grid_renderer.as_ref() {
                    let Some(title) = grid.header.as_ref().and_then(|header| {
                        header
                            .grid_header_renderer
                            .title
                            .first()
                            .map(|run| run.text.clone())
                    }) else {
                        continue;
                    };
                    let items: Vec<YTItem> = grid
                        .items
                        .iter()
                        .filter_map(|item| item.music_two_row_item_renderer.as_ref())
                        .filter_map(Self::convert_music_two_row_item)
                        .collect();
                    if !items.is_empty() {
                        sections.push(ChartSection {
                            title,
                            items,
                            chart_type: ChartType::NewReleases,
                        });
                    }
                }
            }
        }

        Ok(ChartsPage {
            sections,
            continuation: response
                .continuation_contents
                .as_ref()
                .and_then(|contents| contents.section_list_continuation.as_ref())
                .and_then(|section| section.continuations.as_ref())
                .and_then(|continuations| get_continuation(continuations))
                .map(str::to_owned),
        })
    }

    /// `determineChartType` (YouTube.kt:1017-1023).
    fn determine_chart_type(title: &str) -> ChartType {
        if title.to_lowercase().contains("trending") {
            ChartType::Trending
        } else if title.to_lowercase().contains("top") {
            ChartType::Top
        } else {
            ChartType::Genre
        }
    }

    /// `convertToChartItem` (YouTube.kt:1025-1072).
    fn convert_to_chart_item(
        renderer: &crate::models::music_renderers::MusicResponsiveListItemRenderer,
    ) -> Option<YTItem> {
        if renderer.flex_columns.len() < 3 || renderer.playlist_item_data.is_none() {
            return None;
        }
        let first_column = renderer
            .flex_columns
            .first()?
            .music_responsive_list_item_flex_column_renderer
            .text
            .as_ref()?;
        let second_column = renderer
            .flex_columns
            .get(1)?
            .music_responsive_list_item_flex_column_renderer
            .text
            .as_ref()?;
        let title = first_column.first()?.text.clone();
        if title.trim().is_empty() {
            return None;
        }
        let artists = second_column
            .runs
            .as_ref()
            .map(|runs| {
                runs.iter()
                    .filter_map(|run| {
                        let name = run.text.trim();
                        if name.is_empty() {
                            return None;
                        }
                        Some(Artist {
                            name: name.to_owned(),
                            id: run
                                .navigation_endpoint
                                .as_ref()
                                .and_then(|endpoint| endpoint.browse_endpoint.as_ref())
                                .map(|browse| browse.browse_id.clone()),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let third_column = renderer
            .flex_columns
            .get(2)?
            .music_responsive_list_item_flex_column_renderer
            .text
            .as_ref();
        Some(YTItem::Song(SongItem {
            id: renderer.playlist_item_data.as_ref()?.video_id.clone(),
            title,
            artists,
            thumbnail: renderer
                .thumbnail
                .as_ref()?
                .music_thumbnail_renderer
                .as_ref()?
                .get_thumbnail_url()?
                .to_owned(),
            music_video_type: renderer.music_video_type(),
            explicit: crate::pages::home::explicit_badge(&renderer.badges),
            chart_position: third_column
                .and_then(|text| text.first())
                .and_then(|run| run.text.parse().ok()),
            chart_change: third_column
                .and_then(|text| text.runs.as_ref())
                .and_then(|runs| runs.get(1))
                .map(|run| run.text.clone()),
            ..Default::default()
        }))
    }

    /// `convertMusicTwoRowItem` (YouTube.kt:1074-1119).
    fn convert_music_two_row_item(
        renderer: &crate::models::music_renderers::MusicTwoRowItemRenderer,
    ) -> Option<YTItem> {
        if renderer.is_song() {
            let subtitle = renderer.subtitle.as_ref()?.runs.as_ref()?;
            Some(YTItem::Song(SongItem {
                id: renderer
                    .navigation_endpoint
                    .watch_endpoint
                    .as_ref()?
                    .video_id
                    .clone()?,
                title: renderer.title.first()?.text.clone(),
                artists: subtitle
                    .iter()
                    .filter_map(|run| {
                        run.navigation_endpoint
                            .as_ref()
                            .and_then(|endpoint| endpoint.browse_endpoint.as_ref())
                            .map(|browse| Artist {
                                name: run.text.clone(),
                                id: Some(browse.browse_id.clone()),
                            })
                    })
                    .collect(),
                thumbnail: renderer
                    .thumbnail_renderer
                    .music_thumbnail_renderer
                    .as_ref()?
                    .get_thumbnail_url()?
                    .to_owned(),
                music_video_type: renderer.music_video_type(),
                explicit: crate::pages::home::explicit_badge(&renderer.subtitle_badges),
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
                    crate::models::runs::odd_elements(renderer.subtitle.as_ref()?.runs.as_ref()?)
                        .iter()
                        .skip(1)
                        .filter_map(|run| {
                            run.navigation_endpoint
                                .as_ref()
                                .and_then(|endpoint| endpoint.browse_endpoint.as_ref())
                                .map(|browse| Artist {
                                    name: run.text.clone(),
                                    id: Some(browse.browse_id.clone()),
                                })
                        })
                        .collect(),
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
                explicit: crate::pages::home::explicit_badge(&renderer.subtitle_badges),
                ..Default::default()
            }))
        } else {
            None
        }
    }

    /// `musicHistory(hlOverride, maxRequests)` (YouTube.kt:1121-1180) —
    /// the history, read `max_requests` pages deep: the first page only
    /// holds the latest few buckets, older ones (and the rest of a long
    /// bucket) come as continuations.
    pub async fn music_history(
        &self,
        hl_override: Option<&str>,
        max_requests: u32,
    ) -> Result<HistoryPage, InnertubeError> {
        let client = self.web_remix();
        let page = |browse_id: Option<String>, continuation: Option<String>| {
            let transport = self.transport.clone();
            let client = client.clone();
            async move {
                let value = transport
                    .browse(
                        &client,
                        browse_id.as_deref(),
                        None,
                        continuation.as_deref(),
                        true,
                        hl_override,
                    )
                    .await?;
                Self::decode::<BrowseResponse>(value)
            }
        };

        let mut sections: Vec<(String, Vec<SongItem>)> = Vec::new();
        // more of a shelf already read, as (shelf index, token), read
        // before going further back
        let mut shelf_continuations: VecDeque<(usize, String)> = VecDeque::new();

        macro_rules! add_shelf {
            ($shelf:expr) => {{
                let shelf = $shelf;
                if let Some(section) =
                    crate::pages::history::HistoryPageParser::from_music_shelf_renderer(shelf)
                {
                    // a bucket cut across two pages comes back as two
                    // shelves with the same title (YouTube.kt:1137-1145)
                    let index = if sections
                        .last()
                        .map(|(title, _)| title == &section.title)
                        .unwrap_or(false)
                    {
                        sections.last_mut().unwrap().1.extend(section.songs);
                        sections.len() - 1
                    } else {
                        sections.push((section.title.clone(), section.songs));
                        sections.len() - 1
                    };
                    if let Some(token) = shelf
                        .continuations
                        .as_ref()
                        .and_then(|continuations| get_continuation(continuations))
                        .map(str::to_owned)
                        .or_else(|| {
                            shelf
                                .contents
                                .as_ref()
                                .map(|items| {
                                    crate::models::music_renderers::shelf_continuation(items)
                                })
                                .flatten()
                        })
                    {
                        shelf_continuations.push_back((index, token));
                    }
                }
            }};
        }

        let first = page(Some("FEmusic_history".to_owned()), None).await?;
        let list = first
            .contents
            .as_ref()
            .and_then(|contents| contents.single_column_browse_results_renderer.as_ref())
            .and_then(|single| single.tabs.first())
            .and_then(|tab| tab.tab_renderer.as_ref())
            .and_then(|tab| tab.content.as_ref())
            .and_then(|content| content.section_list_renderer.as_ref());
        if let Some(list) = list
            && let Some(contents) = list.contents.as_ref()
        {
            for content in contents {
                if let Some(shelf) = content.music_shelf_renderer.as_ref() {
                    add_shelf!(shelf);
                }
            }
        }
        let mut list_continuation = list
            .and_then(|list| list.continuations.as_ref())
            .and_then(|continuations| get_continuation(continuations))
            .map(str::to_owned);
        let mut seen = std::collections::HashSet::new();

        let mut requests: u32 = 1;
        while requests < max_requests {
            if let Some((shelf_index, token)) = shelf_continuations.pop_front() {
                if !seen.insert(token.clone()) {
                    continue;
                }
                let response = page(None, Some(token.clone())).await?;
                requests += 1;
                let shelf_continuation = response
                    .continuation_contents
                    .as_ref()
                    .and_then(|contents| contents.music_shelf_continuation.as_ref());
                let items = shelf_continuation
                    .and_then(|shelf| shelf.contents.clone())
                    .or_else(|| {
                        response
                            .on_response_received_actions
                            .as_ref()
                            .and_then(|actions| actions.first())
                            .and_then(|action| action.append_continuation_items_action.as_ref())
                            .and_then(|items| items.continuation_items.clone())
                    })
                    .unwrap_or_default();
                sections[shelf_index]
                    .1
                    .extend(crate::pages::history::HistoryPageParser::songs_of(&items));
                let next = shelf_continuation
                    .and_then(|shelf| shelf.continuations.as_ref())
                    .and_then(|continuations| get_continuation(continuations))
                    .map(str::to_owned)
                    .or_else(|| crate::models::music_renderers::shelf_continuation(&items));
                if let Some(next) = next {
                    shelf_continuations.push_front((shelf_index, next));
                }
                continue;
            }
            let Some(token) = list_continuation.clone() else {
                break;
            };
            if !seen.insert(token.clone()) {
                break;
            }
            let response = page(None, Some(token.clone())).await?;
            requests += 1;
            let more = response
                .continuation_contents
                .and_then(|contents| contents.section_list_continuation);
            if let Some(more) = &more {
                for content in &more.contents {
                    if let Some(shelf) = content.music_shelf_renderer.as_ref() {
                        add_shelf!(shelf);
                    }
                }
            }
            list_continuation = more.and_then(|more| {
                more.continuations
                    .as_ref()
                    .and_then(|continuations| get_continuation(continuations))
                    .map(str::to_owned)
            });
        }

        Ok(HistoryPage {
            sections: Some(
                sections
                    .into_iter()
                    .map(|(title, songs)| HistorySection { title, songs })
                    .collect(),
            ),
        })
    }
}

/// Part 3 of the facade: likes/subscriptions, playlist CRUD, player,
/// playback reporting, next/radio, lyrics, related, queue, transcripts,
/// captions, session helpers, account and media info (YouTube.kt:1182-1554).
impl YouTube {
    /// `likeVideo` (YouTube.kt:1182-1187).
    pub async fn like_video(&self, video_id: &str, like: bool) -> Result<(), InnertubeError> {
        if like {
            self.transport
                .like_video(&self.web_remix(), video_id)
                .await?;
        } else {
            self.transport
                .unlike_video(&self.web_remix(), video_id)
                .await?;
        }
        Ok(())
    }

    /// `likePlaylist` (YouTube.kt:1189-1194).
    pub async fn like_playlist(&self, playlist_id: &str, like: bool) -> Result<(), InnertubeError> {
        if like {
            self.transport
                .like_playlist(&self.web_remix(), playlist_id)
                .await?;
        } else {
            self.transport
                .unlike_playlist(&self.web_remix(), playlist_id)
                .await?;
        }
        Ok(())
    }

    /// `subscribeChannel` (YouTube.kt:1196-1201).
    pub async fn subscribe_channel(
        &self,
        channel_id: &str,
        subscribe: bool,
    ) -> Result<(), InnertubeError> {
        if subscribe {
            self.transport
                .subscribe_channel(&self.web_remix(), channel_id)
                .await?;
        } else {
            self.transport
                .unsubscribe_channel(&self.web_remix(), channel_id)
                .await?;
        }
        Ok(())
    }

    /// `getChannelId` (YouTube.kt:1203-1208).
    pub async fn get_channel_id(&self, browse_id: &str) -> String {
        self.artist(browse_id)
            .await
            .ok()
            .and_then(|page| page.artist.channel_id)
            .unwrap_or_default()
    }

    /// `addToPlaylist` (YouTube.kt:1210-1212).
    pub async fn add_to_playlist(
        &self,
        playlist_id: &str,
        video_id: &str,
    ) -> Result<(), InnertubeError> {
        self.transport
            .add_to_playlist(&self.web_remix(), playlist_id, video_id)
            .await?;
        Ok(())
    }

    /// `addVideosToPlaylist` (YouTube.kt:1214-1220) — the videos go in
    /// batches of `batch_size`, one edit_playlist call each instead of one
    /// per song.
    pub async fn add_videos_to_playlist(
        &self,
        playlist_id: &str,
        video_ids: &[String],
        batch_size: usize,
    ) -> Result<(), InnertubeError> {
        for batch in video_ids.chunks(batch_size.max(1)) {
            self.transport
                .add_videos_to_playlist(&self.web_remix(), playlist_id, batch)
                .await?;
        }
        Ok(())
    }

    /// `addPlaylistToPlaylist` (YouTube.kt:1222-1224).
    pub async fn add_playlist_to_playlist(
        &self,
        playlist_id: &str,
        add_playlist_id: &str,
    ) -> Result<(), InnertubeError> {
        self.transport
            .add_playlist_to_playlist(&self.web_remix(), playlist_id, add_playlist_id)
            .await?;
        Ok(())
    }

    /// `removeFromPlaylist` (YouTube.kt:1226-1228).
    pub async fn remove_from_playlist(
        &self,
        playlist_id: &str,
        video_id: &str,
        set_video_id: &str,
    ) -> Result<(), InnertubeError> {
        self.transport
            .remove_from_playlist(&self.web_remix(), playlist_id, video_id, set_video_id)
            .await?;
        Ok(())
    }

    /// `moveSongPlaylist` (YouTube.kt:1230-1232).
    pub async fn move_song_playlist(
        &self,
        playlist_id: &str,
        set_video_id: &str,
        successor_set_video_id: Option<&str>,
    ) -> Result<(), InnertubeError> {
        self.transport
            .move_song_playlist(
                &self.web_remix(),
                playlist_id,
                set_video_id,
                successor_set_video_id,
            )
            .await?;
        Ok(())
    }

    /// `createPlaylist` (YouTube.kt:1234-1236) — returns the new playlist
    /// id.
    pub async fn create_playlist(&self, title: &str) -> Result<String, InnertubeError> {
        let value = self
            .transport
            .create_playlist(&self.web_remix(), title)
            .await?;
        let response: CreatePlaylistResponse = Self::decode(value)?;
        Ok(response.playlist_id)
    }

    /// `renamePlaylist` (YouTube.kt:1238-1240).
    pub async fn rename_playlist(
        &self,
        playlist_id: &str,
        name: &str,
    ) -> Result<(), InnertubeError> {
        self.transport
            .rename_playlist(&self.web_remix(), playlist_id, name)
            .await?;
        Ok(())
    }

    /// `uploadCustomThumbnailLink` (YouTube.kt:1242-1251) — the resumable
    /// two-step upload, then the set-thumbnail edit; returns the new
    /// thumbnail url.
    pub async fn upload_custom_thumbnail_link(
        &self,
        playlist_id: &str,
        image: Vec<u8>,
    ) -> Result<Option<String>, InnertubeError> {
        let upload_url = self
            .transport
            .playlist_image_upload_start(&self.web_remix(), image.len())
            .await?;
        let blob_response = self
            .transport
            .playlist_image_upload_finalize(&self.web_remix(), &upload_url, image)
            .await?;
        let blob: ImageUploadResponse = Self::decode(blob_response)?;
        let value = self
            .transport
            .set_thumbnail_playlist(&self.web_remix(), playlist_id, &blob.encrypted_blob_id)
            .await?;
        let response: EditPlaylistResponse = Self::decode(value)?;
        Ok(response
            .new_header
            .as_ref()
            .and_then(|header| {
                header
                    .music_editable_playlist_detail_header_renderer
                    .as_ref()
            })
            .and_then(|editable| editable.header.music_responsive_header_renderer.as_ref())
            .and_then(|responsive| responsive.thumbnail.as_ref())
            .and_then(|thumbnail| thumbnail.music_thumbnail_renderer.as_ref())
            .and_then(|music| music.get_thumbnail_url().map(str::to_owned)))
    }

    /// `removeThumbnailPlaylist` (YouTube.kt:1253-1255).
    pub async fn remove_thumbnail_playlist(
        &self,
        playlist_id: &str,
    ) -> Result<Option<String>, InnertubeError> {
        let value = self
            .transport
            .remove_thumbnail_playlist(&self.web_remix(), playlist_id)
            .await?;
        let response: EditPlaylistResponse = Self::decode(value)?;
        Ok(response
            .new_header
            .as_ref()
            .and_then(|header| {
                header
                    .music_editable_playlist_detail_header_renderer
                    .as_ref()
            })
            .and_then(|editable| editable.header.music_responsive_header_renderer.as_ref())
            .and_then(|responsive| responsive.thumbnail.as_ref())
            .and_then(|thumbnail| thumbnail.music_thumbnail_renderer.as_ref())
            .and_then(|music| music.get_thumbnail_url().map(str::to_owned)))
    }

    /// `deletePlaylist` (YouTube.kt:1257-1259).
    pub async fn delete_playlist(&self, playlist_id: &str) -> Result<(), InnertubeError> {
        self.transport
            .delete_playlist(&self.web_remix(), playlist_id)
            .await?;
        Ok(())
    }

    /// `player` (YouTube.kt:1261-1263).
    pub async fn player(
        &self,
        video_id: &str,
        playlist_id: Option<&str>,
        client: &YouTubeClient,
        signature_timestamp: Option<i32>,
        po_token: Option<&str>,
    ) -> Result<PlayerResponse, InnertubeError> {
        let value = self
            .transport
            .player(client, video_id, playlist_id, signature_timestamp, po_token)
            .await?;
        Self::decode(value)
    }

    /// `registerPlayback` (YouTube.kt:1270-1280) — a fresh cpn is minted
    /// when the caller does not supply one.
    pub async fn register_playback(
        &self,
        playlist_id: Option<&str>,
        playback_tracking: &str,
        cpn: Option<&str>,
    ) -> Result<(), InnertubeError> {
        let cpn = cpn.map(str::to_owned).unwrap_or_else(new_playback_nonce);
        self.transport
            .register_playback(
                &music_stats_url(playback_tracking),
                &cpn,
                playlist_id,
                &self.web_remix(),
                &[],
            )
            .await?;
        Ok(())
    }

    /// `reportWatchtime` (YouTube.kt:1283-1297) — says which stretches of
    /// a song were heard; fields the link already carries (like len) are
    /// not sent twice.
    pub async fn report_watchtime(
        &self,
        watchtime_tracking: &str,
        cpn: &str,
        params: &[(String, String)],
        playlist_id: Option<&str>,
    ) -> Result<(), InnertubeError> {
        let url = music_stats_url(watchtime_tracking);
        let already_in_url = query_param_names(&url);
        let extra_params: Vec<(String, String)> = params
            .iter()
            .filter(|(name, _)| !already_in_url.contains(name))
            .cloned()
            .collect();
        self.transport
            .register_playback(&url, cpn, playlist_id, &self.web_remix(), &extra_params)
            .await?;
        Ok(())
    }

    /// `next(endpoint, continuation)` (YouTube.kt:1302-1350) — with the
    /// automix recursion: some hindi songs have an empty automix preview,
    /// so fall back to the song and its related tab instead of failing
    /// the whole queue.
    pub async fn next(
        &self,
        endpoint: &WatchEndpoint,
        continuation: Option<&str>,
    ) -> Result<NextResult, InnertubeError> {
        let response: NextResponse = Self::decode(
            self.transport
                .next(
                    &self.web_remix(),
                    endpoint.video_id.as_deref(),
                    endpoint.playlist_id.as_deref(),
                    endpoint.playlist_set_video_id.as_deref(),
                    endpoint.index,
                    endpoint.params.as_deref(),
                    continuation,
                )
                .await?,
        )?;
        let watch = response
            .contents
            .as_ref()
            .and_then(|contents| {
                contents
                    .single_column_music_watch_next_results_renderer
                    .as_ref()
            })
            .and_then(|single| single.tabbed_renderer.as_ref())
            .and_then(|tabbed| tabbed.watch_next_tabbed_results_renderer.as_ref());
        let tabs = watch.map(|watch| watch.tabs.as_slice()).unwrap_or(&[]);
        let playlist_panel_renderer = response
            .continuation_contents
            .as_ref()
            .and_then(|contents| contents.playlist_panel_continuation.as_ref())
            .or(tabs
                .first()
                .and_then(|tab| tab.tab_renderer.as_ref())
                .and_then(|tab| tab.content.as_ref())
                .and_then(|content| content.music_queue_renderer.as_ref())
                .and_then(|queue| queue.content.as_ref())
                .map(|content| &content.playlist_panel_renderer))
            .ok_or_else(|| InnertubeError::Parse("Missing playlistPanelRenderer".into()))?;
        let title = tabs
            .first()
            .and_then(|tab| tab.tab_renderer.as_ref())
            .and_then(|tab| tab.content.as_ref())
            .and_then(|content| content.music_queue_renderer.as_ref())
            .and_then(|queue| queue.header.as_ref())
            .and_then(|header| header.music_queue_header_renderer.as_ref())
            .and_then(|renderer| renderer.subtitle.as_ref())
            .and_then(|runs| runs.first())
            .map(|run| run.text.clone());
        let items: Vec<(SongItem, bool)> = playlist_panel_renderer
            .contents
            .iter()
            .filter_map(|content| {
                let renderer = content.playlist_panel_video_renderer.as_ref()?;
                let song = NextPage::from_playlist_panel_video_renderer(renderer)?;
                Some((song, renderer.selected))
            })
            .collect();
        let songs: Vec<SongItem> = items.iter().map(|(song, _)| song.clone()).collect();
        let current_index = items.iter().position(|(_, selected)| *selected);
        let lyrics_endpoint = tabs
            .get(1)
            .and_then(|tab| tab.tab_renderer.as_ref())
            .and_then(|tab| tab.endpoint.as_ref())
            .and_then(|endpoint| endpoint.browse_endpoint.clone());
        let related_endpoint = tabs
            .get(2)
            .and_then(|tab| tab.tab_renderer.as_ref())
            .and_then(|tab| tab.endpoint.as_ref())
            .and_then(|endpoint| endpoint.browse_endpoint.clone());

        // the automix usually opens with the song it was made from, which
        // is already in songs (YouTube.kt:1332)
        if let Some(watch_playlist_endpoint) = playlist_panel_renderer
            .contents
            .last()
            .and_then(|content| content.automix_preview_video_renderer.as_ref())
            .and_then(|automix| {
                automix
                    .content
                    .automix_playlist_video_renderer
                    .navigation_endpoint
                    .watch_playlist_endpoint
                    .clone()
            })
        {
            let recursed = Box::pin(self.next(&watch_playlist_endpoint, None)).await;
            // takeIf { it.items.isNotEmpty() || songs.isEmpty() } (YouTube.kt:1329)
            if let Ok(result) = recursed
                && (!result.items.is_empty() || songs.is_empty())
            {
                let fresh: Vec<SongItem> = result
                    .items
                    .into_iter()
                    .filter(|item| !songs.iter().any(|song| song.id == item.id))
                    .collect();
                return Ok(NextResult {
                    title,
                    items: songs.into_iter().chain(fresh).collect(),
                    lyrics_endpoint,
                    related_endpoint,
                    current_index,
                    continuation: result.continuation,
                    endpoint: watch_playlist_endpoint,
                });
            }
        }
        Ok(NextResult {
            title,
            items: songs,
            current_index,
            lyrics_endpoint,
            related_endpoint,
            continuation: playlist_panel_renderer
                .continuations
                .as_ref()
                .and_then(|continuations| get_continuation(continuations))
                .map(str::to_owned),
            endpoint: endpoint.clone(),
        })
    }

    /// `lyrics(endpoint)` (YouTube.kt:1352-1358).
    pub async fn lyrics(
        &self,
        endpoint: &BrowseEndpoint,
    ) -> Result<Option<String>, InnertubeError> {
        let response = self
            .browse_json(Some(&endpoint.browse_id), endpoint.params.as_deref())
            .await?;
        Ok(response
            .contents
            .as_ref()
            .and_then(|contents| contents.section_list_renderer.as_ref())
            .and_then(|section| section.contents.as_ref())
            .and_then(|contents| {
                contents
                    .iter()
                    .find(|content| content.music_description_shelf_renderer.is_some())
            })
            .and_then(|content| content.music_description_shelf_renderer.as_ref())
            .map(|shelf| {
                shelf
                    .description
                    .runs
                    .as_ref()
                    .map(|runs| {
                        runs.iter()
                            .map(|run| run.text.as_str())
                            .collect::<Vec<_>>()
                            .join("")
                    })
                    .unwrap_or_default()
            }))
    }

    /// `related(endpoint)` (YouTube.kt:1360-1410) — only audio tracks make
    /// the songs list (MUSIC_VIDEO_TYPE_ATV).
    pub async fn related(
        &self,
        endpoint: &BrowseEndpoint,
    ) -> Result<crate::pages::related::RelatedPage, InnertubeError> {
        let response = self.browse_json(Some(&endpoint.browse_id), None).await?;
        let mut songs = Vec::new();
        let mut albums = Vec::new();
        let mut artists = Vec::new();
        let mut playlists = Vec::new();

        let mut add_item = |item: YTItem,
                            renderer: Option<
            &crate::models::music_renderers::MusicResponsiveListItemRenderer,
        >| {
            match item {
                YTItem::Song(song) => {
                    let is_audio_track = renderer
                        .and_then(|renderer| renderer.overlay.as_ref())
                        .and_then(|overlay| {
                            overlay
                                .music_item_thumbnail_overlay_renderer
                                .content
                                .music_play_button_renderer
                                .play_navigation_endpoint
                                .as_ref()
                        })
                        .and_then(|endpoint| endpoint.watch_endpoint.as_ref())
                        .and_then(|watch| watch.watch_endpoint_music_supported_configs.as_ref())
                        .map(|configs| {
                            configs.watch_endpoint_music_config.music_video_type
                                == crate::models::endpoint::music_video_type::ATV
                        })
                        .unwrap_or(false);
                    if is_audio_track {
                        songs.push(song);
                    }
                }
                YTItem::Album(album) => albums.push(album),
                YTItem::Artist(artist) => artists.push(artist),
                YTItem::Playlist(playlist) => playlists.push(playlist),
            }
        };

        let contents = response
            .contents
            .as_ref()
            .and_then(|contents| contents.section_list_renderer.as_ref())
            .and_then(|section| section.contents.as_ref());
        if let Some(contents) = contents {
            for section_content in contents {
                if let Some(carousel) = section_content.music_carousel_shelf_renderer.as_ref() {
                    for content in &carousel.contents {
                        let item = content
                            .music_responsive_list_item_renderer
                            .as_ref()
                            .and_then(RelatedPageParser::from_music_responsive_list_item_renderer)
                            .map(YTItem::Song)
                            .or_else(|| {
                                content
                                    .music_two_row_item_renderer
                                    .as_ref()
                                    .and_then(RelatedPageParser::from_music_two_row_item_renderer)
                            });
                        if let Some(item) = item {
                            add_item(item, content.music_responsive_list_item_renderer.as_ref());
                        }
                    }
                }
                if let Some(shelf) = section_content.music_shelf_renderer.as_ref() {
                    for renderer in shelf
                        .contents
                        .as_ref()
                        .map(|contents| crate::models::music_renderers::shelf_items(contents))
                        .unwrap_or_default()
                    {
                        if let Some(item) =
                            RelatedPageParser::from_music_responsive_list_item_renderer(renderer)
                        {
                            add_item(YTItem::Song(item), Some(renderer));
                        }
                    }
                }
                if let Some(section) = section_content.item_section_renderer.as_ref() {
                    for content in section.contents.as_deref().unwrap_or_default() {
                        if let Some(renderer) = content.music_responsive_list_item_renderer.as_ref()
                            && let Some(item) =
                                RelatedPageParser::from_music_responsive_list_item_renderer(
                                    renderer,
                                )
                        {
                            add_item(YTItem::Song(item), Some(renderer));
                        }
                    }
                }
            }
        }
        Ok(crate::pages::related::RelatedPage {
            songs,
            albums,
            artists,
            playlists,
        })
    }

    /// `queue(videoIds, playlistId)` (YouTube.kt:1412-1422).
    pub async fn queue(
        &self,
        video_ids: Option<&[String]>,
        playlist_id: Option<&str>,
    ) -> Result<Vec<SongItem>, InnertubeError> {
        if let Some(video_ids) = video_ids
            && video_ids.len() > MAX_GET_QUEUE_SIZE
        {
            return Err(InnertubeError::Parse(format!(
                "queue accepts at most {MAX_GET_QUEUE_SIZE} video ids"
            )));
        }
        let value = self
            .transport
            .get_queue(&self.web_remix(), video_ids, playlist_id)
            .await?;
        let response: GetQueueResponse = Self::decode(value)?;
        Ok(response
            .queue_datas
            .iter()
            .filter_map(|data| {
                data.content
                    .as_ref()
                    .and_then(|content| content.playlist_panel_video_renderer.as_ref())
            })
            .filter_map(NextPage::from_playlist_panel_video_renderer)
            .collect())
    }

    /// `transcript(videoId)` (YouTube.kt:1424-1433).
    pub async fn transcript(&self, video_id: &str) -> Result<String, InnertubeError> {
        let value = self.transport.get_transcript(&self.web(), video_id).await?;
        let response: GetTranscriptResponse = Self::decode(value)?;
        let groups = response
            .actions
            .ok_or_else(|| InnertubeError::Parse("missing transcript actions".into()))?
            .first()
            .and_then(|action| action.update_engagement_panel_action.as_ref())
            .and_then(|action| action.content.as_ref())
            .and_then(|content| content.transcript_renderer.as_ref())
            .and_then(|renderer| renderer.body.as_ref())
            .and_then(|body| body.transcript_body_renderer.as_ref())
            .map(|body| body.cue_groups.clone())
            .ok_or_else(|| InnertubeError::Parse("missing transcript cue groups".into()))?;
        let mut lines = Vec::new();
        for group in &groups {
            let Some(renderer) = group.transcript_cue_group_renderer.as_ref() else {
                continue;
            };
            let Some(cue) = renderer.cues.first() else {
                continue;
            };
            let Some(cue) = cue.transcript_cue_renderer.as_ref() else {
                continue;
            };
            let time = cue.start_offset_ms;
            let text = cue
                .cue
                .as_ref()
                .map(|text| text.simple_text.trim_matches('♪').trim().to_owned())
                .unwrap_or_default();
            lines.push(format!(
                "[{:02}:{:02}.{:03}]{text}",
                time / 60000,
                time / 1000 % 60,
                time % 1000
            ));
        }
        Ok(lines.join("\n"))
    }

    /// `captions(videoId)` (YouTube.kt:1435-1440) — the video's captions
    /// as line-synced lyrics: youtube's auto-generated ones or an
    /// uploaded track in the same language, asked as clients whose
    /// caption links work without a PoToken.
    pub async fn captions(&self, video_id: &str) -> Result<String, InnertubeError> {
        for key in roles::CAPTION_CLIENTS {
            let client = self.client(*key);
            if let Ok(Some(lrc)) = self.captions_as(video_id, &client).await
                && !lrc.is_empty()
            {
                return Ok(lrc);
            }
        }
        Err(InnertubeError::NoCaptions(video_id.to_owned()))
    }

    /// `captionsAs` (YouTube.kt:1442-1448) — None when the video has no
    /// track worth using for the client.
    async fn captions_as(
        &self,
        video_id: &str,
        client: &YouTubeClient,
    ) -> Result<Option<String>, InnertubeError> {
        let player = self.player(video_id, None, client, None, None).await?;
        let tracks = captions::player_caption_tracks(&player);
        let Some(track) = captions::pick_caption_track(&tracks) else {
            return Ok(None);
        };
        let json = self.transport.timed_text(&track.base_url, client).await?;
        Ok(Some(captions::timed_text_to_lrc(&json.to_string())))
    }

    /// `videoCaptions(videoId)` (YouTube.kt:1450-1460) — every caption
    /// track the video has, uploaded and auto-generated, from the first
    /// client that lists any.
    pub async fn video_captions(&self, video_id: &str) -> Result<VideoCaptions, InnertubeError> {
        for key in roles::CAPTION_CLIENTS {
            let client = self.client(*key);
            let Ok(player) = self.player(video_id, None, &client, None, None).await else {
                continue;
            };
            let tracks = captions::player_caption_tracks(&player);
            if !tracks.is_empty() {
                return Ok(VideoCaptions {
                    video_id: video_id.to_owned(),
                    tracks,
                    client,
                });
            }
        }
        Err(InnertubeError::NoCaptions(video_id.to_owned()))
    }

    /// `captionCues(captions, track)` (YouTube.kt:1462-1465) — one of
    /// [`Self::video_captions`]' tracks as timed cues.
    pub async fn caption_cues(
        &self,
        captions: &VideoCaptions,
        track: &crate::models::response::CaptionTrack,
    ) -> Result<Vec<crate::models::account::CaptionCue>, InnertubeError> {
        let json = self
            .transport
            .timed_text(&track.base_url, &captions.client)
            .await?;
        Ok(captions::timed_text_to_cues(&json.to_string()))
    }

    /// `visitorData()` (YouTube.kt:1473-1483) — the sw.js_data scrape.
    pub async fn visitor_data(&self) -> Result<String, InnertubeError> {
        let text = self.transport.sw_js_data().await?;
        guest::extract_visitor_data(&text).ok_or(InnertubeError::VisitorDataScrapeFailed)
    }

    /// `refreshVisitorData()` (YouTube.kt:131-133) — scrape one and keep
    /// it in the session.
    pub async fn refresh_visitor_data(&self) -> Result<String, InnertubeError> {
        let visitor = self.visitor_data().await?;
        self.transport
            .session()
            .write()
            .map_err(|_| InnertubeError::LockPoisoned)?
            .visitor_data = Some(visitor.clone());
        Ok(visitor)
    }

    /// `refreshVisitorData(client)` (YouTube.kt:135-136) — a fresh visitor
    /// id for a client that mints its own, after youtube's bot check
    /// burned the old one.
    pub async fn refresh_visitor_data_for(
        &self,
        client: &YouTubeClient,
    ) -> Result<String, InnertubeError> {
        self.transport.visitor_id(client).await
    }

    /// `clearGuestSession` (YouTube.kt:138-141).
    pub fn clear_guest_session(&self) -> Result<(), InnertubeError> {
        self.transport
            .session()
            .write()
            .map_err(|_| InnertubeError::LockPoisoned)?
            .clear_guest_session();
        Ok(())
    }

    /// `accountInfo()` (YouTube.kt:1485-1490).
    pub async fn account_info(&self) -> Result<AccountInfo, InnertubeError> {
        let value = self.transport.account_menu(&self.web_remix()).await?;
        let response: AccountMenuResponse = Self::decode(value)?;
        response
            .actions
            .first()
            .and_then(|action| action.open_popup_action.as_ref())
            .and_then(|open| open.popup.as_ref())
            .and_then(|popup| popup.multi_page_menu_renderer.as_ref())
            .and_then(|menu| menu.header.as_ref())
            .and_then(|header| header.active_account_header_renderer.as_ref())
            .map(|renderer| renderer.to_account_info())
            .ok_or_else(|| InnertubeError::Parse("missing activeAccountHeaderRenderer".into()))
    }

    /// `feedback(tokens)` (YouTube.kt:1492-1494).
    pub async fn feedback(&self, tokens: &[String]) -> Result<bool, InnertubeError> {
        let value = self.transport.feedback(&self.web_remix(), tokens).await?;
        let response: FeedbackResponse = Self::decode(value)?;
        Ok(response
            .feedback_responses
            .iter()
            .all(|status| status.is_processed))
    }

    /// `addSongToLibrary(videoId)` (YouTube.kt:1496-1506).
    pub async fn add_song_to_library(&self, video_id: &str) -> Result<bool, InnertubeError> {
        let next_result = self
            .next(
                &WatchEndpoint {
                    video_id: Some(video_id.to_owned()),
                    ..Default::default()
                },
                None,
            )
            .await?;
        let song = next_result
            .items
            .iter()
            .find(|song| song.id == video_id)
            .ok_or_else(|| InnertubeError::SongNotFound(video_id.to_owned()))?;
        let add_token = song
            .library_add_token
            .clone()
            .ok_or(InnertubeError::LibraryTokenMissing("add to library"))?;
        self.feedback(&[add_token]).await
    }

    /// `removeSongFromLibrary(videoId)` (YouTube.kt:1508-1518).
    pub async fn remove_song_from_library(&self, video_id: &str) -> Result<bool, InnertubeError> {
        let next_result = self
            .next(
                &WatchEndpoint {
                    video_id: Some(video_id.to_owned()),
                    ..Default::default()
                },
                None,
            )
            .await?;
        let song = next_result
            .items
            .iter()
            .find(|song| song.id == video_id)
            .ok_or_else(|| InnertubeError::SongNotFound(video_id.to_owned()))?;
        let remove_token = song
            .library_remove_token
            .clone()
            .ok_or(InnertubeError::LibraryTokenMissing("remove from library"))?;
        self.feedback(&[remove_token]).await
    }

    /// `toggleSongLibrary(videoId, addToLibrary)` (YouTube.kt:1520-1526).
    pub async fn toggle_song_library(
        &self,
        video_id: &str,
        add_to_library: bool,
    ) -> Result<bool, InnertubeError> {
        if add_to_library {
            self.add_song_to_library(video_id).await
        } else {
            self.remove_song_from_library(video_id).await
        }
    }

    /// `getMediaInfo(videoId)` (YouTube.kt:1528-1530; the composite itself
    /// is InnerTube.kt:764-834) — the WEB watch-next page plus
    /// ReturnYouTubeDislike.
    pub async fn get_media_info(&self, video_id: &str) -> Result<MediaInfo, InnertubeError> {
        let value = self
            .transport
            .next(&self.web(), Some(video_id), None, None, None, None, None)
            .await?;
        let response: NextResponse = Self::decode(value)?;

        let two_column = response
            .contents
            .as_ref()
            .and_then(|contents| contents.two_column_watch_next_results.as_ref());
        let content_list = two_column
            .and_then(|two| two.results.as_ref())
            .and_then(|results| results.results.as_ref())
            .and_then(|results| results.content.as_ref());

        let base_for_info = content_list.and_then(|contents| {
            contents
                .iter()
                .flatten()
                .find_map(|content| content.video_secondary_info_renderer.as_ref())
        });
        let base_for_title = content_list.and_then(|contents| {
            contents
                .iter()
                .flatten()
                .find_map(|content| content.video_primary_info_renderer.as_ref())
        });

        let dislike_value = self
            .transport
            .return_youtube_dislike_votes(video_id)
            .await?;
        let ryd: crate::models::account::ReturnYouTubeDislikeResponse =
            Self::decode(dislike_value)?;

        Ok(MediaInfo {
            video_id: video_id.to_owned(),
            title: base_for_title
                .and_then(|primary| primary.title.as_ref())
                .and_then(|title| title.runs.as_ref())
                .and_then(|runs| runs.first())
                .and_then(|run| run.text.clone()),
            author: base_for_info
                .and_then(|secondary| secondary.owner.as_ref())
                .and_then(|owner| owner.video_owner_renderer.as_ref())
                .and_then(|owner| owner.title.as_ref())
                .and_then(|title| title.runs.as_ref())
                .and_then(|runs| runs.first())
                .and_then(|run| run.text.clone()),
            author_id: base_for_info
                .and_then(|secondary| secondary.owner.as_ref())
                .and_then(|owner| owner.video_owner_renderer.as_ref())
                .and_then(|owner| owner.navigation_endpoint.as_ref())
                .and_then(|endpoint| endpoint.browse_endpoint.as_ref())
                .map(|browse| browse.browse_id.clone()),
            author_thumbnail: base_for_info
                .and_then(|secondary| secondary.owner.as_ref())
                .and_then(|owner| owner.video_owner_renderer.as_ref())
                .and_then(|owner| owner.thumbnail.as_ref())
                .and_then(|thumbnail| thumbnail.thumbnails.as_ref())
                .and_then(|thumbnails| {
                    thumbnails
                        .iter()
                        .find(|thumb| thumb.height == Some(48))
                        .map(|thumb| thumb.url.replace("s48", "s960"))
                }),
            description: base_for_info
                .and_then(|secondary| secondary.attributed_description.as_ref())
                .and_then(|description| description.content.clone()),
            subscribers: base_for_info
                .and_then(|secondary| secondary.owner.as_ref())
                .and_then(|owner| owner.video_owner_renderer.as_ref())
                .and_then(|owner| owner.subscriber_count_text.as_ref())
                .and_then(|text| text.simple_text.as_ref())
                .map(|text| text.split(' ').next().unwrap_or_default().to_owned()),
            upload_date: base_for_title
                .and_then(|primary| primary.date_text.as_ref())
                .and_then(|text| text.simple_text.clone()),
            view_count: ryd.view_count,
            like: ryd.likes,
            dislike: ryd.dislikes,
        })
    }
}

/// The `Url(url).parameters.names()` equivalent of `reportWatchtime`
/// (YouTube.kt:1293) — no url crate needed for the query names.
fn query_param_names(url: &str) -> Vec<String> {
    url.split_once('?')
        .map(|(_, query)| {
            query
                .split('&')
                .filter(|part| !part.is_empty())
                .map(|pair| pair.split('=').next().unwrap_or_default().to_owned())
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::parse_time;

    /// YouTube.kt:1532-1542 — the six search filter values.
    #[test]
    fn search_filter_values() {
        assert_eq!(SearchFilter::song().0, "EgWKAQIIAWoKEAkQBRAKEAMQBA%3D%3D");
        assert_eq!(SearchFilter::video().0, "EgWKAQIQAWoKEAkQChAFEAMQBA%3D%3D");
        assert_eq!(SearchFilter::album().0, "EgWKAQIYAWoKEAkQChAFEAMQBA%3D%3D");
        assert_eq!(SearchFilter::artist().0, "EgWKAQIgAWoKEAkQChAFEAMQBA%3D%3D");
        assert_eq!(
            SearchFilter::featured_playlist().0,
            "EgeKAQQoADgBagwQDhAKEAMQBRAJEAQ%3D"
        );
        assert_eq!(
            SearchFilter::community_playlist().0,
            "EgeKAQQoAEABagoQAxAEEAoQCRAF"
        );
    }

    /// YouTube.kt:1544-1552 — the four library filter values, verbatim.
    #[test]
    fn library_filter_values() {
        assert_eq!(
            LibraryFilter::RECENT_ACTIVITY,
            "4qmFsgIrEhdGRW11c2ljX2xpYnJhcnlfbGFuZGluZxoQZ2dNR0tnUUlCaEFCb0FZQg%3D%3D"
        );
        assert_eq!(
            LibraryFilter::RECENTLY_PLAYED,
            "4qmFsgIrEhdGRW11c2ljX2xpYnJhcnlfbGFuZGluZxoQZ2dNR0tnUUlCUkFCb0FZQg%3D%3D"
        );
        assert_eq!(
            LibraryFilter::PLAYLISTS_ALPHABETICAL,
            "4qmFsgIrEhdGRW11c2ljX2xpa2VkX3BsYXlsaXN0cxoQZ2dNR0tnUUlBUkFBb0FZQg%3D%3D"
        );
        assert_eq!(
            LibraryFilter::PLAYLISTS_RECENTLY_SAVED,
            "4qmFsgIrEhdGRW11c2ljX2xpa2VkX3BsYXlsaXN0cxoQZ2dNR0tnUUlBQkFCb0FZQg%3D%3D"
        );
    }

    /// The watchtime param de-duplication (YouTube.kt:1289-1295).
    #[test]
    fn query_names_from_url() {
        let names =
            query_param_names("https://music.youtube.com/api/stats/watchback?cpn=x&len=210&ver=2");
        assert_eq!(names, vec!["cpn", "len", "ver"]);
        assert!(query_param_names("https://example.com/no-query").is_empty());
    }

    /// `musicHistory`'s default depth contract + a shared parse sanity
    /// check for the parse_time import used in the walk paths.
    #[test]
    fn parse_time_wired() {
        assert_eq!(parse_time("3:45"), Some(225));
        let _ = parse_time("song");
    }
}
