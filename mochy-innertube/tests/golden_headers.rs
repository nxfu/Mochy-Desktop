//! Golden-header tests — the Phase 04 acceptance criterion:
//! "golden-header unit tests per client (assert exact injected headers
//! for WEB_REMIX signed-out, WEB_REMIX signed-in, VISIONOS guest)".
//!
//! Every assertion cites the upstream line that produces the header
//! (InnerTube.kt:158-182 `ytClient` + 149-155 `defaultRequest`).

mod common;

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use common::serve::{CapturedRequest, empty_json, json_response, serve_responses};
use mochy_innertube::client::{ClientKey, ClientRegistry, USER_AGENT_WEB};
use mochy_innertube::session::{Session, SharedSession};
use mochy_innertube::transport::{InnerTubeTransport, TransportConfig};

fn test_transport(session: SharedSession, addr: std::net::SocketAddr) -> InnerTubeTransport {
    let local = format!("http://{addr}");
    let mut overrides = HashMap::new();
    overrides.insert("https://music.youtube.com".to_owned(), local.clone());
    overrides.insert("https://www.youtube.com".to_owned(), local);
    let mut config = TransportConfig {
        cache_enabled: false,
        ..TransportConfig::default()
    };
    config.origin_overrides = overrides;
    // a tiny retry policy so a failing test fails fast
    config.retry = mochy_net::RetryPolicy {
        max_attempts: 1,
        ..mochy_net::RetryPolicy::upstream()
    };
    InnerTubeTransport::with_config(session, config).unwrap()
}

fn session_with(visitor: Option<&str>, cookie: Option<&str>) -> SharedSession {
    session_full(visitor, cookie, None)
}

fn session_full(
    visitor: Option<&str>,
    cookie: Option<&str>,
    data_sync: Option<&str>,
) -> SharedSession {
    let mut session = Session {
        visitor_data: visitor.map(str::to_owned),
        data_sync_id: data_sync.map(str::to_owned),
        ..Session::default()
    };
    session.set_cookie(cookie.map(str::to_owned));
    Arc::new(RwLock::new(session))
}

/// WEB_REMIX signed-out: the full client header set minus the auth pair —
/// InnerTube.kt:158-182 with `setLogin` false or no cookie stored.
#[tokio::test]
async fn golden_headers_web_remix_signed_out() {
    let (addr, captured, server) = serve_responses(vec![empty_json()]).await;
    let session = session_with(Some("CgtGUESTID0000000000"), None);
    let transport = test_transport(session, addr);
    let client = ClientRegistry::new().get(ClientKey::WebRemix);

    transport
        .search(&client, Some("test query"), None, None)
        .await
        .unwrap();
    server.await.unwrap();
    let requests = captured.lock().unwrap().clone();
    assert_eq!(requests.len(), 1);
    let request = &requests[0];

    // InnerTube.kt:231-246 — POST to the music origin's search endpoint
    assert!(
        request
            .request_line()
            .starts_with("POST /youtubei/v1/search")
    );
    // InnerTube.kt:149-155 — the client defaults
    assert_eq!(
        request.header("Accept").as_deref(),
        Some("application/json")
    );
    assert_eq!(
        request.header("Accept-Language").as_deref(),
        Some("en-US,en;q=0.9")
    );
    assert_eq!(request.header("Cache-Control").as_deref(), Some("no-cache"));
    // InnerTube.kt:161-163 — client identification
    assert_eq!(
        request.header("X-Goog-Api-Format-Version").as_deref(),
        Some("1")
    );
    assert_eq!(
        request.header("X-YouTube-Client-Name").as_deref(),
        Some("67")
    );
    assert_eq!(
        request.header("X-YouTube-Client-Version").as_deref(),
        Some("1.20260707.12.00")
    );
    // InnerTube.kt:164-167 — browser clients carry origin + referer
    assert_eq!(
        request.header("X-Origin").as_deref(),
        Some("https://music.youtube.com")
    );
    assert_eq!(
        request.header("Referer").as_deref(),
        Some("https://music.youtube.com/")
    );
    // InnerTube.kt:168 — the session's visitor id
    assert_eq!(
        request.header("X-Goog-Visitor-Id").as_deref(),
        Some("CgtGUESTID0000000000")
    );
    // InnerTube.kt:180 — the per-client user agent
    assert_eq!(
        request.header("User-Agent").as_deref(),
        Some(USER_AGENT_WEB)
    );
    // InnerTube.kt:159/181 — JSON content type + prettyPrint=false
    assert!(
        request
            .header("Content-Type")
            .map(|v| v.starts_with("application/json"))
            .unwrap_or(false)
    );
    assert_eq!(request.query("prettyPrint").as_deref(), Some("false"));
    // signed out: NO cookie, NO Authorization (InnerTube.kt:169-178)
    assert_eq!(request.header("cookie"), None);
    assert_eq!(request.header("Authorization"), None);

    // the body carries the context (WEB_REMIX, visitor, no onBehalfOfUser)
    let body: serde_json::Value = serde_json::from_str(&request.body()).unwrap();
    assert_eq!(body["context"]["client"]["clientName"], "WEB_REMIX");
    assert_eq!(
        body["context"]["client"]["clientVersion"],
        "1.20260707.12.00"
    );
    assert_eq!(
        body["context"]["client"]["visitorData"],
        "CgtGUESTID0000000000"
    );
    assert_eq!(body["query"], "test query");
    assert!(body["context"]["user"].get("onBehalfOfUser").is_none());
}

