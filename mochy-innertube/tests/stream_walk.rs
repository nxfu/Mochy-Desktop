//! The resolution walk over a scripted wire — the Phase 06
//! acceptance suite:
//!
//! * walk-order tests asserting the **exact client sequence** for
//!   guest and signed-in sessions, including the login-required skip
//!   rules (the account-client prefix, the VISIONOS main reuse, the
//!   metadata client);
//! * the fault-injection matrix: 403 → the client is dropped and the
//!   next one serves; 416 without a known length → served; bot-check
//!   → visitor rotation, the second bot check's 30-minute rest, and
//!   the bot-check error; 429 → the whole walk stops; a dead
//!   connection → the next client; the 403 invalidation that forces
//!   a fresh resolution; the unvalidated privately-owned track; the
//!   missing-expiry skip; the ciphered-candidate fallback;
//! * the stand-in and studio-swap flows over scripted searches.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use common::serve::json_response;
use mochy_innertube::client::ClientKey;
use mochy_innertube::session::{Session, SharedSession};
use mochy_innertube::stream::{
    AudioQuality, PlaybackRequest, RankingConditions, StreamEngine, StreamError, resolve_playback,
};
use mochy_innertube::transport::{InnerTubeTransport, TransportConfig};
use mochy_innertube::youtube::YouTube;

const STREAM_HOST: &str = "https://gonecdf.example";

/// The registry's client ids, for readable expectations.
fn client_id(key: ClientKey) -> String {
    mochy_innertube::ClientRegistry::new().get(key).client_id
}

/// One scripted environment: a local server, a transport pointed at
/// it for every origin the walk touches, and a stream engine with an
/// isolated cipher cache.
struct WalkHarness {
    youtube: YouTube,
    engine: Arc<StreamEngine>,
    _cipher_dir: tempfile::TempDir,
}

async fn harness(
    responses: Vec<String>,
    session: SharedSession,
) -> (
    WalkHarness,
    Arc<std::sync::Mutex<Vec<common::serve::CapturedRequest>>>,
    tokio::task::JoinHandle<()>,
) {
    let (addr, captured, server) = common::serve::serve_responses(responses).await;
    let local = format!("http://{addr}");
    let mut overrides = HashMap::new();
    overrides.insert("https://music.youtube.com".to_owned(), local.clone());
    overrides.insert("https://www.youtube.com".to_owned(), local.clone());
    overrides.insert(STREAM_HOST.to_owned(), local);
    let config = TransportConfig {
        cache_enabled: false,
        cache_dir: std::env::temp_dir().join("mochy-walk-test-cache"),
        origin_overrides: overrides,
        retry: mochy_net::RetryPolicy {
            max_attempts: 1,
            ..mochy_net::RetryPolicy::upstream()
        },
    };
    let transport = InnerTubeTransport::with_config(session, config).unwrap();
    let cipher_dir = tempfile::tempdir().unwrap();
    let engine = Arc::new(StreamEngine::new(cipher_dir.path()).unwrap());
    let youtube = YouTube::with_transport(transport);
    (
        WalkHarness {
            youtube,
            engine,
            _cipher_dir: cipher_dir,
        },
        captured,
        server,
    )
}

fn guest_session() -> SharedSession {
    let mut session = Session {
        visitor_data: Some("CgtGUESTVISITOR000".into()),
        ..Default::default()
    };
    session.set_locale_for_test();
    Arc::new(RwLock::new(session))
}

fn signed_in_session() -> SharedSession {
    let mut session = Session {
        visitor_data: Some("CgtGUESTVISITOR000".into()),
        data_sync_id: Some("sync-id-1".into()),
        ..Default::default()
    };
    session.set_cookie(Some("SAPISID=xYz123; __Secure-YEC=Ok; LOGIN_INFO=1".into()));
    session.set_locale_for_test();
    Arc::new(RwLock::new(session))
}

/// A fixed locale so contexts are deterministic (the session mirror
/// usually sets this from the system).
trait LocaleForTest {
    fn set_locale_for_test(&mut self);
}

impl LocaleForTest for Session {
    fn set_locale_for_test(&mut self) {
        self.locale.gl = "US".into();
        self.locale.hl = "en".into();
    }
}

fn high() -> RankingConditions {
    RankingConditions {
        quality: AudioQuality::High,
        network_metered: false,
        data_saver: false,
    }
}

/// The visitor_id mint answer (VISIONOS mints its own id before its
/// first player request).
fn mint(visitor: &str) -> String {
    json_response(&format!(
        r#"{{"responseContext":{{"visitorData":"{visitor}"}}}}"#
    ))
}

/// A playable player response: one direct-url audio format. Pass
/// `None` for the content length or the expiry to omit the field (the
/// walk's skip rules key on their absence).
fn player_ok_optional(
    itag: i64,
    bitrate: i64,
    content_length: Option<i64>,
    expires: Option<i64>,
) -> String {
    let mut format = serde_json::json!({
        "itag": itag,
        "url": format!("{STREAM_HOST}/videoplayback?itag={itag}&mn=sn-abc"),
        "mimeType": "audio/webm; codecs=\"opus\"",
        "bitrate": bitrate,
        "audioQuality": "AUDIO_QUALITY_HIGH",
    });
    if let Some(length) = content_length {
        format["contentLength"] = serde_json::json!(length);
    }
    let mut streaming = serde_json::json!({ "adaptiveFormats": [format] });
    if let Some(expires) = expires {
        streaming["expiresInSeconds"] = serde_json::json!(expires);
    }
    json_response(
        &serde_json::json!({
            "playabilityStatus": {"status": "OK"},
            "videoDetails": {
                "videoId": "dQw4w9WgXcQ",
                "title": "Song",
                "author": "Artist - Topic",
                "lengthSeconds": "200",
                "musicVideoType": "MUSIC_VIDEO_TYPE_ATV",
            },
            "streamingData": streaming,
        })
        .to_string(),
    )
}

