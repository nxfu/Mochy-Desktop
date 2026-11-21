//! Parser tests over the committed fixture corpus — the Phase 05
//! acceptance criterion: "every pages/ parser has at least one fixture
//! test" + "serde round-trip tests".
//!
//! The fixtures are SYNTHETIC (structurally faithful minimal payloads
//! hand-built from the upstream parser expectations; see
//! tests/fixtures/README.md). Every test decodes the fixture into the
//! DTO layer and runs the pure parser, asserting the domain result —
//! no IO, no clock, no randomness.

mod common;

use common::load_fixture;
use mochy_innertube::captions;
use mochy_innertube::models::ReturnYouTubeDislikeResponse;
use mochy_innertube::models::common::get_continuation;
use mochy_innertube::models::response::{
    AccountMenuResponse, BrowseResponse, GetQueueResponse, GetSearchSuggestionsResponse,
    GetTranscriptResponse, NextResponse, PlayerResponse, SearchResponse,
};
use mochy_innertube::models::ytitem::YTItem;
use mochy_innertube::pages::album::AlbumPageParser;
use mochy_innertube::pages::artist::ArtistPageParser;
use mochy_innertube::pages::artist_items::ArtistItemsPageParser;
use mochy_innertube::pages::browse_result::BrowseResult;
use mochy_innertube::pages::history::HistoryPageParser;
use mochy_innertube::pages::home::{Chip, HomeSection};
use mochy_innertube::pages::library::LibraryPageParser;
use mochy_innertube::pages::mood_genres::MoodAndGenresParser;
use mochy_innertube::pages::new_release::NewReleaseAlbumPage;
use mochy_innertube::pages::next::NextPage;
use mochy_innertube::pages::playlist::PlaylistPageParser;
use mochy_innertube::pages::related::RelatedPageParser;
use mochy_innertube::pages::search::SearchPage;
use mochy_innertube::pages::search_suggestion::SearchSuggestionPage;
use mochy_innertube::pages::search_summary::SearchSummaryPageParser;
use mochy_innertube::utils::parse_time;

fn artist_name_id(artist: &mochy_innertube::models::ytitem::Artist) -> (String, Option<String>) {
    (artist.name.clone(), artist.id.clone())
}

fn fixture(name: &str) -> serde_json::Value {
    load_fixture(name).unwrap_or_else(|| panic!("fixture {name} missing"))
}

/// The single-column tab content walk every browse-based test starts
/// with (the facade's own first step).
fn single_column_contents(
    response: &BrowseResponse,
) -> Vec<mochy_innertube::models::section_list::Content> {
    response
        .contents
        .as_ref()
        .and_then(|contents| contents.single_column_browse_results_renderer.as_ref())
        .and_then(|single| single.tabs.first())
        .and_then(|tab| tab.tab_renderer.as_ref())
        .and_then(|tab| tab.content.as_ref())
        .and_then(|content| content.section_list_renderer.as_ref())
        .and_then(|section| section.contents.as_ref().cloned())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// SearchPage + SearchSummaryPage + SearchSuggestionPage — search.json
// ---------------------------------------------------------------------------

/// SearchPage.kt:21-200 — the shelf song carries artists, album, duration
/// and the library tokens; the shelf artist carries shuffle/radio.
#[test]
fn search_page_parses_song_and_artist() {
    let response: SearchResponse =
        serde_json::from_value(fixture("search")).expect("decode search.json");
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
        })
        .expect("musicShelfRenderer");
    let items = mochy_innertube::models::music_renderers::shelf_items(
        shelf.contents.as_deref().unwrap_or_default(),
    );
    let song = SearchPage::to_yt_item(items[0]).expect("song parses");
    let YTItem::Song(song) = &song else {
        panic!("expected a song");
    };
    assert_eq!(song.id, "SONG1");
    assert_eq!(song.title, "Song One");
    assert_eq!(song.artists.len(), 1);
    assert_eq!(song.artists[0].name, "Artist A");
    assert_eq!(song.artists[0].id.as_deref(), Some("UCA1"));
    assert_eq!(
        song.album.as_ref().map(|album| album.id.as_str()),
        Some("MPREb_albumA")
    );
    assert_eq!(song.duration, Some(225));
    assert!(song.explicit);
    assert_eq!(song.library_add_token.as_deref(), Some("ADD_SONG1"));
    assert_eq!(song.library_remove_token.as_deref(), Some("REMOVE_SONG1"));

    let artist = SearchPage::to_yt_item(items[1]).expect("artist parses");
    let YTItem::Artist(artist) = &artist else {
        panic!("expected an artist");
    };
    assert_eq!(artist.id, "UCB2");
    assert_eq!(artist.title, "Artist B");
    assert_eq!(
        artist
            .shuffle_endpoint
            .as_ref()
            .and_then(|e| e.playlist_id.as_deref()),
        Some("RDAEM_ARTB")
    );
    assert_eq!(
        artist
            .radio_endpoint
            .as_ref()
            .and_then(|e| e.playlist_id.as_deref()),
        Some("RDM_ARTB")
    );

    // the shelf's own continuation rides nextContinuationData
    assert_eq!(
        shelf
            .continuations
            .as_ref()
            .and_then(|continuations| get_continuation(continuations)),
        Some("SEARCH_CONT_NEXT")
    );
}

