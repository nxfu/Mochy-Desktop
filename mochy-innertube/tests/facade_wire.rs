//! Facade wire tests — the `YouTube` facade against the local wire
//! server over the committed fixtures: the Phase 05 offline proof that
//! the endpoint layer, the DTO decode and the parsers compose end to
//! end (search with a filter, album, queue, account, the getMediaInfo
//! composite, charts, transcript).

mod common;

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use common::serve::{json_response, serve_responses};
use mochy_innertube::models::ytitem::YTItem;
use mochy_innertube::session::{Session, SharedSession};
use mochy_innertube::transport::{InnerTubeTransport, TransportConfig};
use mochy_innertube::youtube::{SearchFilter, YouTube};

fn fixture_text(name: &str) -> String {
    common::load_fixture_text(name).unwrap_or_else(|| panic!("fixture {name} missing"))
}

fn facade(session: SharedSession, addr: std::net::SocketAddr) -> YouTube {
    let local = format!("http://{addr}");
    let mut overrides = HashMap::new();
    overrides.insert("https://music.youtube.com".to_owned(), local.clone());
    overrides.insert("https://www.youtube.com".to_owned(), local.clone());
    overrides.insert("https://returnyoutubedislikeapi.com".to_owned(), local);
    let mut config = TransportConfig {
        cache_enabled: false,
        ..TransportConfig::default()
    };
    config.origin_overrides = overrides;
    config.retry = mochy_net::RetryPolicy {
        max_attempts: 1,
        ..mochy_net::RetryPolicy::upstream()
    };
    YouTube::with_transport(InnerTubeTransport::with_config(session, config).unwrap())
}

fn guest_session() -> SharedSession {
    Arc::new(RwLock::new(Session::default()))
}

/// `search(query, filter)` (YouTube.kt:239-251) over search.json — the
/// filter rides the request body, the results parse through SearchPage.
#[tokio::test]
async fn facade_search_over_the_wire() {
    let (addr, captured, handle) =
        serve_responses(vec![json_response(&fixture_text("search"))]).await;
    let youtube = facade(guest_session(), addr);

    let result = youtube
        .search("artist a", &SearchFilter::song())
        .await
        .expect("search");
    assert_eq!(result.items.len(), 2);
    assert_eq!(result.items[0].id(), "SONG1");
    assert_eq!(result.continuation.as_deref(), Some("SEARCH_CONT_NEXT"));

    let requests = captured.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].path().ends_with("/youtubei/v1/search"));
    // the filter value rides the body params (SearchBody.params)
    let body = requests[0].body();
    assert!(body.contains("EgWKAQIIAWoKEAkQBRAKEAMQBA%3D%3D"));
    handle.abort();
}

/// `album(browseId, withSongs = false)` (YouTube.kt:265-375) over
/// album.json — the normal two-column branch with header walks and the
/// other-versions/releases carousels.
#[tokio::test]
async fn facade_album_over_the_wire() {
    let (addr, captured, handle) =
        serve_responses(vec![json_response(&fixture_text("album"))]).await;
    let youtube = facade(guest_session(), addr);

    let album = youtube
        .album("MPREb_albumA", false)
        .await
        .expect("album parses");
    assert_eq!(album.album.title, "Album A");
    assert_eq!(album.album.playlist_id, "OLAK5uy_albumA");
    assert_eq!(album.album.year, Some(2021));
    assert_eq!(album.album.artists.as_ref().unwrap()[0].name, "Artist A");
    assert_eq!(
        album.description.as_deref(),
        Some("The description of Album A.")
    );
    assert_eq!(album.other_versions.len(), 1);
    assert_eq!(album.other_versions[0].browse_id, "MPREb_albumD");
    assert_eq!(album.releases_for_you.len(), 1);
    assert_eq!(album.releases_for_you[0].browse_id, "MPREb_albumE");

    let requests = captured.lock().unwrap();
    // one browse call, browseId in the body
    assert!(requests[0].body().contains("MPREb_albumA"));
    handle.abort();
}

/// `queue(videoIds, playlistId)` (YouTube.kt:1412-1422) over queue.json.
#[tokio::test]
async fn facade_queue_over_the_wire() {
    let (addr, _captured, handle) =
        serve_responses(vec![json_response(&fixture_text("queue"))]).await;
    let youtube = facade(guest_session(), addr);

    let songs = youtube
        .queue(None, Some("PL_playlist1"))
        .await
        .expect("queue parses");
    assert_eq!(songs.len(), 2);
    assert_eq!(songs[0].id, "SONG5");
    assert_eq!(songs[1].duration, Some(150));
    handle.abort();
}