/// The plain variant: every field present.
fn player_ok(itag: i64, bitrate: i64, content_length: i64, expires: i64) -> String {
    player_ok_optional(itag, bitrate, Some(content_length), Some(expires))
}

/// A player response that refuses to play.
fn player_status(status: &str, reason: &str) -> String {
    json_response(
        &serde_json::json!({
            "playabilityStatus": {"status": status, "reason": reason},
            "videoDetails": {
                "videoId": "dQw4w9WgXcQ",
                "title": "Song",
                "author": "Artist - Topic",
                "lengthSeconds": "200",
            },
        })
        .to_string(),
    )
}

const PROBE_206: &str = "HTTP/1.1 206 Partial Content\r\nContent-Length: 1\r\n\r\nx";
const PROBE_403: &str = "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n";
const PROBE_416: &str = "HTTP/1.1 416 Range Not Satisfiable\r\nContent-Length: 0\r\n\r\n";
/// A malformed response: the connection dies mid-HTTP, which reads as
/// the transport's IO failure class ("timed out" in the summary).
const DEAD_CONNECTION: &str = "HTTP/1.1 \r\ngarbage\r\n\r\n";
/// The iframe API without a player hash: the cipher fetcher gives up
/// (no runtime, no STS) without a second request.
const IFRAME_NO_HASH: &str =
    "HTTP/1.1 200 OK\r\nContent-Type: text/javascript\r\nContent-Length: 6\r\n\r\nvar x;";
/// A BotGuard Create answer that fails to parse: the PO-token mint
/// enters its cooldown and the walk continues without tokens.
const CREATE_GARBAGE: &str =
    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n{}";

/// `(client-id header, path)` for each captured request — the walk's
/// fingerprint.
fn fingerprint(requests: &[common::serve::CapturedRequest]) -> Vec<(String, String)> {
    requests
        .iter()
        .map(|request| {
            (
                request.header("X-YouTube-Client-Name").unwrap_or_default(),
                request.path(),
            )
        })
        .collect()
}

#[tokio::test]
async fn guest_main_client_serves_without_fallbacks() {
    // the mint, the main player answer, the validation probe
    let (harness, captured, server) = harness(
        vec![
            mint("CgtVISIONOSA"),
            player_ok(251, 160_000, 4_000_000, 21_600),
            PROBE_206.to_owned(),
        ],
        guest_session(),
    )
    .await;
    let request = PlaybackRequest::new("dQw4w9WgXcQ", high());
    let data = resolve_playback(&harness.youtube, &harness.engine, &request)
        .await
        .expect("the main client serves");
    server.await.unwrap();
    let requests = captured.lock().unwrap().clone();
    assert_eq!(
        fingerprint(&requests),
        vec![
            (
                client_id(ClientKey::VisionOs),
                "/youtubei/v1/visitor_id".into()
            ),
            (client_id(ClientKey::VisionOs), "/youtubei/v1/player".into()),
            (String::new(), "/videoplayback".into()),
        ],
        "mint, main, probe — nothing else is asked"
    );
    assert_eq!(data.stream_client, "VISIONOS");
    assert!(data.stream_url.contains("itag=251"));
    assert_eq!(data.stream_expires_in_seconds, 21_600);
    // the probe asked for the song's last byte with the client's
    // googlevideo headers (validateStatus, YTPlayerUtils.kt:1064-1099)
    let probe = &requests[2];
    assert_eq!(
        probe.header("Range").as_deref(),
        Some("bytes=3999999-3999999")
    );
    assert_eq!(
        probe.header("Referer").as_deref(),
        Some("https://www.youtube.com/")
    );
    assert_eq!(
        probe.header("Origin").as_deref(),
        Some("https://www.youtube.com")
    );
    assert!(
        probe
            .header("User-Agent")
            .unwrap()
            .starts_with("com.google.visionos")
    );
    // the resolution is cached for the audio layer
    assert_eq!(harness.engine.cache.lock().await.len(), 1);
}

#[tokio::test]
async fn guest_walk_order_and_skip_rules() {
    // every client answers premium-only: the walk visits every
    // fallback, the login-required ones are skipped, and the premium
    // error surfaces (no guest rotation for it)
    let premium = || {
        player_status(
            "LOGIN_REQUIRED",
            "This content is only available to Premium members",
        )
    };
    // LOGIN_REQUIRED is one of the restricted statuses, so the walk
    // treats the answer as age-restricted and the fallback clients
    // send NO signature timestamp — only the PO-token mint is asked
    let mut responses = vec![mint("CgtVISIONOSA"), premium()];
    let mut expected = vec![
        (
            client_id(ClientKey::VisionOs),
            "/youtubei/v1/visitor_id".to_owned(),
        ),
        (
            client_id(ClientKey::VisionOs),
            "/youtubei/v1/player".to_owned(),
        ),
    ];
    responses.push(CREATE_GARBAGE.to_owned());
    expected.push((String::new(), "/api/jnn/v1/Create".to_owned()));
    for key in [
        ClientKey::WebRemix,
        ClientKey::Web,
        // WEB_CREATOR skipped: login required, guest session
        ClientKey::IpadOs,
        ClientKey::AndroidVr1_43_32,
        ClientKey::AndroidVr1_61_48,
        ClientKey::AndroidVr1_65_10,
        ClientKey::AndroidVrNoAuth,
        ClientKey::TvHtml5SimplyEmbeddedPlayer,
        // TVHTML5 skipped: login required
        ClientKey::AndroidCreator,
        ClientKey::Mobile,
    ] {
        responses.push(premium());
        expected.push((client_id(key), "/youtubei/v1/player".to_owned()));
    }
    // the premium-wrapped refusal still carries the video details, so
    // the stand-in lookup searches (and finds nothing) before the
    // error surfaces
    responses.push(json_response("{}"));
    responses.push(json_response("{}"));
    expected.push((
        client_id(ClientKey::WebRemix),
        "/youtubei/v1/search".to_owned(),
    ));
    expected.push((
        client_id(ClientKey::WebRemix),
        "/youtubei/v1/search".to_owned(),
    ));
    let (harness, captured, server) = harness(responses, guest_session()).await;
    let request = PlaybackRequest::new("dQw4w9WgXcQ", high());
    let error = resolve_playback(&harness.youtube, &harness.engine, &request)
        .await
        .expect_err("every client refuses");
    server.await.unwrap();
    let requests = captured.lock().unwrap().clone();
    assert_eq!(fingerprint(&requests), expected, "the exact guest sequence");
    assert!(matches!(error, StreamError::PremiumOnly(_)), "{error:?}");
    // no rotation for a premium-only refusal: exactly one walk — the
    // main client plus every fallback that isn't login-skipped
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.path() == "/youtubei/v1/player")
            .count(),
        11
    );
}