/// WEB_REMIX signed-in: cookie + the origin-bound SAPISIDHASH
/// Authorization (InnerTube.kt:169-178).
#[tokio::test]
async fn golden_headers_web_remix_signed_in() {
    let (addr, captured, server) = serve_responses(vec![empty_json()]).await;
    // a cookie WITH a SAPISID so the hash is produced, and a dataSyncId so
    // onBehalfOfUser rides the context (both are set by the app's
    // observeSettingsChanges collectors for a signed-in session)
    let cookie = "SAPISID=xYz1234567890abcdef; __Secure-YEC=Ok; LOGIN_INFO=1";
    let session = session_full(
        Some("CgtGUESTID0000000000"),
        Some(cookie),
        Some("sync-id-123"),
    );
    let transport = test_transport(session, addr);
    let client = ClientRegistry::new().get(ClientKey::WebRemix);

    // browse with set_login: the signed-in surface
    transport
        .browse(&client, Some("FEmusic_home"), None, None, true, None)
        .await
        .unwrap();
    server.await.unwrap();
    let requests = captured.lock().unwrap().clone();
    assert_eq!(requests.len(), 1);
    let request = &requests[0];

    assert!(
        request
            .request_line()
            .starts_with("POST /youtubei/v1/browse")
    );
    // the whole signed-out set still holds...
    assert_eq!(
        request.header("X-YouTube-Client-Name").as_deref(),
        Some("67")
    );
    assert_eq!(
        request.header("X-Origin").as_deref(),
        Some("https://music.youtube.com")
    );
    // ...plus the cookie verbatim (InnerTube.kt:171)
    assert_eq!(request.header("cookie").as_deref(), Some(cookie));
    // ...plus Authorization: SAPISIDHASH time_hash (InnerTube.kt:172-176)
    let authorization = request
        .header("Authorization")
        .expect("Authorization header");
    let (scheme, value) = authorization.split_once(' ').unwrap();
    assert_eq!(scheme, "SAPISIDHASH");
    let (time, hash) = value.split_once('_').unwrap();
    let time: u64 = time.parse().unwrap();
    // recompute the hash from the SAME triple — the origin is the
    // client's apiOrigin, not the overridden test origin
    assert_eq!(
        hash,
        mochy_innertube::sha1_hex(&format!(
            "{time} xYz1234567890abcdef https://music.youtube.com"
        )),
        "SAPISIDHASH must be SHA-1 of \"time SAPISID origin\""
    );
    // the context carries onBehalfOfUser for a login-supported client
    let body: serde_json::Value = serde_json::from_str(&request.body()).unwrap();
    assert!(body["context"]["user"]["onBehalfOfUser"].is_string());
}