/// SearchPage.kt:104-143 + 144-197 — the flat item-section album and the
/// continuation playlist.
#[test]
fn search_page_parses_album_and_playlist() {
    let response: SearchResponse =
        serde_json::from_value(fixture("search")).expect("decode search.json");
    let contents = response
        .contents
        .as_ref()
        .and_then(|contents| contents.tabbed_search_results_renderer.as_ref())
        .and_then(|tabs| tabs.first())
        .and_then(|tab| tab.content.as_ref())
        .and_then(|content| content.section_list_renderer.as_ref())
        .and_then(|section| section.contents.as_ref().cloned())
        .unwrap_or_default();
    let album = contents
        .iter()
        .find_map(|content| content.item_section_renderer.as_ref())
        .and_then(|section| section.contents.as_ref())
        .and_then(|contents| contents.first())
        .and_then(|content| content.music_responsive_list_item_renderer.as_ref())
        .and_then(SearchPage::to_yt_item)
        .expect("album parses");
    let YTItem::Album(album) = &album else {
        panic!("expected an album");
    };
    assert_eq!(album.browse_id, "MPREb_albumC");
    assert_eq!(album.playlist_id, "OLAK5uy_albumC");
    assert_eq!(album.year, Some(2021));
    assert_eq!(
        album.artists.as_ref().unwrap()[0].id.as_deref(),
        Some("UCC3")
    );

    let playlist = response
        .continuation_contents
        .as_ref()
        .and_then(|contents| contents.music_shelf_continuation.as_ref())
        .and_then(|shelf| shelf.contents.first())
        .and_then(|content| content.music_responsive_list_item_renderer.as_ref())
        .and_then(SearchPage::to_yt_item)
        .expect("playlist parses");
    let YTItem::Playlist(playlist) = &playlist else {
        panic!("expected a playlist");
    };
    assert_eq!(playlist.id, "PL_playlist1"); // VL stripped
    assert_eq!(playlist.song_count_text.as_deref(), Some("50 songs"));
    // the author is the FIRST RUN of the first segment (SearchPage.kt:
    // 159-165) — "Playlist", no id; the owner's link lives later
    assert_eq!(
        playlist.author.as_ref().map(artist_name_id),
        Some(("Playlist".to_owned(), None))
    );
}

/// SearchSummaryPage.kt:82-210 — the top-result card song.
#[test]
fn search_summary_parses_the_top_result_card() {
    let response: SearchResponse =
        serde_json::from_value(fixture("search")).expect("decode search.json");
    let card = response
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
                .find_map(|content| content.music_card_shelf_renderer.as_ref())
        })
        .expect("musicCardShelfRenderer");
    let top = SearchSummaryPageParser::from_music_card_shelf_renderer(card).expect("card parses");
    let YTItem::Song(song) = &top else {
        panic!("expected the card to be a song");
    };
    assert_eq!(song.id, "SONG1");
    assert_eq!(song.artists[0].name, "Artist A");
    assert_eq!(song.duration, Some(225));
    assert!(song.explicit);
    // the card's own shelf item parses with the card artist fallback
    let inner = card.contents.as_ref().unwrap()[0]
        .music_responsive_list_item_renderer
        .as_ref()
        .unwrap();
    let inner_item = SearchSummaryPageParser::from_music_responsive_list_item_renderer(inner, None)
        .expect("card shelf item parses");
    assert_eq!(inner_item.id(), "SONG1");
}

/// SearchSuggestionPage.kt:15-151 + the suggestions response — typed
/// queries and recommended song items.
#[test]
fn search_suggestions_parse() {
    let response: GetSearchSuggestionsResponse =
        serde_json::from_value(fixture("search_suggestions")).expect("decode");
    let sections = response.contents.unwrap_or_default();
    let queries: Vec<String> = sections[0]
        .search_suggestions_section_renderer
        .as_ref()
        .unwrap()
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
        .collect();
    assert_eq!(queries, vec!["artist a", "artist a songs"]);

    let recommended = sections[1]
        .search_suggestions_section_renderer
        .as_ref()
        .unwrap()
        .contents
        .iter()
        .filter_map(|content| content.music_responsive_list_item_renderer.as_ref())
        .filter_map(SearchSuggestionPage::from_music_responsive_list_item_renderer)
        .collect::<Vec<_>>();
    assert_eq!(recommended.len(), 1);
    assert_eq!(recommended[0].id(), "SONG1");
    let YTItem::Song(song) = &recommended[0] else {
        panic!("expected a song");
    };
    // the suggestion song keeps its album through the third column
    assert_eq!(
        song.album.as_ref().map(|album| album.id.as_str()),
        Some("MPREb_albumA")
    );
}

// ---------------------------------------------------------------------------
// HomePage — home.json
// ---------------------------------------------------------------------------

/// HomePage.kt:52-81 — carousel sections, the strapline label, the
/// speed-dial heuristic and the chip cloud.
#[test]
fn home_page_parses_sections_and_chips() {
    let response: BrowseResponse =
        serde_json::from_value(fixture("home")).expect("decode home.json");
    let contents = single_column_contents(&response);
    let sections: Vec<HomeSection> = contents
        .iter()
        .filter_map(|content| content.music_carousel_shelf_renderer.as_ref())
        .filter_map(HomeSection::from_music_carousel_shelf_renderer)
        .collect();
    assert_eq!(sections.len(), 2);

    assert_eq!(sections[0].title, "Quick picks");
    assert_eq!(
        sections[0].label.as_deref(),
        Some("Because you like Artist A")
    );
    assert_eq!(sections[0].items.len(), 2);
    assert_eq!(sections[0].items[0].id(), "SONG1");
    assert_eq!(sections[0].items[1].id(), "MPREb_albumA");
    assert!(!sections[0].is_speed_dial);

    // the title is translated, so a three-row grid of covers under your
    // own avatar counts too (HomePage.kt:71-81)
    assert_eq!(sections[1].title, "Speed dial");
    assert!(sections[1].is_speed_dial);

    let section_list = response
        .contents
        .as_ref()
        .and_then(|contents| contents.single_column_browse_results_renderer.as_ref())
        .and_then(|single| single.tabs.first())
        .and_then(|tab| tab.tab_renderer.as_ref())
        .and_then(|tab| tab.content.as_ref())
        .and_then(|content| content.section_list_renderer.as_ref())
        .unwrap();
    let chips: Vec<Chip> = section_list
        .header
        .as_ref()
        .and_then(|header| header.chip_cloud_renderer.as_ref())
        .map(|cloud| {
            cloud
                .chips
                .iter()
                .filter_map(|chip| {
                    chip.chip_cloud_chip_renderer
                        .as_ref()
                        .and_then(Chip::from_chip_cloud_chip_renderer)
                })
                .collect()
        })
        .unwrap_or_default();
    assert_eq!(chips.len(), 2);
    assert_eq!(chips[0].title, "Listen now");
    assert_eq!(chips[1].title, "Podcasts");
    assert!(chips[1].deselect_endpoint.is_some());
    assert_eq!(
        section_list
            .continuations
            .as_ref()
            .and_then(|continuations| get_continuation(continuations)),
        Some("HOME_CONT_NEXT")
    );
}