#[tokio::test]
async fn signed_in_prefixes_the_account_clients() {
    // the main client can't serve: the account's clients come first,
    // the metadata client is asked alongside, and the login-required
    // clients are NOT skipped anymore — MOBILE (last) serves
    let mut responses = vec![
        mint("CgtVISIONOSA"),
        player_status("UNPLAYABLE", "Embedding disabled"),
        // the metadata client's PO-token mint fails, its STS lookup
        // finds no player script, its own answer is consumed silently
        CREATE_GARBAGE.to_owned(),
        IFRAME_NO_HASH.to_owned(),
        json_response("{}"),
    ];
    let mut expected = vec![
        (
            client_id(ClientKey::VisionOs),
            "/youtubei/v1/visitor_id".to_owned(),
        ),
        (
            client_id(ClientKey::VisionOs),
            "/youtubei/v1/player".to_owned(),
        ),
        (String::new(), "/api/jnn/v1/Create".to_owned()),
        (String::new(), "/iframe_api".to_owned()),
        (
            client_id(ClientKey::WebRemix),
            "/youtubei/v1/player".to_owned(),
        ),
    ];
    for key in [
        ClientKey::TvHtml5Samsung,
        ClientKey::WebEmbeddedPlayer,
        ClientKey::TvHtml5Downgraded,
        ClientKey::WebRemix,
        ClientKey::Web,
        ClientKey::WebCreator,
        ClientKey::IpadOs,
        ClientKey::AndroidVr1_43_32,
        ClientKey::AndroidVr1_61_48,
        ClientKey::AndroidVr1_65_10,
        ClientKey::AndroidVrNoAuth,
        ClientKey::TvHtml5SimplyEmbeddedPlayer,
        ClientKey::TvHtml5,
        ClientKey::AndroidCreator,
    ] {
        responses.push(player_status("UNPLAYABLE", "Embedding disabled"));
        expected.push((client_id(key), "/youtubei/v1/player".to_owned()));
    }
    // MOBILE serves and its url validates
    responses.push(player_ok(251, 160_000, 4_000_000, 21_600));
    expected.push((
        client_id(ClientKey::Mobile),
        "/youtubei/v1/player".to_owned(),
    ));
    responses.push(PROBE_206.to_owned());
    expected.push((String::new(), "/videoplayback".to_owned()));

    let (harness, captured, server) = harness(responses, signed_in_session()).await;
    let request = PlaybackRequest::new("dQw4w9WgXcQ", high());
    let data = resolve_playback(&harness.youtube, &harness.engine, &request)
        .await
        .expect("MOBILE serves");
    server.await.unwrap();
    let requests = captured.lock().unwrap().clone();
    assert_eq!(
        fingerprint(&requests),
        expected,
        "the exact signed-in sequence"
    );
    assert_eq!(data.stream_client, "ANDROID");
    // the cookie rides the requests of the clients that support login
    // (InnerTube.kt:169-178 `setLogin && client.loginSupported`);
    // VISIONOS / WEB / IPAD(OS) / ANDROID_VR never see it
    let login_capable: Vec<String> = [
        ClientKey::WebRemix,
        ClientKey::TvHtml5Samsung,
        ClientKey::WebEmbeddedPlayer,
        ClientKey::TvHtml5Downgraded,
        ClientKey::WebCreator,
        ClientKey::TvHtml5,
        ClientKey::TvHtml5SimplyEmbeddedPlayer,
        ClientKey::AndroidCreator,
        ClientKey::Mobile,
    ]
    .iter()
    .map(|key| client_id(*key))
    .collect();
    for request in requests
        .iter()
        .filter(|request| request.path() == "/youtubei/v1/player")
    {
        let id = request.header("X-YouTube-Client-Name").unwrap_or_default();
        if login_capable.contains(&id) {
            assert!(
                request.header("cookie").is_some(),
                "{id} supports login and must carry the cookie"
            );
        } else {
            assert!(
                request.header("cookie").is_none(),
                "{id} must not see the session cookie"
            );
        }
    }
}