/// `accountInfo()` (YouTube.kt:1485-1490) over account_menu.json — with
/// the `//` avatar prefix normalization.
#[tokio::test]
async fn facade_account_info_over_the_wire() {
    let (addr, _captured, handle) =
        serve_responses(vec![json_response(&fixture_text("account_menu"))]).await;
    let youtube = facade(guest_session(), addr);

    let info = youtube.account_info().await.expect("account parses");
    assert_eq!(info.name, "Mochy Listener");
    assert_eq!(
        info.thumbnail_url.as_deref(),
        Some("https://yt3.ggpht.com/s176/photo.jpg")
    );
    handle.abort();
}

/// `getMediaInfo(videoId)` (YouTube.kt:1528 + InnerTube.kt:764-834) — the
/// WEB watch-next page plus the ReturnYouTubeDislike composite, two
/// scripted responses.
#[tokio::test]
async fn facade_media_info_over_the_wire() {
    let (addr, captured, handle) = serve_responses(vec![
        json_response(&fixture_text("media_info_watch_next")),
        json_response(&fixture_text("ryd_votes")),
    ])
    .await;
    let youtube = facade(guest_session(), addr);

    let info = youtube.get_media_info("SONG1").await.expect("media info");
    assert_eq!(info.video_id, "SONG1");
    assert_eq!(info.title.as_deref(), Some("Song One"));
    assert_eq!(info.author.as_deref(), Some("Artist A - Topic"));
    assert_eq!(info.author_id.as_deref(), Some("UCA1"));
    // the s48 avatar maps to s960 (InnerTube.kt:816-819); upstream does
    // NOT normalize the protocol-relative prefix here (only
    // toAccountInfo does)
    assert_eq!(
        info.author_thumbnail.as_deref(),
        Some("//yt3.ggpht.com/s960/a.jpg")
    );
    assert_eq!(
        info.description.as_deref(),
        Some("Provided to YouTube by Auto-generated")
    );
    assert_eq!(info.upload_date.as_deref(), Some("Jan 1, 2021"));
    assert_eq!(info.view_count, Some(123456));
    assert_eq!(info.like, Some(100));
    assert_eq!(info.dislike, Some(5));

    let requests = captured.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert!(requests[0].path().ends_with("/youtubei/v1/next"));
    assert!(requests[1].path().starts_with("/Votes"));
    handle.abort();
}

/// `getChartsPage` (YouTube.kt:951-1015) over charts.json — the chart
/// types and the position/change columns.
#[tokio::test]
async fn facade_charts_over_the_wire() {
    let (addr, _captured, handle) =
        serve_responses(vec![json_response(&fixture_text("charts"))]).await;
    let youtube = facade(guest_session(), addr);

    let charts = youtube.get_charts_page(None).await.expect("charts parse");
    assert_eq!(charts.sections.len(), 3);
    assert_eq!(charts.sections[0].title, "Trending");
    assert_eq!(
        charts.sections[0].chart_type,
        mochy_innertube::pages::charts::ChartType::Trending
    );
    assert_eq!(
        charts.sections[1].chart_type,
        mochy_innertube::pages::charts::ChartType::Top
    );
    assert_eq!(
        charts.sections[2].chart_type,
        mochy_innertube::pages::charts::ChartType::NewReleases
    );
    let YTItem::Song(trending) = &charts.sections[0].items[0] else {
        panic!("expected a chart song");
    };
    assert_eq!(trending.id, "SONG1");
    assert_eq!(trending.chart_position, Some(1));
    assert_eq!(trending.chart_change.as_deref(), Some(" ▲ 2"));
    handle.abort();
}

/// `transcript(videoId)` (YouTube.kt:1424-1433) over transcript.json —
/// the LRC formatting with the ♪/bracket scrub.
#[tokio::test]
async fn facade_transcript_over_the_wire() {
    let (addr, _captured, handle) =
        serve_responses(vec![json_response(&fixture_text("transcript"))]).await;
    let youtube = facade(guest_session(), addr);

    let transcript = youtube.transcript("SONG1").await.expect("transcript");
    // transcript() only TRIMS the ♪/space edges (YouTube.kt:1428-1430) —
    // the bracket/♪ scrub belongs to captions()' json3 path
    assert_eq!(
        transcript,
        "[00:02.000][Music] first line\n[00:05.000]second ♪ line"
    );
    handle.abort();
}

/// `musicHistory` (YouTube.kt:1121-1180) at depth 1 — the first page's
/// shelves, with the untitled shelf dropped.
#[tokio::test]
async fn facade_history_first_page_over_the_wire() {
    let (addr, _captured, handle) =
        serve_responses(vec![json_response(&fixture_text("history"))]).await;
    let youtube = facade(guest_session(), addr);

    let history = youtube.music_history(None, 1).await.expect("history");
    let sections = history.sections.expect("sections");
    assert_eq!(sections.len(), 2);
    assert_eq!(sections[0].title, "Today");
    assert_eq!(
        sections[0].songs[0].history_remove_token.as_deref(),
        Some("HISTORY_REMOVE_SONG5")
    );
    handle.abort();
}