// ---------------------------------------------------------------------------
// AlbumPage — album.json
// ---------------------------------------------------------------------------

/// AlbumPage.kt:24-120 — title/year/artists/thumbnail/playlistId header
/// walks + the songs shelf (with the library tokens) + the continuation.
#[test]
fn album_page_parses_header_and_songs() {
    let response: BrowseResponse =
        serde_json::from_value(fixture("album")).expect("decode album.json");
    assert_eq!(
        AlbumPageParser::get_playlist_id(&response).as_deref(),
        Some("OLAK5uy_albumA")
    );
    assert_eq!(
        AlbumPageParser::get_title(&response).as_deref(),
        Some("Album A")
    );
    assert_eq!(AlbumPageParser::get_year(&response), Some(2021));
    assert_eq!(
        AlbumPageParser::get_thumbnail(&response).as_deref(),
        Some("https://i.ytimg.com/vi/ALBA/544.jpg")
    );
    let artists = AlbumPageParser::get_artists(&response);
    assert_eq!(artists.len(), 1);
    assert_eq!(artists[0].name, "Artist A");
    assert_eq!(artists[0].id.as_deref(), Some("UCA1"));

    // the two-column secondary contents carry the shelf continuation and
    // the other-versions carousel
    let two_column = response
        .contents
        .as_ref()
        .and_then(|contents| contents.two_column_browse_results_renderer.as_ref())
        .unwrap();
    let secondary = two_column
        .secondary_contents
        .as_ref()
        .and_then(|secondary| secondary.section_list_renderer.as_ref())
        .unwrap();
    let shelf = secondary.contents.as_ref().unwrap()[0]
        .music_playlist_shelf_renderer
        .as_ref()
        .unwrap();

    // getSongs reads musicShelfRenderer (AlbumPage.kt:76-77) — the
    // two-column secondary carries a musicPlaylistShelfRenderer, so the
    // songs themselves come through getSong over that shelf (the
    // albumSongs path, YouTube.kt:377-411)
    assert!(AlbumPageParser::get_songs(&response, None).is_empty());
    let songs: Vec<_> = mochy_innertube::models::music_renderers::shelf_items(&shelf.contents)
        .iter()
        .filter_map(|renderer| AlbumPageParser::get_song(renderer, None))
        .collect();
    assert_eq!(songs.len(), 1);
    assert_eq!(songs[0].id, "SONG1");
    assert_eq!(songs[0].title, "Song One");
    assert_eq!(songs[0].duration, Some(225));
    // the album is taken from the third flex column when no album item is
    // passed in (AlbumPage.kt:103-108)
    assert_eq!(
        songs[0].album.as_ref().map(|album| album.id.as_str()),
        Some("MPREb_albumA")
    );
    assert_eq!(songs[0].library_add_token.as_deref(), Some("ADD_SONG1"));

    assert_eq!(
        mochy_innertube::models::music_renderers::shelf_continuation(&shelf.contents),
        Some("ALBUM_SONGS_CONT".to_owned())
    );
    let versions: Vec<_> = secondary
        .contents
        .as_ref()
        .unwrap()
        .iter()
        .filter_map(|content| content.music_carousel_shelf_renderer.as_ref())
        .filter_map(|carousel| {
            carousel
                .contents
                .iter()
                .filter_map(|content| content.music_two_row_item_renderer.as_ref())
                .filter_map(NewReleaseAlbumPage::from_music_two_row_item_renderer)
                .collect::<Vec<_>>()
                .into_iter()
                .next()
        })
        .collect();
    assert_eq!(versions.len(), 2);
    assert_eq!(versions[0].browse_id, "MPREb_albumD"); // "Other versions"
    assert_eq!(versions[1].browse_id, "MPREb_albumE"); // "More from this artist"
}

// ---------------------------------------------------------------------------
// ArtistPage + ArtistItemsPage — artist.json / artist_items.json
// ---------------------------------------------------------------------------

/// ArtistPage.kt:38-208 — shelf/carousel sections with the more-endpoint.
#[test]
fn artist_page_parses_sections() {
    let response: BrowseResponse =
        serde_json::from_value(fixture("artist")).expect("decode artist.json");
    let sections: Vec<_> = single_column_contents(&response)
        .iter()
        .filter_map(ArtistPageParser::from_section_list_renderer_content)
        .collect();
    assert_eq!(sections.len(), 3);
    assert_eq!(sections[0].title, "Songs");
    assert_eq!(sections[0].items.len(), 1);
    assert_eq!(sections[0].items[0].id(), "SONG2");
    assert_eq!(
        sections[0]
            .more_endpoint
            .as_ref()
            .map(|endpoint| endpoint.params.as_deref()),
        Some(Some("more_songs"))
    );
    assert_eq!(sections[1].title, "Albums");
    assert_eq!(sections[1].items[0].id(), "MPREb_albumB");
    // the carousel with list rows mixes converters
    assert_eq!(sections[2].title, "Singles");
    assert_eq!(sections[2].items[0].id(), "SONG3");
    // the immersive header carries the subscription state
    let immersive = response
        .header
        .as_ref()
        .unwrap()
        .music_immersive_header_renderer
        .as_ref()
        .unwrap();
    assert!(
        immersive
            .subscription_button
            .as_ref()
            .unwrap()
            .subscribe_button_renderer
            .subscribed
    );
}