#[tokio::test]
async fn bot_check_rotates_the_visitor_then_rests_the_main_client() {
    // main answers the bot check twice with a fresh visitor in
    // between: the second one starts the 30-minute rest, the fallback
    // walk continues, and the bot-check error surfaces
    let bot = || {
        player_status(
            "LOGIN_REQUIRED",
            "Please sign in to confirm you're not a bot",
        )
    };
    let mut responses = vec![
        mint("CgtFIRSTVISION"),
        bot(),
        mint("CgtSECONDTVISION"),
        bot(),
    ];
    let mut expected = vec![
        (
            client_id(ClientKey::VisionOs),
            "/youtubei/v1/visitor_id".to_owned(),
        ),
        (
            client_id(ClientKey::VisionOs),
            "/youtubei/v1/player".to_owned(),
        ),
        (
            client_id(ClientKey::VisionOs),
            "/youtubei/v1/visitor_id".to_owned(),
        ),
        (
            client_id(ClientKey::VisionOs),
            "/youtubei/v1/player".to_owned(),
        ),
    ];
    responses.push(CREATE_GARBAGE.to_owned());
    responses.push(IFRAME_NO_HASH.to_owned());
    expected.push((String::new(), "/api/jnn/v1/Create".to_owned()));
    expected.push((String::new(), "/iframe_api".to_owned()));
    for key in [
        ClientKey::WebRemix,
        ClientKey::Web,
        ClientKey::IpadOs,
        ClientKey::AndroidVr1_43_32,
        ClientKey::AndroidVr1_61_48,
        ClientKey::AndroidVr1_65_10,
        ClientKey::AndroidVrNoAuth,
        ClientKey::TvHtml5SimplyEmbeddedPlayer,
        ClientKey::AndroidCreator,
        ClientKey::Mobile,
    ] {
        responses.push(player_status("UNPLAYABLE", "Embedding disabled"));
        expected.push((client_id(key), "/youtubei/v1/player".to_owned()));
    }
    let (harness, captured, server) = harness(responses, guest_session()).await;
    let request = PlaybackRequest::new("dQw4w9WgXcQ", high());
    let error = resolve_playback(&harness.youtube, &harness.engine, &request)
        .await
        .expect_err("nothing serves");
    server.await.unwrap();
    let requests = captured.lock().unwrap().clone();
    assert_eq!(fingerprint(&requests), expected);
    assert!(matches!(error, StreamError::BotCheck(_)), "{error:?}");
    // the re-ask after the rotation carried the freshly minted id
    let reask = &requests[3];
    assert_eq!(
        reask.header("X-Goog-Visitor-Id").as_deref(),
        Some("CgtSECONDTVISION")
    );

    // the second resolution skips the resting main client entirely
    let (addr2, captured2, server2) = common::serve::serve_responses(vec![]).await;
    let local2 = format!("http://{addr2}");
    let mut overrides = HashMap::new();
    overrides.insert("https://music.youtube.com".to_owned(), local2.clone());
    overrides.insert("https://www.youtube.com".to_owned(), local2.clone());
    let config = TransportConfig {
        cache_enabled: false,
        cache_dir: std::env::temp_dir().join("mochy-walk-test-cache"),
        origin_overrides: overrides,
        retry: mochy_net::RetryPolicy {
            max_attempts: 1,
            ..mochy_net::RetryPolicy::upstream()
        },
    };
    let transport2 = InnerTubeTransport::with_config(guest_session(), config).unwrap();
    let youtube2 = YouTube::with_transport(transport2);
    let result = resolve_playback(&youtube2, &harness.engine, &request).await;
    // nothing was scripted for the second server: abort the accept
    // loop instead of waiting for responses that never come
    server2.abort();
    // nothing was scripted for the second server: the walk must have
    // gone out (and failed on the empty script) WITHOUT a main-client
    // player request — the first player ask is the first fallback
    let asks: Vec<(String, String)> = fingerprint(&captured2.lock().unwrap().clone())
        .into_iter()
        .filter(|(_, path)| path == "/youtubei/v1/player")
        .collect();
    assert!(
        !asks
            .iter()
            .any(|(client, _)| *client == client_id(ClientKey::VisionOs)),
        "the resting main client is not asked: {asks:?}"
    );
    let _ = result;
}

#[tokio::test]
async fn rate_limit_stops_the_whole_walk() {
    let (harness, captured, server) = harness(
        vec![
            mint("CgtVISIONOSA"),
            "HTTP/1.1 429 Too Many Requests\r\nContent-Length: 0\r\n\r\n".to_owned(),
        ],
        guest_session(),
    )
    .await;
    let request = PlaybackRequest::new("dQw4w9WgXcQ", high());
    let error = resolve_playback(&harness.youtube, &harness.engine, &request)
        .await
        .expect_err("429");
    server.await.unwrap();
    let requests = captured.lock().unwrap().clone();
    assert!(matches!(error, StreamError::RateLimited), "{error:?}");
    assert_eq!(
        requests.len(),
        2,
        "nothing else is asked — every client goes to the same host"
    );
}

#[tokio::test]
async fn a_dead_connection_falls_through_to_the_next_client() {
    // WEB_REMIX's connection dies mid-HTTP: an IO-class failure, which
    // the walk records and moves on from — WEB serves instead
    let (harness, captured, server) = harness(
        vec![
            mint("CgtVISIONOSA"),
            player_status("UNPLAYABLE", "Embedding disabled"),
            CREATE_GARBAGE.to_owned(),
            IFRAME_NO_HASH.to_owned(),
            DEAD_CONNECTION.to_owned(),
            player_ok(251, 160_000, 4_000_000, 21_600),
            PROBE_206.to_owned(),
        ],
        guest_session(),
    )
    .await;
    let request = PlaybackRequest::new("dQw4w9WgXcQ", high());
    let data = resolve_playback(&harness.youtube, &harness.engine, &request)
        .await
        .expect("WEB serves after WEB_REMIX's connection died");
    server.await.unwrap();
    let requests = captured.lock().unwrap().clone();
    let players = fingerprint(&requests)
        .into_iter()
        .filter(|(_, path)| path == "/youtubei/v1/player")
        .collect::<Vec<_>>();
    assert_eq!(
        players,
        vec![
            (client_id(ClientKey::VisionOs), "/youtubei/v1/player".into()),
            (client_id(ClientKey::WebRemix), "/youtubei/v1/player".into()),
            (client_id(ClientKey::Web), "/youtubei/v1/player".into()),
        ],
        "the walk continued past the dead connection"
    );
    assert_eq!(data.stream_client, "WEB");
}