/// A cookie WITHOUT SAPISID: the cookie rides, the Authorization does not
/// (InnerTube.kt:172 `if ("SAPISID" !in cookieMap) return@let`).
#[tokio::test]
async fn cookie_without_sapisid_sends_no_authorization() {
    let (addr, captured, server) = serve_responses(vec![empty_json()]).await;
    let cookie = "VISITOR_INFO1_LIVE=abc; PREF=hl=en";
    let session = session_with(None, Some(cookie));
    let transport = test_transport(session, addr);
    let client = ClientRegistry::new().get(ClientKey::WebRemix);
    transport
        .next(&client, Some("dQw4w9WgXcQ"), None, None, None, None, None)
        .await
        .unwrap();
    server.await.unwrap();
    let request = &captured.lock().unwrap()[0];
    assert_eq!(request.header("cookie").as_deref(), Some(cookie));
    assert_eq!(request.header("Authorization"), None);
}

/// VISIONOS guest: the native-app client — no X-Origin, no Referer, api
/// format 2, its own minted visitor id, and the player request goes to
/// the www origin (InnerTube.kt:164-167 + 256-257).
#[tokio::test]
async fn golden_headers_visionos_guest() {
    // two responses: the visitor_id mint, then the player
    let (addr, captured, server) = serve_responses(vec![
        json_response(r#"{"responseContext":{"visitorData":"CgtMINTEDVISIONOS"}}"#),
        empty_json(),
    ])
    .await;
    let session = session_with(None, None);
    let transport = test_transport(session, addr);
    let client = ClientRegistry::new().get(ClientKey::VisionOs);

    transport
        .player(&client, "dQw4w9WgXcQ", None, None, None)
        .await
        .unwrap();
    server.await.unwrap();
    let requests = captured.lock().unwrap().clone();
    assert_eq!(requests.len(), 2, "mint + player");

    // request 1: the visitor_id mint at the www origin (InnerTube.kt:193)
    let mint = &requests[0];
    assert!(
        mint.request_line()
            .starts_with("POST /youtubei/v1/visitor_id")
    );
    assert_eq!(mint.header("X-YouTube-Client-Name").as_deref(), Some("101"));
    assert_eq!(
        mint.header("X-Goog-Visitor-Id"),
        None,
        "mint sends no visitor"
    );
    let mint_body: serde_json::Value = serde_json::from_str(&mint.body()).unwrap();
    assert!(mint_body["context"]["client"].get("visitorData").is_none());

    // request 2: the player with the freshly minted visitor
    let player = &requests[1];
    assert!(
        player
            .request_line()
            .starts_with("POST /youtubei/v1/player")
    );
    // InnerTube.kt:161 — VISIONOS apiFormatVersion "2"
    assert_eq!(
        player.header("X-Goog-Api-Format-Version").as_deref(),
        Some("2")
    );
    assert_eq!(
        player.header("X-YouTube-Client-Name").as_deref(),
        Some("101")
    );
    assert_eq!(
        player.header("X-YouTube-Client-Version").as_deref(),
        Some("1.04")
    );
    // InnerTube.kt:164-167 + YouTubeClient.kt:33-34 — the native app
    // sends no origin or referer
    assert_eq!(player.header("X-Origin"), None);
    assert_eq!(player.header("Referer"), None);
    // the minted visitor id rides both the header and the context
    assert_eq!(
        player.header("X-Goog-Visitor-Id").as_deref(),
        Some("CgtMINTEDVISIONOS")
    );
    let player_body: serde_json::Value = serde_json::from_str(&player.body()).unwrap();
    assert_eq!(
        player_body["context"]["client"]["visitorData"],
        "CgtMINTEDVISIONOS"
    );
    // the context describes the app the way newpipe does
    assert_eq!(player_body["context"]["client"]["clientScreen"], "WATCH");
    assert_eq!(player_body["context"]["client"]["platform"], "MOBILE");
    assert_eq!(player_body["context"]["client"]["osName"], "visionOS");
    assert_eq!(
        player_body["context"]["client"]["deviceModel"],
        "RealityDevice17,1"
    );
    assert_eq!(player_body["context"]["client"]["utcOffsetMinutes"], 0);
    // the visionos user agent
    assert_eq!(
        player.header("User-Agent").as_deref(),
        Some(
            "com.google.visionos.youtube/1.04(RealityDevice17,1; U; CPU visionOS 26_6_0 like Mac OS X; US)"
        )
    );
    // signed-out guest: no cookie/auth
    assert_eq!(player.header("cookie"), None);
    assert_eq!(player.header("Authorization"), None);
    // no signatureTimestamp and no poToken -> no playbackContext, no
    // serviceIntegrityDimensions (InnerTube.kt:272-281)
    assert!(player_body.get("playbackContext").is_none());
    assert!(player_body.get("serviceIntegrityDimensions").is_none());
    // but the content checks are on (PlayerBody.kt:8-9)
    assert_eq!(player_body["contentCheckOk"], true);
    assert_eq!(player_body["racyCheckOk"], true);
}

/// The embedded-player clients add thirdParty.embedUrl
/// (InnerTube.kt:262-268).
#[tokio::test]
async fn embedded_player_sends_third_party_embed_url() {
    let (addr, captured, server) = serve_responses(vec![empty_json()]).await;
    let session = session_with(Some("CgtGUESTID0000000000"), None);
    let transport = test_transport(session, addr);
    let client = ClientRegistry::new().get(ClientKey::WebEmbeddedPlayer);
    transport
        .player(&client, "abc123XYZ-_ ", None, Some(20_000), None)
        .await
        .unwrap();
    server.await.unwrap();
    let request = &captured.lock().unwrap()[0];
    let body: serde_json::Value = serde_json::from_str(&request.body()).unwrap();
    assert_eq!(
        body["context"]["thirdParty"]["embedUrl"],
        "https://www.youtube.com/watch?v=abc123XYZ-_ "
    );
    // STS rides playbackContext for useSignatureTimestamp clients
    assert_eq!(
        body["playbackContext"]["contentPlaybackContext"]["signatureTimestamp"],
        20_000
    );
    assert_eq!(
        body["playbackContext"]["contentPlaybackContext"]["html5Preference"],
        "HTML5_PREF_WANTS"
    );
}

/// The search continuation rides BOTH query params
/// (InnerTube.kt:244-245).
#[tokio::test]
async fn search_continuation_rides_the_query_string() {
    let (addr, captured, server) = serve_responses(vec![empty_json()]).await;
    let session = session_with(None, None);
    let transport = test_transport(session, addr);
    let client = ClientRegistry::new().get(ClientKey::WebRemix);
    transport
        .search(&client, None, None, Some("CONTINUATIONTOKEN4D"))
        .await
        .unwrap();
    server.await.unwrap();
    let request = &captured.lock().unwrap()[0];
    assert_eq!(
        request.query("continuation").as_deref(),
        Some("CONTINUATIONTOKEN4D")
    );
    assert_eq!(
        request.query("ctoken").as_deref(),
        Some("CONTINUATIONTOKEN4D")
    );
}

/// get_transcript posts ONLY a Content-Type header with no client
/// headers and no visitor (InnerTube.kt:407-424).
#[tokio::test]
async fn get_transcript_is_header_minimal() {
    let (addr, captured, server) = serve_responses(vec![empty_json()]).await;
    let session = session_with(Some("CgtGUESTID0000000000"), None);
    let transport = test_transport(session, addr);
    let client = ClientRegistry::new().get(ClientKey::WebRemix);
    transport
        .get_transcript(&client, "dQw4w9WgXcQ")
        .await
        .unwrap();
    server.await.unwrap();
    let request = &captured.lock().unwrap()[0];
    assert!(
        request
            .request_line()
            .starts_with("POST /youtubei/v1/get_transcript")
    );
    assert_eq!(request.header("X-YouTube-Client-Name"), None);
    assert_eq!(request.header("X-Goog-Visitor-Id"), None);
    assert!(
        request
            .header("Content-Type")
            .map(|v| v.starts_with("application/json"))
            .unwrap_or(false)
    );
    let body: serde_json::Value = serde_json::from_str(&request.body()).unwrap();
    assert_eq!(body["params"], "CgtkUXc0dzlXZ1hjUQ==");
    assert!(body["context"]["client"].get("visitorData").is_none());
}

/// 429 short-circuits: exactly ONE request goes out and the error is the
/// typed rate limit (YTPlayerUtils.kt:1229-1233) — no retry, even for an
/// IO-labeled failure mode.
#[tokio::test]
async fn rate_limit_short_circuits_the_transport() {
    let (addr, captured, server) = serve_responses(vec![
        "HTTP/1.1 429 Too Many Requests\r\nContent-Length: 0\r\n\r\n".to_owned(),
    ])
    .await;
    let session = session_with(None, None);
    let transport = test_transport(session, addr);
    let client = ClientRegistry::new().get(ClientKey::WebRemix);
    let result = transport.search(&client, Some("q"), None, None).await;
    assert!(matches!(
        result,
        Err(mochy_innertube::InnertubeError::Net(
            mochy_net::NetError::YouTubeRateLimited
        ))
    ));
    server.await.unwrap();
    let requests = captured.lock().unwrap().clone();
    assert_eq!(requests.len(), 1, "429 must not be retried");
}

/// Offline mode: nothing goes out (InnerTube.kt:71-73 +
/// utils/OfflineMode.kt).
#[tokio::test]
async fn offline_mode_refuses_before_any_request() {
    let (addr, captured, server) = serve_responses(vec![]).await;
    let session = session_with(None, None);
    let transport = test_transport(session, addr);
    let client = ClientRegistry::new().get(ClientKey::WebRemix);
    mochy_net::set_offline(true);
    let result = transport.search(&client, Some("q"), None, None).await;
    mochy_net::set_offline(false);
    assert!(result.unwrap_err().is_offline());
    drop(server);
    assert!(
        captured.lock().unwrap().is_empty(),
        "no request may go out offline"
    );
}

/// The resumable playlist-image upload handshake: start captures
/// `x-guploader-uploadid`, finalize carries it as a query param
/// (InnerTube.kt:670-699).
#[tokio::test]
async fn playlist_image_upload_handshake() {
    let (addr, captured, server) = serve_responses(vec![
        common::serve::json_response_with_header("{}", "x-guploader-uploadid", "UPLOAD123"),
        empty_json(),
    ])
    .await;
    let session = session_with(None, None);
    let transport = test_transport(session, addr);
    let client = ClientRegistry::new().get(ClientKey::WebRemix);
    let upload_id = transport
        .playlist_image_upload_start(&client, 12_345)
        .await
        .unwrap();
    assert_eq!(upload_id, "UPLOAD123");
    transport
        .playlist_image_upload_finalize(&client, &upload_id, b"fakepngbytes!".to_vec())
        .await
        .unwrap();
    server.await.unwrap();
    let requests = captured.lock().unwrap().clone();
    assert_eq!(requests.len(), 2);
    let start = &requests[0];
    assert!(
        start
            .request_line()
            .starts_with("POST /playlist_image_upload/playlist_custom_thumbnail")
    );
    assert_eq!(
        start.header("X-Goog-Upload-Command").as_deref(),
        Some("start")
    );
    assert_eq!(
        start.header("X-Goog-Upload-Protocol").as_deref(),
        Some("resumable")
    );
    assert_eq!(
        start
            .header("X-Goog-Upload-Header-Content-Length")
            .as_deref(),
        Some("12345")
    );
    let finalize = &requests[1];
    assert_eq!(
        finalize.header("X-Goog-Upload-Command").as_deref(),
        Some("upload, finalize")
    );
    assert_eq!(
        finalize.header("X-Goog-Upload-Offset").as_deref(),
        Some("0")
    );
    assert_eq!(finalize.query("upload_id").as_deref(), Some("UPLOAD123"));
    assert_eq!(
        finalize.query("upload_protocol").as_deref(),
        Some("resumable")
    );
}

/// videostats play reporting: the s.youtube.com url is rewritten to the
/// music origin, ver/c/cver/cpn ride the query, and the playlist pair
/// appears when a playlist is set (InnerTube.kt:287-307 + YouTube.kt:1300).
#[tokio::test]
async fn videostats_reporting_shape() {
    let (addr, captured, server) = serve_responses(vec![empty_json()]).await;
    let session = session_with(None, None);
    let transport = test_transport(session, addr);
    let client = ClientRegistry::new().get(ClientKey::WebRemix);
    transport
        .register_playback(
            "https://s.youtube.com/api/stats/watchback?docid=abc&ns=yt",
            "CPN1234567890ABCD",
            Some("PL123"),
            &client,
            &[("st".to_owned(), "120".to_owned())],
        )
        .await
        .unwrap();
    server.await.unwrap();
    let request = &captured.lock().unwrap()[0];
    let line = request.request_line();
    assert!(
        line.starts_with("GET /api/stats/watchback"),
        "the rewritten path: {line}"
    );
    assert_eq!(request.query("ver"), Some("2".to_owned()));
    assert_eq!(request.query("c"), Some("WEB_REMIX".to_owned()));
    assert_eq!(request.query("cver"), Some("1.20260707.12.00".to_owned()));
    assert_eq!(request.query("cpn"), Some("CPN1234567890ABCD".to_owned()));
    assert_eq!(request.query("st"), Some("120".to_owned()));
    assert_eq!(request.query("list"), Some("PL123".to_owned()));
    assert_eq!(
        request.query("referrer"),
        Some("https://music.youtube.com/playlist?list=PL123".to_owned())
    );
    assert_eq!(request.query("docid"), Some("abc".to_owned()));
}

/// The BotGuard service requests carry the web-api-key header set
/// (PoTokenWebView.kt:251-280).
#[tokio::test]
async fn botguard_requests_carry_the_protobuf_headers() {
    let (addr, captured, server) = serve_responses(vec![
        json_response("[\"challenge-bytes\"]"),
        json_response("[\"integrity-token\",3600]"),
    ])
    .await;
    let session = session_with(None, None);
    let transport = test_transport(session, addr);
    let create = transport
        .botguard_create("O43z0dpjhgX20SCx4KAo")
        .await
        .unwrap();
    assert_eq!(create, "[\"challenge-bytes\"]");
    let generate = transport
        .botguard_generate_it("O43z0dpjhgX20SCx4KAo", "BOTGUARD_RESPONSE")
        .await
        .unwrap();
    assert_eq!(generate, "[\"integrity-token\",3600]");
    server.await.unwrap();
    let requests: Vec<CapturedRequest> = captured.lock().unwrap().clone();
    assert_eq!(requests.len(), 2);
    let create = &requests[0];
    assert!(create.request_line().starts_with("POST /api/jnn/v1/Create"));
    assert_eq!(
        create.header("Content-Type").as_deref(),
        Some("application/json+protobuf")
    );
    assert_eq!(
        create.header("x-goog-api-key").as_deref(),
        Some("AIzaSyDyT5W0Jh49F30Pqqtyfdf7pDLFKLJoAnw")
    );
    assert_eq!(
        create.header("x-user-agent").as_deref(),
        Some("grpc-web-javascript/0.1")
    );
    assert_eq!(create.body(), "[ \"O43z0dpjhgX20SCx4KAo\" ]");
    let generate = &requests[1];
    assert!(
        generate
            .request_line()
            .starts_with("POST /api/jnn/v1/GenerateIT")
    );
    assert_eq!(
        generate.body(),
        "[ \"O43z0dpjhgX20SCx4KAo\", \"BOTGUARD_RESPONSE\" ]"
    );
}

/// The sw.js_data request is a plain GET with the default headers only
/// (InnerTube.kt:434), and the visitor is scraped out of the payload
/// (YouTube.kt:1473-1483).
#[tokio::test]
async fn sw_js_data_scrape_end_to_end() {
    let payload = ")]}'\n[[\"sid\",\"other\",[\"ignored\",123,\"CgtSCRAPEDVISITOR\"]]]";
    let (addr, captured, server) =
        serve_responses(vec![common::serve::json_response(payload)]).await;
    let session = session_with(None, None);
    let transport = test_transport(session, addr);
    let visitor = transport.scrape_visitor_data().await.unwrap();
    assert_eq!(visitor, "CgtSCRAPEDVISITOR");
    server.await.unwrap();
    let requests = captured.lock().unwrap().clone();
    let request = &requests[0];
    assert!(request.request_line().starts_with("GET /sw.js_data"));
    assert_eq!(request.header("X-YouTube-Client-Name"), None);
    // the failed-session rotation keeps the old id (BotDetectionMitigator
    // semantics are unit-tested in guest.rs)
    let _ = transport.rotate_guest_session().await;
}