/// ArtistItemsPage.kt:73-128 + the grid continuation
/// (ArtistItemsPage.kt through ArtistItemsContinuationPage).
#[test]
fn artist_items_parses_grid_and_continuation() {
    let response: BrowseResponse = serde_json::from_value(fixture("artist_items")).expect("decode");
    let contents = single_column_contents(&response);
    let grid = contents[0].grid_renderer.as_ref().unwrap();
    let items: Vec<_> = grid
        .items
        .iter()
        .filter_map(|item| item.music_two_row_item_renderer.as_ref())
        .filter_map(ArtistItemsPageParser::from_music_two_row_item_renderer)
        .collect();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id(), "MPREb_albumB");
    assert_eq!(
        grid.continuations
            .as_ref()
            .and_then(|continuations| get_continuation(continuations)),
        Some("ARTIST_ITEMS_CONT")
    );

    let continuation = response
        .continuation_contents
        .as_ref()
        .unwrap()
        .grid_continuation
        .as_ref()
        .unwrap();
    let more: Vec<_> = continuation
        .items
        .iter()
        .filter_map(|item| item.music_two_row_item_renderer.as_ref())
        .filter_map(ArtistItemsPageParser::from_music_two_row_item_renderer)
        .collect();
    assert_eq!(more.len(), 1);
    assert_eq!(more[0].id(), "MPREb_albumF");
    // the radio-flavored continuation alias is read too
    assert_eq!(
        continuation
            .continuations
            .as_ref()
            .and_then(|continuations| get_continuation(continuations)),
        Some("ARTIST_ITEMS_CONT_2")
    );
}

// ---------------------------------------------------------------------------
// PlaylistPage — playlist.json
// ---------------------------------------------------------------------------

/// PlaylistPage.kt:22-61 — playlist songs REQUIRE their setVideoId (the
/// move/remove API needs it).
#[test]
fn playlist_page_songs_require_set_video_ids() {
    let response: BrowseResponse =
        serde_json::from_value(fixture("playlist")).expect("decode playlist.json");
    let two_column = response
        .contents
        .as_ref()
        .and_then(|contents| contents.two_column_browse_results_renderer.as_ref())
        .unwrap();
    let secondary = two_column
        .secondary_contents
        .as_ref()
        .and_then(|secondary| secondary.section_list_renderer.as_ref())
        .unwrap();
    let shelf = secondary.contents.as_ref().unwrap()[0]
        .music_playlist_shelf_renderer
        .as_ref()
        .unwrap();
    let songs: Vec<_> = mochy_innertube::models::music_renderers::shelf_items(&shelf.contents)
        .iter()
        .filter_map(|renderer| {
            PlaylistPageParser::from_music_responsive_list_item_renderer(renderer)
        })
        .collect();
    assert_eq!(songs.len(), 1);
    assert_eq!(songs[0].id, "SONG5");
    assert_eq!(songs[0].set_video_id.as_deref(), Some("SET_SONG5"));
    assert_eq!(songs[0].duration, Some(200));
    assert_eq!(
        songs[0].album.as_ref().map(|album| album.id.as_str()),
        Some("MPREb_albumG")
    );
    assert_eq!(
        mochy_innertube::models::music_renderers::shelf_continuation(&shelf.contents),
        Some("PLAYLIST_SONGS_CONT".to_owned())
    );
    // the related carousel of the secondary contents parses as albums
    let related: Vec<_> = secondary.contents.as_ref().unwrap()[1..]
        .iter()
        .filter_map(|content| content.music_carousel_shelf_renderer.as_ref())
        .flat_map(|carousel| {
            carousel
                .contents
                .iter()
                .filter_map(|content| content.music_two_row_item_renderer.as_ref())
                .filter_map(RelatedPageParser::from_music_two_row_item_renderer)
                .collect::<Vec<_>>()
        })
        .collect();
    assert_eq!(related.len(), 1);
    assert_eq!(related[0].id(), "MPREb_albumH");
}

// ---------------------------------------------------------------------------
// NextPage — next.json
// ---------------------------------------------------------------------------

/// NextPage.kt:25-74 — the queue panel songs with byline artists, album
/// and library tokens.
#[test]
fn next_page_parses_the_queue_panel() {
    let response: NextResponse = serde_json::from_value(fixture("next")).expect("decode next.json");
    let panel = response
        .contents
        .as_ref()
        .and_then(|contents| {
            contents
                .single_column_music_watch_next_results_renderer
                .as_ref()
        })
        .and_then(|single| single.tabbed_renderer.as_ref())
        .and_then(|tabbed| tabbed.watch_next_tabbed_results_renderer.as_ref())
        .and_then(|watch| watch.tabs.first())
        .and_then(|tab| tab.tab_renderer.as_ref())
        .and_then(|tab| tab.content.as_ref())
        .and_then(|content| content.music_queue_renderer.as_ref())
        .and_then(|queue| queue.content.as_ref())
        .map(|content| &content.playlist_panel_renderer)
        .expect("playlistPanelRenderer");
    let songs: Vec<_> = panel
        .contents
        .iter()
        .filter_map(|content| {
            content
                .playlist_panel_video_renderer
                .as_ref()
                .and_then(NextPage::from_playlist_panel_video_renderer)
        })
        .collect();
    assert_eq!(songs.len(), 2);
    assert_eq!(songs[0].id, "SONG5");
    assert_eq!(songs[0].duration, Some(200));
    assert_eq!(songs[0].artists[0].name, "Owner O");
    assert_eq!(
        songs[0].album.as_ref().map(|album| album.id.as_str()),
        Some("MPREb_albumG")
    );
    assert_eq!(
        songs[0].library_remove_token.as_deref(),
        Some("REMOVE_SONG5")
    );
    assert!(songs[0].explicit);
    // the panel continuation and the lyrics/related tabs
    assert_eq!(
        panel
            .continuations
            .as_ref()
            .and_then(|continuations| get_continuation(continuations)),
        Some("NEXT_QUEUE_CONT")
    );
    let tabs = &response
        .contents
        .as_ref()
        .and_then(|contents| {
            contents
                .single_column_music_watch_next_results_renderer
                .as_ref()
        })
        .and_then(|single| single.tabbed_renderer.as_ref())
        .and_then(|tabbed| tabbed.watch_next_tabbed_results_renderer.as_ref())
        .unwrap()
        .tabs;
    assert_eq!(
        tabs.get(1)
            .and_then(|tab| tab.tab_renderer.as_ref())
            .and_then(|tab| tab.endpoint.as_ref())
            .and_then(|endpoint| endpoint.browse_endpoint.as_ref())
            .map(|browse| browse.browse_id.as_str()),
        Some("MPLYt_SONG5_LYRICS")
    );
}