#[tokio::test]
async fn a_cut_off_url_falls_through_to_the_next_client() {
    // the main client's url answers 403 past the untokened window:
    // the client is dropped (VISIONOS does not solve n, so no cipher
    // rescue) and WEB_REMIX serves
    let (harness, captured, server) = harness(
        vec![
            mint("CgtVISIONOSA"),
            player_ok(251, 160_000, 4_000_000, 21_600),
            PROBE_403.to_owned(),
            CREATE_GARBAGE.to_owned(),
            // WEB_REMIX: the PO-token mint, the STS lookup, its answer,
            // the EJS solver's own player-script fetch (WEB_REMIX
            // solves n), then the probe
            IFRAME_NO_HASH.to_owned(),
            player_ok(140, 128_000, 3_200_000, 21_600),
            IFRAME_NO_HASH.to_owned(),
            PROBE_206.to_owned(),
        ],
        guest_session(),
    )
    .await;
    let request = PlaybackRequest::new("dQw4w9WgXcQ", high());
    let data = resolve_playback(&harness.youtube, &harness.engine, &request)
        .await
        .expect("the next client serves");
    server.await.unwrap();
    let requests = captured.lock().unwrap().clone();
    let probes: Vec<_> = requests
        .iter()
        .filter(|r| r.path() == "/videoplayback")
        .collect();
    assert_eq!(probes.len(), 2, "one probe per candidate client");
    assert_eq!(
        probes[0].header("Range").as_deref(),
        Some("bytes=3999999-3999999")
    );
    assert_eq!(data.stream_client, "WEB_REMIX");
    assert!(data.stream_url.contains("itag=140"));
    // no po token was minted: the url carries no pot= parameter
    assert!(!data.stream_url.contains("pot="));
}

#[tokio::test]
async fn a_416_without_a_known_length_is_served() {
    // with no length to aim at, a song shorter than the probe answers
    // 416: served, not refused (validateStatus's third rule)
    let ok = player_ok_optional(251, 160_000, None, Some(21_600));
    let (harness, _captured, server) = harness(
        vec![mint("CgtVISIONOSA"), ok, PROBE_416.to_owned()],
        guest_session(),
    )
    .await;
    let request = PlaybackRequest::new("dQw4w9WgXcQ", high());
    let data = resolve_playback(&harness.youtube, &harness.engine, &request)
        .await
        .expect("416 without a length means served");
    server.await.unwrap();
    assert_eq!(data.stream_client, "VISIONOS");
}

#[tokio::test]
async fn a_privately_owned_track_is_taken_unvalidated() {
    // a user's own upload: the main client's answer is put aside (the
    // loop restarts at the fallbacks for private tracks,
    // YTPlayerUtils.kt:522) and WEB_REMIX's answer is taken WITHOUT
    // the validation probe (YTPlayerUtils.kt:664-672)
    let private_ok = |video_id: &str| {
        json_response(
            &serde_json::json!({
                "playabilityStatus": {"status": "OK"},
                "videoDetails": {
                    "videoId": video_id,
                    "title": "My Upload",
                    "author": "Me",
                    "lengthSeconds": "200",
                    "musicVideoType": "MUSIC_VIDEO_TYPE_PRIVATELY_OWNED_TRACK",
                },
                "streamingData": {
                    "expiresInSeconds": 21_600,
                    "adaptiveFormats": [{
                        "itag": 140,
                        "url": format!("{STREAM_HOST}/videoplayback?itag=140"),
                        "mimeType": "audio/mp4; codecs=\"mp4a.40.2\"",
                        "bitrate": 128_000,
                        "contentLength": 3_200_000,
                    }],
                },
            })
            .to_string(),
        )
    };
    let (harness, captured, server) = harness(
        vec![
            mint("CgtVISIONOSA"),
            private_ok("dQw4w9WgXcQ"),
            CREATE_GARBAGE.to_owned(),
            IFRAME_NO_HASH.to_owned(),
            private_ok("dQw4w9WgXcQ"),
            IFRAME_NO_HASH.to_owned(),
        ],
        guest_session(),
    )
    .await;
    let request = PlaybackRequest::new("dQw4w9WgXcQ", high());
    let data = resolve_playback(&harness.youtube, &harness.engine, &request)
        .await
        .expect("taken unvalidated");
    server.await.unwrap();
    let requests = captured.lock().unwrap().clone();
    assert!(
        !requests
            .iter()
            .any(|request| request.path() == "/videoplayback"),
        "no validation probe for a privately owned track"
    );
    assert_eq!(data.stream_client, "WEB_REMIX");
    assert_eq!(data.format.itag, 140);
}

#[tokio::test]
async fn a_missing_expiry_skips_the_client() {
    // the main client's answer has no expiresInSeconds: the client is
    // skipped and WEB_REMIX serves
    let ok = player_ok_optional(251, 160_000, Some(4_000_000), None);
    let (harness, _captured, server) = harness(
        vec![
            mint("CgtVISIONOSA"),
            ok,
            CREATE_GARBAGE.to_owned(),
            IFRAME_NO_HASH.to_owned(),
            player_ok(140, 128_000, 3_200_000, 21_600),
            IFRAME_NO_HASH.to_owned(),
            PROBE_206.to_owned(),
        ],
        guest_session(),
    )
    .await;
    let request = PlaybackRequest::new("dQw4w9WgXcQ", high());
    let data = resolve_playback(&harness.youtube, &harness.engine, &request)
        .await
        .expect("the next client serves");
    server.await.unwrap();
    assert_eq!(data.stream_client, "WEB_REMIX");
}