// ---------------------------------------------------------------------------
// HistoryPage — history.json
// ---------------------------------------------------------------------------

/// HistoryPage.kt:24-81 — shelves parse, the untitled shelf drops, the
/// history-remove token and the play-button id fallback.
#[test]
fn history_page_parses_sections_and_tokens() {
    let response: BrowseResponse =
        serde_json::from_value(fixture("history")).expect("decode history.json");
    let contents = single_column_contents(&response);
    let sections: Vec<_> = contents
        .iter()
        .filter_map(|content| content.music_shelf_renderer.as_ref())
        .filter_map(HistoryPageParser::from_music_shelf_renderer)
        .collect();
    // the untitled shelf drops (HistoryPage.kt:24-27)
    assert_eq!(sections.len(), 2);
    assert_eq!(sections[0].title, "Today");
    assert_eq!(sections[0].songs.len(), 1);
    let song = &sections[0].songs[0];
    assert_eq!(song.id, "SONG5");
    assert_eq!(song.duration, Some(200));
    assert_eq!(
        song.history_remove_token.as_deref(),
        Some("HISTORY_REMOVE_SONG5")
    );
    assert_eq!(
        song.album.as_ref().map(|album| album.id.as_str()),
        Some("MPREb_albumG")
    );
    // Yesterday's video: no playlistItemData — the id rides the play
    // button (HistoryPage.kt:33-37) and the cover falls back to ""
    assert_eq!(sections[1].title, "Yesterday");
    let video = &sections[1].songs[0];
    assert_eq!(video.id, "VID7");
    assert_eq!(video.thumbnail, "");
}

// ---------------------------------------------------------------------------
// LibraryPage — library.json
// ---------------------------------------------------------------------------

/// LibraryPage.kt:21-171 — the grid converters (album, editable playlist)
/// and the shelf continuation song with the empty-string id fallbacks.
#[test]
fn library_page_parses_grid_and_songs() {
    let response: BrowseResponse =
        serde_json::from_value(fixture("library")).expect("decode library.json");
    let contents = single_column_contents(&response);
    let grid = contents[0].grid_renderer.as_ref().unwrap();
    let items: Vec<_> = grid
        .items
        .iter()
        .filter_map(|item| item.music_two_row_item_renderer.as_ref())
        .filter_map(LibraryPageParser::from_music_two_row_item_renderer)
        .collect();
    assert_eq!(items.len(), 2);
    let YTItem::Album(album) = &items[0] else {
        panic!("expected an album");
    };
    assert_eq!(album.browse_id, "MPREb_albumA");
    assert_eq!(album.year, Some(2021));
    let YTItem::Playlist(playlist) = &items[1] else {
        panic!("expected a playlist");
    };
    // the VL prefix strips and the EDIT menu entry marks editability
    assert_eq!(playlist.id, "RD_MIX1");
    assert!(playlist.is_editable);
    assert_eq!(playlist.song_count_text.as_deref(), Some("25 songs"));

    let shelf = response
        .continuation_contents
        .as_ref()
        .and_then(|contents| contents.music_shelf_continuation.as_ref())
        .unwrap();
    let songs: Vec<_> = mochy_innertube::models::music_renderers::shelf_items(
        shelf.contents.as_deref().unwrap_or_default(),
    )
    .iter()
    .filter_map(|renderer| LibraryPageParser::from_music_responsive_list_item_renderer(renderer))
    .collect();
    assert_eq!(songs.len(), 1);
    let YTItem::Song(song) = &songs[0] else {
        panic!("expected a song");
    };
    assert_eq!(song.id, "SONG1");
    // the artist's browse id rides the run's browse endpoint; a run
    // WITHOUT one would keep the empty-string id (LibraryPage.kt:104-108)
    assert_eq!(song.artists[0].id, Some("UCA1".to_owned()));
    assert_eq!(song.library_remove_token.as_deref(), Some("REMOVE_SONG1"));
}

// ---------------------------------------------------------------------------
// RelatedPage — related.json
// ---------------------------------------------------------------------------

/// RelatedPage.kt:22-174 — carousel/shelf/item-section converters; the
/// ATV filter itself is exercised in the facade wire test.
#[test]
fn related_page_parses_all_three_surfaces() {
    let response: BrowseResponse =
        serde_json::from_value(fixture("related")).expect("decode related.json");
    let contents = response
        .contents
        .as_ref()
        .and_then(|contents| contents.section_list_renderer.as_ref())
        .and_then(|section| section.contents.as_ref())
        .cloned()
        .unwrap_or_default();
    let mut songs = Vec::new();
    let mut albums = Vec::new();
    let mut artists = Vec::new();
    let mut playlists = Vec::new();
    for content in &contents {
        if let Some(carousel) = content.music_carousel_shelf_renderer.as_ref() {
            for item in &carousel.contents {
                if let Some(song) = item
                    .music_responsive_list_item_renderer
                    .as_ref()
                    .and_then(RelatedPageParser::from_music_responsive_list_item_renderer)
                {
                    songs.push(song);
                }
                if let Some(item) = item
                    .music_two_row_item_renderer
                    .as_ref()
                    .and_then(RelatedPageParser::from_music_two_row_item_renderer)
                {
                    match item {
                        YTItem::Album(album) => albums.push(album),
                        YTItem::Artist(artist) => artists.push(artist),
                        YTItem::Playlist(playlist) => playlists.push(playlist),
                        YTItem::Song(song) => songs.push(song),
                    }
                }
            }
        }
        if let Some(shelf) = content.music_shelf_renderer.as_ref() {
            songs.extend(
                mochy_innertube::models::music_renderers::shelf_items(
                    shelf.contents.as_deref().unwrap_or_default(),
                )
                .iter()
                .filter_map(|renderer| {
                    RelatedPageParser::from_music_responsive_list_item_renderer(renderer)
                }),
            );
        }
        if let Some(section) = content.item_section_renderer.as_ref() {
            songs.extend(
                section
                    .contents
                    .as_deref()
                    .unwrap_or_default()
                    .iter()
                    .filter_map(|content| content.music_responsive_list_item_renderer.as_ref())
                    .filter_map(|renderer| {
                        RelatedPageParser::from_music_responsive_list_item_renderer(renderer)
                    }),
            );
        }
    }
    // carousel + shelf + item-section; the UGC video counts too here —
    // the ATV filter is the facade's `related()` job, not the parser's
    assert_eq!(songs.len(), 4);
    assert_eq!(albums.len(), 1);
    assert_eq!(artists.len(), 1);
    assert_eq!(playlists.len(), 1);
    // the playlist's digit-bearing song-count run wins over view counts
    assert_eq!(playlists[0].song_count_text.as_deref(), Some("12 songs"));
}

// ---------------------------------------------------------------------------
// MoodAndGenres + ExplorePage + NewReleaseAlbumPage + ChartsPage
// ---------------------------------------------------------------------------

/// MoodAndGenres.kt:22-48.
#[test]
fn mood_and_genres_parses_buttons() {
    let response: BrowseResponse =
        serde_json::from_value(fixture("mood_and_genres")).expect("decode");
    let parsed = MoodAndGenresParser::from_section_list_renderer_content(
        &single_column_contents(&response)[0],
    )
    .expect("moods parse");
    assert_eq!(parsed.title, "Moods & genres");
    assert_eq!(parsed.items.len(), 2);
    assert_eq!(parsed.items[0].title, "Chill");
    assert_eq!(parsed.items[0].stripe_color, 4294967295);
    assert_eq!(
        parsed.items[0].endpoint.params.as_deref(),
        Some("chill_params")
    );
}

/// The explore walk's two carousels (ExplorePage.kt through the facade
/// walk): new releases by the more-button browse id, moods by the nav
/// buttons.
#[test]
fn explore_parses_new_releases_and_moods() {
    let response: BrowseResponse =
        serde_json::from_value(fixture("explore")).expect("decode explore.json");
    let explore_contents = single_column_contents(&response);
    let carousels: Vec<_> = explore_contents
        .iter()
        .filter_map(|content| content.music_carousel_shelf_renderer.as_ref())
        .collect();
    let by_more_button = |browse_id: &str| {
        carousels.iter().find(|carousel| {
            carousel
                .header
                .as_ref()
                .map(|header| {
                    &header
                        .music_carousel_shelf_basic_header_renderer
                        .more_content_button
                })
                .and_then(|button| button.as_ref().map(|button| &button.button_renderer))
                .and_then(|button| button.navigation_endpoint.as_ref())
                .and_then(|endpoint| endpoint.browse_endpoint.as_ref())
                .map(|browse| browse.browse_id == browse_id)
                .unwrap_or(false)
        })
    };
    let albums: Vec<_> = by_more_button("FEmusic_new_releases_albums")
        .unwrap()
        .contents
        .iter()
        .filter_map(|content| content.music_two_row_item_renderer.as_ref())
        .filter_map(NewReleaseAlbumPage::from_music_two_row_item_renderer)
        .collect();
    assert_eq!(albums.len(), 1);
    assert_eq!(albums[0].browse_id, "MPREb_albumN");
    assert_eq!(albums[0].year, Some(2026));

    let moods: Vec<_> = by_more_button("FEmusic_moods_and_genres")
        .unwrap()
        .contents
        .iter()
        .filter_map(|content| content.music_navigation_button_renderer.as_ref())
        .filter_map(MoodAndGenresParser::from_music_navigation_button_renderer)
        .collect();
    assert_eq!(moods.len(), 1);
    assert_eq!(moods[0].title, "Chill");
}

/// NewReleaseAlbumPage.kt:10-43 over the new-release grid.
#[test]
fn new_release_albums_parses_grid() {
    let response: BrowseResponse =
        serde_json::from_value(fixture("new_release_albums")).expect("decode");
    let contents = single_column_contents(&response);
    let grid = contents[0].grid_renderer.as_ref().unwrap();
    let albums: Vec<_> = grid
        .items
        .iter()
        .filter_map(|item| item.music_two_row_item_renderer.as_ref())
        .filter_map(NewReleaseAlbumPage::from_music_two_row_item_renderer)
        .collect();
    assert_eq!(albums.len(), 1);
    assert_eq!(albums[0].playlist_id, "OLAK5uy_albumN");
    assert!(albums[0].explicit);
    assert_eq!(
        albums[0].artists.as_ref().unwrap()[0].id.as_deref(),
        Some("UCN14")
    );
}

/// ChartsPage.kt + the facade chart converters (determineChartType and
/// the position/change columns) — exercised through the fixture walk.
#[test]
fn charts_page_sections_and_types() {
    let response: BrowseResponse =
        serde_json::from_value(fixture("charts")).expect("decode charts.json");
    let contents = single_column_contents(&response);
    let mut section_titles = Vec::new();
    for content in &contents {
        if let Some(carousel) = content.music_carousel_shelf_renderer.as_ref() {
            let title = carousel
                .header
                .as_ref()
                .map(|header| &header.music_carousel_shelf_basic_header_renderer.title)
                .and_then(|title| title.first())
                .map(|run| run.text.clone())
                .unwrap_or_default();
            section_titles.push(title);
        }
    }
    assert_eq!(section_titles, vec!["Trending", "Top 50"]);
    // the trending chart item carries position and change columns
    let trending = contents[0]
        .music_carousel_shelf_renderer
        .as_ref()
        .unwrap()
        .contents[0]
        .music_responsive_list_item_renderer
        .as_ref()
        .unwrap();
    let columns = &trending.flex_columns;
    assert_eq!(
        columns[2]
            .music_responsive_list_item_flex_column_renderer
            .text
            .as_ref()
            .unwrap()
            .runs
            .as_ref()
            .unwrap()[0]
            .text,
        "1"
    );
    assert_eq!(parse_time("3:45"), Some(225)); // chart items reuse durations
}

// ---------------------------------------------------------------------------
// Queue + transcript + captions + player + account — the response-only
// parsers
// ---------------------------------------------------------------------------