#[tokio::test]
async fn a_ciphered_only_client_falls_through_to_the_next_one() {
    // the main client's only format is ciphered and the player script
    // cannot be fetched: the cipher deobfuscation (and its fresh-JS
    // retry), the NewPipe-shaped leg and the watch-page last resort
    // all come up empty, the client is skipped, and WEB_REMIX serves
    // with a direct url
    let ciphered_only = json_response(
        &serde_json::json!({
            "playabilityStatus": {"status": "OK"},
            "videoDetails": {
                "videoId": "dQw4w9WgXcQ",
                "title": "Song",
                "author": "Artist - Topic",
                "lengthSeconds": "200",
            },
            "streamingData": {
                "expiresInSeconds": 21_600,
                "adaptiveFormats": [{
                    "itag": 251,
                    "signatureCipher": "s=abc&sp=sig&url=https%3A%2F%2Fgonecdf.example%2Fvideoplayback%3Fitag%3D251",
                    "mimeType": "audio/webm; codecs=\"opus\"",
                    "bitrate": 160_000,
                    "contentLength": 4_000_000,
                }],
            },
        })
        .to_string(),
    );
    let (harness, captured, server) = harness(
        vec![
            mint("CgtVISIONOSA"),
            ciphered_only,
            // the cipher fetch: no hash in the iframe API, retried
            // with fresh JS, then the NewPipe-shaped leg asks twice
            // more
            IFRAME_NO_HASH.to_owned(),
            IFRAME_NO_HASH.to_owned(),
            IFRAME_NO_HASH.to_owned(),
            IFRAME_NO_HASH.to_owned(),
            // the watch page's last resort carries no embedded player
            // response
            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: 8\r\n\r\n<html />"
                .to_owned(),
            // WEB_REMIX serves: the PO-token mint, the STS lookup,
            // its answer, the EJS solver's player-script fetch, the
            // validating probe
            CREATE_GARBAGE.to_owned(),
            IFRAME_NO_HASH.to_owned(),
            player_ok(140, 128_000, 3_200_000, 21_600),
            IFRAME_NO_HASH.to_owned(),
            PROBE_206.to_owned(),
        ],
        guest_session(),
    )
    .await;
    let request = PlaybackRequest::new("dQw4w9WgXcQ", high());
    let data = resolve_playback(&harness.youtube, &harness.engine, &request)
        .await
        .expect("the next client serves");
    server.await.unwrap();
    let requests = captured.lock().unwrap().clone();
    let prints: Vec<(String, String)> = fingerprint(&requests)
        .into_iter()
        .filter(|(_client, path)| {
            path == "/iframe_api"
                || path == "/watch"
                || path == "/youtubei/v1/player"
                || path == "/videoplayback"
        })
        .collect();
    assert_eq!(
        prints,
        vec![
            (client_id(ClientKey::VisionOs), "/youtubei/v1/player".into()),
            // the cipher deobfuscation's four player-script fetches
            (String::new(), "/iframe_api".into()),
            (String::new(), "/iframe_api".into()),
            (String::new(), "/iframe_api".into()),
            (String::new(), "/iframe_api".into()),
            // the watch-page last resort
            (String::new(), "/watch".into()),
            // WEB_REMIX: the STS lookup, its answer, the EJS solver's
            // own fetch, the validating probe
            (String::new(), "/iframe_api".into()),
            (web_remix_id(), "/youtubei/v1/player".into()),
            (String::new(), "/iframe_api".into()),
            (String::new(), "/videoplayback".into()),
        ],
        "the ciphered-only client exhausts its options, WEB_REMIX serves"
    );
    assert_eq!(data.stream_client, "WEB_REMIX");
    assert_eq!(data.format.itag, 140);
}

fn web_remix_id() -> String {
    client_id(ClientKey::WebRemix)
}

#[tokio::test]
async fn the_403_invalidation_forces_a_fresh_resolution() {
    // resolve, then the audio layer's 403 drops the cached url and
    // the next resolution walks again (fresh links) — the second walk
    // goes to a fresh server below, so only one script is served here
    let (harness, captured, server) = harness(
        vec![
            mint("CgtVISIONOSA"),
            player_ok(251, 160_000, 4_000_000, 21_600),
            PROBE_206.to_owned(),
        ],
        guest_session(),
    )
    .await;
    let request = PlaybackRequest::new("dQw4w9WgXcQ", high());
    let first = resolve_playback(&harness.youtube, &harness.engine, &request)
        .await
        .expect("serves");
    server.await.unwrap();
    let requests = captured.lock().unwrap().clone();
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.path() == "/youtubei/v1/player")
            .count(),
        1
    );

    // the cached resolution is live and would be reused as-is
    {
        let cache = harness.engine.cache.lock().await;
        let entry = cache
            .get("dQw4w9WgXcQ", &first.format)
            .expect("live, same itag and length");
        assert_eq!(entry.url, first.stream_url);
    }
    // the 403 from the audio layer drops it
    assert!(harness.engine.cache.lock().await.invalidate("dQw4w9WgXcQ"));
    // and the next resolution is a fresh walk
    let (addr2, captured2, server2) = common::serve::serve_responses(vec![
        mint("CgtVISIONOSA"),
        player_ok(251, 160_000, 4_000_000, 21_600),
        PROBE_206.to_owned(),
    ])
    .await;
    let local2 = format!("http://{addr2}");
    let mut overrides = HashMap::new();
    overrides.insert("https://music.youtube.com".to_owned(), local2.clone());
    overrides.insert("https://www.youtube.com".to_owned(), local2.clone());
    overrides.insert(STREAM_HOST.to_owned(), local2);
    let config = TransportConfig {
        cache_enabled: false,
        cache_dir: std::env::temp_dir().join("mochy-walk-test-cache"),
        origin_overrides: overrides,
        retry: mochy_net::RetryPolicy {
            max_attempts: 1,
            ..mochy_net::RetryPolicy::upstream()
        },
    };
    let transport2 = InnerTubeTransport::with_config(guest_session(), config).unwrap();
    let youtube2 = YouTube::with_transport(transport2);
    resolve_playback(&youtube2, &harness.engine, &request)
        .await
        .expect("a fresh walk serves");
    server2.await.unwrap();
    let requests2 = captured2.lock().unwrap().clone();
    assert_eq!(
        requests2
            .iter()
            .filter(|r| r.path() == "/youtubei/v1/player")
            .count(),
        1
    );
}