/// GetQueueResponse + NextPage (the facade's `queue()`).
#[test]
fn queue_parses_panel_songs() {
    let response: GetQueueResponse =
        serde_json::from_value(fixture("queue")).expect("decode queue.json");
    let songs: Vec<_> = response
        .queue_datas
        .iter()
        .filter_map(|data| {
            data.content
                .as_ref()
                .and_then(|content| content.playlist_panel_video_renderer.as_ref())
        })
        .filter_map(NextPage::from_playlist_panel_video_renderer)
        .collect();
    assert_eq!(songs.len(), 2);
    assert_eq!(songs[0].id, "SONG5");
    assert_eq!(songs[1].id, "SONG6");
    assert_eq!(songs[1].duration, Some(150));
}

/// GetTranscriptResponse — the cue structure the facade formats.
#[test]
fn transcript_parses_cue_groups() {
    let response: GetTranscriptResponse =
        serde_json::from_value(fixture("transcript")).expect("decode transcript.json");
    let groups = response.actions.as_ref().unwrap()[0]
        .update_engagement_panel_action
        .as_ref()
        .unwrap()
        .content
        .as_ref()
        .unwrap()
        .transcript_renderer
        .as_ref()
        .unwrap()
        .body
        .as_ref()
        .unwrap()
        .transcript_body_renderer
        .as_ref()
        .unwrap()
        .cue_groups
        .clone();
    assert_eq!(groups.len(), 2);
    let cue = groups[0]
        .transcript_cue_group_renderer
        .as_ref()
        .unwrap()
        .cues[0]
        .transcript_cue_renderer
        .as_ref()
        .unwrap();
    assert_eq!(cue.start_offset_ms, 2000);
    assert_eq!(cue.duration_ms, 3000);
    assert_eq!(
        cue.cue.as_ref().unwrap().simple_text,
        " [Music] first line "
    );
}

/// Captions.kt — pick + json3 conversion over the timedtext fixture.
#[test]
fn captions_convert_json3() {
    let timedtext = fixture("timedtext").to_string();
    let lrc = captions::timed_text_to_lrc(&timedtext);
    assert_eq!(lrc, "[00:03.000]hello world\n[01:01.000]again");
    let cues = captions::timed_text_to_cues(&timedtext);
    assert_eq!(cues.len(), 3);
    assert_eq!(
        (cues[0].start_ms, cues[0].end_ms, cues[0].text.as_str()),
        (3000, 4500, "hello")
    );
    assert_eq!(
        (cues[1].start_ms, cues[1].end_ms, cues[1].text.as_str()),
        (4500, 6000, "world")
    );
    assert_eq!(cues[2].text, "again");

    // pick_caption_track: the uploaded track in the auto track's language
    let player: PlayerResponse =
        serde_json::from_value(fixture("player_web_remix")).expect("decode player");
    let tracks = captions::player_caption_tracks(&player);
    assert_eq!(tracks.len(), 2);
    let picked = captions::pick_caption_track(&tracks).unwrap();
    assert!(!picked.is_auto_generated());
    assert_eq!(picked.language_code.as_deref(), Some("en"));
}

/// PlayerResponse — the VISIONOS shape (unciphered full-file urls, the
/// audio/original flags, init/index ranges).
#[test]
fn player_visionos_parses_formats() {
    let player: PlayerResponse =
        serde_json::from_value(fixture("player_visionos")).expect("decode player");
    assert_eq!(player.playability_status.as_ref().unwrap().status, "OK");
    let streaming = player.streaming_data.as_ref().unwrap();
    assert_eq!(streaming.expires_in_seconds, 21540);
    let adaptive = &streaming.adaptive_formats[0];
    assert_eq!(adaptive.itag, 251);
    assert!(adaptive.url.is_some());
    assert!(adaptive.is_audio());
    assert!(adaptive.is_original());
    assert_eq!(adaptive.audio_sample_rate, Some(48000));
    assert_eq!(
        adaptive.init_range.as_ref().map(|r| r.start.as_str()),
        Some("0")
    );
    // the muxed format is NOT audio (it has a width)
    let muxed = &streaming.formats.as_ref().unwrap()[0];
    assert_eq!(muxed.itag, 140);
    assert!(muxed.is_audio());
    let video = &streaming.adaptive_formats[1];
    assert!(!video.is_audio());
    // the captions and the playback tracking ride along
    assert_eq!(
        player.video_details.as_ref().unwrap().music_video_type,
        Some("MUSIC_VIDEO_TYPE_ATV".to_owned())
    );
    assert!(
        player
            .playback_tracking
            .as_ref()
            .unwrap()
            .videostats_playback_url
            .as_ref()
            .unwrap()
            .base_url
            .as_ref()
            .unwrap()
            .starts_with("https://s.youtube.com/api/stats/playback")
    );
}

/// PlayerResponse — the WEB_REMIX shape (ciphered urls only).
#[test]
fn player_web_remix_parses_ciphers() {
    let player: PlayerResponse =
        serde_json::from_value(fixture("player_web_remix")).expect("decode player");
    let streaming = player.streaming_data.as_ref().unwrap();
    for format in &streaming.adaptive_formats {
        assert!(format.url.is_none());
        assert!(format.signature_cipher.is_some() || format.cipher.is_some());
        assert!(format.is_audio());
    }
    // the s48 avatar of the author thumbnail maps to s960 in getMediaInfo
    // (InnerTube.kt:816-819) — checked against the watch-next fixture
    let watch: NextResponse =
        serde_json::from_value(fixture("media_info_watch_next")).expect("decode watch next");
    let owner = watch
        .contents
        .as_ref()
        .unwrap()
        .two_column_watch_next_results
        .as_ref()
        .unwrap()
        .results
        .as_ref()
        .unwrap()
        .results
        .as_ref()
        .unwrap()
        .content
        .as_ref()
        .unwrap();
    let secondary = owner
        .iter()
        .flatten()
        .find_map(|content| content.video_secondary_info_renderer.as_ref())
        .unwrap();
    let thumb = secondary
        .owner
        .as_ref()
        .unwrap()
        .video_owner_renderer
        .as_ref()
        .unwrap()
        .thumbnail
        .as_ref()
        .unwrap()
        .thumbnails
        .as_ref()
        .unwrap()[0]
        .url
        .replace("s48", "s960");
    assert!(thumb.contains("s960"));
    // the RYD half of the composite
    let ryd: ReturnYouTubeDislikeResponse =
        serde_json::from_value(fixture("ryd_votes")).expect("decode ryd");
    assert_eq!(ryd.likes, Some(100));
    assert_eq!(ryd.dislikes, Some(5));
    assert_eq!(ryd.view_count, Some(123456));
}

/// AccountMenuResponse.kt:33-47 — `toAccountInfo` and the `//` → `https:`
/// avatar prefix.
#[test]
fn account_menu_parses_account_info() {
    let response: AccountMenuResponse =
        serde_json::from_value(fixture("account_menu")).expect("decode account_menu.json");
    let renderer = response.actions[0]
        .open_popup_action
        .as_ref()
        .unwrap()
        .popup
        .as_ref()
        .unwrap()
        .multi_page_menu_renderer
        .as_ref()
        .unwrap()
        .header
        .as_ref()
        .unwrap()
        .active_account_header_renderer
        .as_ref()
        .unwrap();
    let info = renderer.to_account_info();
    assert_eq!(info.name, "Mochy Listener");
    assert_eq!(info.email.as_deref(), Some("listener@example.com"));
    assert_eq!(info.channel_handle.as_deref(), Some("@mochylistener"));
    assert_eq!(
        info.thumbnail_url.as_deref(),
        Some("https://yt3.ggpht.com/s176/photo.jpg")
    );
}

// ---------------------------------------------------------------------------
// BrowseResult — browse_genre.json
// ---------------------------------------------------------------------------

/// BrowseResult.kt:8-67 — the genre browse grid/carousel items and the
/// section filters.
#[test]
fn browse_result_parses_genre_page_and_filters() {
    let response: BrowseResponse =
        serde_json::from_value(fixture("browse_genre")).expect("decode browse_genre.json");
    let contents = single_column_contents(&response);
    let mut result = BrowseResult {
        title: response
            .header
            .as_ref()
            .and_then(|header| header.music_header_renderer.as_ref())
            .and_then(|plain| plain.title.as_ref())
            .and_then(|title| title.first())
            .map(|run| run.text.clone()),
        items: Vec::new(),
    };
    assert_eq!(result.title.as_deref(), Some("Chill"));
    for content in &contents {
        if let Some(grid) = content.grid_renderer.as_ref() {
            result
                .items
                .push(mochy_innertube::pages::browse_result::BrowseItem {
                    title: grid.header.as_ref().and_then(|header| {
                        header
                            .grid_header_renderer
                            .title
                            .first()
                            .map(|run| run.text.clone())
                    }),
                    items: grid
                        .items
                        .iter()
                        .filter_map(|item| item.music_two_row_item_renderer.as_ref())
                        .filter_map(|renderer| {
                            RelatedPageParser::from_music_two_row_item_renderer(renderer)
                        })
                        .collect(),
                });
        }
    }
    assert_eq!(result.items.len(), 1);
    assert_eq!(result.items[0].items.len(), 1);
    assert_eq!(result.items[0].items[0].id(), "PL_chill");

    // the filter family drops empty sections (BrowseResult.kt:17-32)
    result.items[0].items.clear();
    let filtered = result.clone().filter_explicit(true);
    assert!(filtered.items.is_empty());
}

// ---------------------------------------------------------------------------
// Serde round-trips over the corpus
// ---------------------------------------------------------------------------

fn round_trip<T: serde::de::DeserializeOwned + serde::Serialize>(name: &str) -> serde_json::Value {
    let original = fixture(name);
    let decoded: T = serde_json::from_value(original.clone()).expect("decode");
    let encoded = serde_json::to_value(&decoded).expect("encode");
    let redecoded: T = serde_json::from_value(encoded.clone()).expect("redecode");
    let reencoded = serde_json::to_value(&redecoded).expect("reencode");
    assert_eq!(encoded, reencoded, "{name}: re-encode drift");
    encoded
}

/// Round-trip: SearchResponse over search.json.
#[test]
fn round_trip_search_response() {
    let encoded = round_trip::<SearchResponse>("search");
    // a spot check that the wire shape survived
    assert!(encoded["continuationContents"]["musicShelfContinuation"]
        ["continuations"][0]["nextContinuationData"]["continuation"]
        .is_string());
}

/// Round-trip: BrowseResponse over album.json (the richest envelope).
#[test]
fn round_trip_browse_response() {
    let encoded = round_trip::<BrowseResponse>("album");
    assert_eq!(
        encoded["microformat"]["microformatDataRenderer"]["urlCanonical"],
        "https://music.youtube.com/playlist?list=OLAK5uy_albumA"
    );
}

/// Round-trip: PlayerResponse over player_visionos.json.
#[test]
fn round_trip_player_response() {
    let encoded = round_trip::<PlayerResponse>("player_visionos");
    assert_eq!(encoded["streamingData"]["adaptiveFormats"][0]["itag"], 251);
}

/// Round-trip: NextResponse over next.json.
#[test]
fn round_trip_next_response() {
    let encoded = round_trip::<NextResponse>("next");
    assert!(encoded["contents"]["singleColumnMusicWatchNextResultsRenderer"]
        ["tabbedRenderer"]["watchNextTabbedResultsRenderer"]["tabs"]
        .is_array());
}

/// The corpus is present and redacted: no fixture carries a live
/// visitorData (the redaction contract of tests/fixtures/README.md).
#[test]
fn corpus_is_redacted() {
    for name in [
        "search",
        "home",
        "album",
        "artist",
        "playlist",
        "player_visionos",
        "player_web_remix",
    ] {
        let text = common::load_fixture_text(name).unwrap();
        assert!(
            !text.contains("Cgt") || text.contains("<redacted:visitordata>"),
            "{name} carries an unredacted visitor data"
        );
    }
}