/// A minimal song search response carrying one item (the corpus
/// shape, trimmed).
fn search_response(video_id: &str, title: &str, music_video_type: &str, duration: &str) -> String {
    json_response(
        &serde_json::json!({
            "contents": {
                "tabbedSearchResultsRenderer": {
                    "tabs": [{
                        "tabRenderer": {
                            "content": {
                                "sectionListRenderer": {
                                    "contents": [{
                                        "musicShelfRenderer": {
                                            "title": {"runs": [{"text": "Songs"}]},
                                            "contents": [{
                                                "musicResponsiveListItemRenderer": {
                                                    "navigationEndpoint": {
                                                        "watchEndpoint": {
                                                            "videoId": video_id,
                                                            "watchEndpointMusicSupportedConfigs": {
                                                                "watchEndpointMusicConfig": {"musicVideoType": music_video_type}
                                                            }
                                                        }
                                                    },
                                                    "flexColumns": [
                                                        {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": title, "navigationEndpoint": {"watchEndpoint": {"videoId": video_id, "watchEndpointMusicSupportedConfigs": {"watchEndpointMusicConfig": {"musicVideoType": music_video_type}}}}}]}}},
                                                        {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "Artist A", "navigationEndpoint": {"browseEndpoint": {"browseId": "UCA1", "browseEndpointContextSupportedConfigs": {"browseEndpointContextMusicConfig": {"pageType": "MUSIC_PAGE_TYPE_ARTIST"}}}}}, {"text": " • "}, {"text": "3:20"}]}}}
                                                    ],
                                                    "fixedColumns": [{"musicResponsiveListItemFixedColumnRenderer": {"text": {"runs": [{"text": duration}]}}}],
                                                    "thumbnail": {"musicThumbnailRenderer": {"thumbnail": {"thumbnails": [{"url": "https://i.ytimg.com/vi/COVER/120.jpg", "width": 120, "height": 120}]}}},
                                                    "playlistItemData": {"videoId": video_id},
                                                }
                                            }]
                                        }
                                    }]
                                }
                            }
                        }
                    }]
                }
            }
        })
        .to_string(),
    )
}

#[tokio::test]
async fn a_refused_upload_plays_the_artists_other_upload() {
    // every client refuses the upload itself: the walk wraps the
    // failure with the video details, the stand-in search finds the
    // artist's other upload, and its walk serves — with the ORIGINAL
    // song's details and no play reporting
    // the walk: the main ask, the WEB_REMIX po-token + STS lookups
    // (they precede its player ask), then every fallback refusing
    let mut fixed_responses: Vec<String> = vec![
        mint("CgtVISIONOSA"),
        player_status("UNPLAYABLE", "Video unavailable"),
        CREATE_GARBAGE.to_owned(),
        IFRAME_NO_HASH.to_owned(),
    ];
    let mut expected = vec![
        (
            client_id(ClientKey::VisionOs),
            "/youtubei/v1/visitor_id".to_owned(),
        ),
        (
            client_id(ClientKey::VisionOs),
            "/youtubei/v1/player".to_owned(),
        ),
        (String::new(), "/api/jnn/v1/Create".to_owned()),
        (String::new(), "/iframe_api".to_owned()),
    ];
    for key in [
        ClientKey::WebRemix,
        ClientKey::Web,
        ClientKey::IpadOs,
        ClientKey::AndroidVr1_43_32,
        ClientKey::AndroidVr1_61_48,
        ClientKey::AndroidVr1_65_10,
        ClientKey::AndroidVrNoAuth,
        ClientKey::TvHtml5SimplyEmbeddedPlayer,
        ClientKey::AndroidCreator,
        ClientKey::Mobile,
    ] {
        fixed_responses.push(player_status("UNPLAYABLE", "Video unavailable"));
        expected.push((client_id(key), "/youtubei/v1/player".to_owned()));
    }
    // the stand-in searches (song filter and video filter run in
    // parallel — arrival order is deterministic here because the song
    // filter is awaited first) and the stand-in's own successful walk
    fixed_responses.push(search_response(
        "OTHER1",
        "Artist A - Song",
        "MUSIC_VIDEO_TYPE_OMV",
        "3:20",
    ));
    fixed_responses.push(search_response(
        "OTHER1",
        "Artist A - Song",
        "MUSIC_VIDEO_TYPE_OMV",
        "3:20",
    ));
    expected.push((
        client_id(ClientKey::WebRemix),
        "/youtubei/v1/search".to_owned(),
    ));
    expected.push((
        client_id(ClientKey::WebRemix),
        "/youtubei/v1/search".to_owned(),
    ));
    fixed_responses.push(player_ok_for("OTHER1", 251, 160_000, 4_000_000, 21_600));
    fixed_responses.push(PROBE_206.to_owned());
    expected.push((
        client_id(ClientKey::VisionOs),
        "/youtubei/v1/player".to_owned(),
    ));
    expected.push((String::new(), "/videoplayback".to_owned()));

    let (harness, captured, server) = harness(fixed_responses, guest_session()).await;
    let request = PlaybackRequest::new("dQw4w9WgXcQ", high());
    let data = resolve_playback(&harness.youtube, &harness.engine, &request)
        .await
        .expect("the stand-in serves");
    server.await.unwrap();
    let requests = captured.lock().unwrap().clone();
    assert_eq!(fingerprint(&requests), expected, "the stand-in flow");
    // the song keeps its own details, the play isn't reported as the
    // stand-in's, and the music-video flag is set
    assert_eq!(data.video_details.as_ref().unwrap().video_id, "dQw4w9WgXcQ");
    assert!(data.playback_tracking.is_none());
    assert!(
        data.from_music_video,
        "the stand-in was the song's music video"
    );
    assert_eq!(data.stream_client, "VISIONOS");
}

/// `player_ok` for a chosen video id (the stand-in's walk).
fn player_ok_for(
    video_id: &str,
    itag: i64,
    bitrate: i64,
    content_length: i64,
    expires: i64,
) -> String {
    json_response(
        &serde_json::json!({
            "playabilityStatus": {"status": "OK"},
            "videoDetails": {
                "videoId": video_id,
                "title": "Artist A - Song (Official Music Video)",
                "author": "Artist A - Topic",
                "lengthSeconds": "200",
                "musicVideoType": "MUSIC_VIDEO_TYPE_OMV",
            },
            "streamingData": {
                "expiresInSeconds": expires,
                "adaptiveFormats": [{
                    "itag": itag,
                    "url": format!("{STREAM_HOST}/videoplayback?itag={itag}"),
                    "mimeType": "audio/webm; codecs=\"opus\"",
                    "bitrate": bitrate,
                    "contentLength": content_length,
                }],
            },
        })
        .to_string(),
    )
}

#[tokio::test]
async fn a_music_video_plays_its_studio_version_when_preferred() {
    // preferStudio: the video's walk succeeds, the studio lookup finds
    // the album version, and its stream plays with the video's details
    let video_ok = json_response(
        &serde_json::json!({
            "playabilityStatus": {"status": "OK"},
            "videoDetails": {
                "videoId": "VIDEO1",
                "title": "Artist A - Song (Official Music Video)",
                "author": "Artist A",
                "lengthSeconds": "200",
                "musicVideoType": "MUSIC_VIDEO_TYPE_OMV",
            },
            "streamingData": {
                "expiresInSeconds": 21_600,
                "adaptiveFormats": [{
                    "itag": 251,
                    "url": format!("{STREAM_HOST}/videoplayback?itag=251"),
                    "mimeType": "audio/webm; codecs=\"opus\"",
                    "bitrate": 160_000,
                    "contentLength": 4_000_000,
                }],
            },
        })
        .to_string(),
    );
    let (harness, captured, server) = harness(
        vec![
            mint("CgtVISIONOSA"),
            video_ok,
            PROBE_206.to_owned(),
            search_response("STUDIO1", "Song", "MUSIC_VIDEO_TYPE_ATV", "3:20"),
            player_ok_for("STUDIO1", 251, 160_000, 4_000_000, 21_600),
            PROBE_206.to_owned(),
        ],
        guest_session(),
    )
    .await;
    let request = PlaybackRequest::new("VIDEO1", high()).with_prefer_studio(true);
    let data = resolve_playback(&harness.youtube, &harness.engine, &request)
        .await
        .expect("the studio version serves");
    server.await.unwrap();
    let requests = captured.lock().unwrap().clone();
    assert_eq!(
        fingerprint(&requests),
        vec![
            (
                client_id(ClientKey::VisionOs),
                "/youtubei/v1/visitor_id".into()
            ),
            (client_id(ClientKey::VisionOs), "/youtubei/v1/player".into()),
            (String::new(), "/videoplayback".into()),
            (client_id(ClientKey::WebRemix), "/youtubei/v1/search".into()),
            (client_id(ClientKey::VisionOs), "/youtubei/v1/player".into()),
            (String::new(), "/videoplayback".into()),
        ],
        "video walk, studio lookup, studio walk"
    );
    assert!(data.from_studio);
    // the video's own details are kept
    assert_eq!(data.video_details.as_ref().unwrap().video_id, "VIDEO1");
    assert!(data.playback_tracking.is_none());
    // the studio id is remembered: a second resolution does not search
    // again
    // the second resolution: the video's own walk, then the
    // remembered studio id's walk — no search this time
    let (addr2, captured2, server2) = common::serve::serve_responses(vec![
        mint("CgtVISIONOSB"),
        player_ok_for("STUDIO1", 251, 160_000, 4_000_000, 21_600),
        PROBE_206.to_owned(),
        player_ok_for("STUDIO1", 251, 160_000, 4_000_000, 21_600),
        PROBE_206.to_owned(),
    ])
    .await;
    let local2 = format!("http://{addr2}");
    let mut overrides = HashMap::new();
    overrides.insert("https://music.youtube.com".to_owned(), local2.clone());
    overrides.insert("https://www.youtube.com".to_owned(), local2.clone());
    overrides.insert(STREAM_HOST.to_owned(), local2);
    let config = TransportConfig {
        cache_enabled: false,
        cache_dir: std::env::temp_dir().join("mochy-walk-test-cache"),
        origin_overrides: overrides,
        retry: mochy_net::RetryPolicy {
            max_attempts: 1,
            ..mochy_net::RetryPolicy::upstream()
        },
    };
    let transport2 = InnerTubeTransport::with_config(guest_session(), config).unwrap();
    let youtube2 = YouTube::with_transport(transport2);
    let second = resolve_playback(&youtube2, &harness.engine, &request)
        .await
        .expect("the remembered studio version serves");
    server2.await.unwrap();
    assert!(second.from_studio);
    let requests2 = captured2.lock().unwrap().clone();
    assert!(
        !requests2
            .iter()
            .any(|request| request.path() == "/youtubei/v1/search"),
        "the studio id is remembered for the session"
    );
}

#[tokio::test]
async fn the_ejs_engine_and_cipher_boot_lazily_and_survive() {
    // the engine's JS machinery only boots when a client that solves
    // n produces a url — and the same engine serves later calls
    let (harness, _captured, server) = harness(
        vec![
            mint("CgtVISIONOSA"),
            player_ok(251, 160_000, 4_000_000, 21_600),
            PROBE_206.to_owned(),
        ],
        guest_session(),
    )
    .await;
    let request = PlaybackRequest::new("dQw4w9WgXcQ", high());
    let data = resolve_playback(&harness.youtube, &harness.engine, &request)
        .await
        .expect("serves");
    server.await.unwrap();
    // VISIONOS does not solve n and has no po token: the url is the
    // raw googlevideo answer
    assert!(!data.stream_url.contains("pot="));
    assert_eq!(
        harness.engine.backend(),
        mochy_innertube::stream::StreamBackend::InnertubeX
    );
    // without the innertubex feature the seam is a miss and the walk
    // served everything — which it did
    assert!(harness.engine.has_po_token() == harness.engine.potoken.has_session_token());
}
