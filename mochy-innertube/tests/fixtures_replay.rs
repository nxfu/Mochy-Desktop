//! Offline fixture-replay tests — the transport round-trips recorded
//! (redacted) response fixtures without touching the network
//! (blueprint ch. 15: "innertube contract tests over recorded JSON").
//!
//! Fixtures live in `tests/fixtures/`; the committed ones are SYNTHETIC
//! (structurally faithful minimal payloads, no user data) — see
//! `tests/fixtures/README.md`. Real captures replace them by running
//! `cargo test -p mochy-innertube --features live --test live`, which is
//! NEVER enabled in CI.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use common::serve::{empty_json, json_response, serve_responses};
use mochy_innertube::client::{ClientKey, ClientRegistry};
use mochy_innertube::session::{Session, SharedSession};
use mochy_innertube::transport::{InnerTubeTransport, TransportConfig};
use serde_json::Value as Json;

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
    config.retry = mochy_net::RetryPolicy {
        max_attempts: 1,
        ..mochy_net::RetryPolicy::upstream()
    };
    InnerTubeTransport::with_config(session, config).unwrap()
}

/// The sw.js_data fixture replays through the scrape end to end.
#[tokio::test]
async fn sw_js_data_fixture_replays_through_the_scrape() {
    let Some(payload) = common::load_fixture_text("sw_js_data") else {
        eprintln!("SKIP: sw_js_data fixture not present");
        return;
    };
    // the fixture stores the JSON body; the sw.js_data wire form carries
    // the )]}' prefix (YouTube.kt:1474 substring(5))
    let wire = format!(")]}}'\n{payload}");
    let (addr, _captured, server) = serve_responses(vec![json_response(&wire)]).await;
    let session: SharedSession = Arc::new(RwLock::new(Session::default()));
    let transport = test_transport(session, addr);
    let visitor = transport.scrape_visitor_data().await.unwrap();
    // the synthetic fixture's visitor is a Cgt-prefixed placeholder
    assert!(
        visitor.starts_with("Cgt") || visitor.starts_with("Cgs"),
        "scraped visitor must look like one: {visitor}"
    );
    server.await.unwrap();
}

/// The visitor_id fixture replays through the mint.
#[tokio::test]
async fn visitor_id_fixture_replays_through_the_mint() {
    let Some(fixture) = common::fixture_or_skip("visitor_id") else {
        return;
    };
    let body = serde_json::to_string(&fixture).unwrap();
    let (addr, _captured, server) = serve_responses(vec![json_response(&body)]).await;
    let session: SharedSession = Arc::new(RwLock::new(Session::default()));
    let transport = test_transport(session, addr);
    let client = ClientRegistry::new().get(ClientKey::VisionOs);
    let visitor = transport.visitor_id(&client).await.unwrap();
    assert_eq!(
        visitor,
        fixture["responseContext"]["visitorData"].as_str().unwrap()
    );
    server.await.unwrap();
}

/// A search-response fixture replays through the search endpoint and the
/// raw JSON is returned verbatim for Phase 05's parsers.
#[tokio::test]
async fn search_fixture_replays_as_raw_json() {
    let Some(fixture) = common::fixture_or_skip("search") else {
        return;
    };
    let body = serde_json::to_string(&fixture).unwrap();
    let (addr, _captured, server) = serve_responses(vec![json_response(&body)]).await;
    let session: SharedSession = Arc::new(RwLock::new(Session::default()));
    let transport = test_transport(session, addr);
    let client = ClientRegistry::new().get(ClientKey::WebRemix);
    let response: Json = transport
        .search(&client, Some("fixture query"), None, None)
        .await
        .unwrap();
    // the response is the fixture verbatim (the transport does not
    // interpret bodies in Phase 04)
    assert_eq!(response, fixture);
    server.await.unwrap();
}

/// The redaction pass scrubs the sensitive keys but keeps the shape.
#[test]
fn redaction_keeps_shape_and_scrubs_values() {
    let recorded = serde_json::json!({
        "responseContext": {
            "visitorData": "CgtREALVISITORID12345",
            "trackingParams": "someTrackingParam"
        },
        "contents": [
            { "clickTrackingParams": "abc", "title": "Keep me" },
            { "nested": { "cpn": "XYZ", "poToken": "SECRET" } }
        ]
    });
    let redacted = common::redact(&recorded);
    assert_eq!(
        redacted["responseContext"]["visitorData"],
        "<redacted:visitordata>"
    );
    assert_eq!(
        redacted["responseContext"]["trackingParams"],
        "<redacted:trackingparams>"
    );
    assert_eq!(redacted["contents"][0]["title"], "Keep me");
    assert_eq!(redacted["contents"][1]["nested"]["cpn"], "<redacted:cpn>");
    assert_eq!(
        redacted["contents"][1]["nested"]["poToken"],
        "<redacted:potoken>"
    );
    // the shape (key sets) is unchanged
    let shape = |value: &Json| -> Vec<String> {
        let mut keys: Vec<String> = value
            .as_object()
            .unwrap()
            .keys()
            .map(|key| key.to_owned())
            .collect();
        keys.sort();
        keys
    };
    assert_eq!(shape(&recorded), shape(&redacted));
}

/// The disk cache serves a repeated GET from disk (the 50 MB GET cache,
/// InnerTube.kt:111-116): the second sw.js_data read makes NO request.
#[tokio::test]
async fn get_disk_cache_serves_repeats_without_requests() {
    let wire = ")]}'\n[[\"sid\",\"x\",[\"CgtCACHEDVISITOR\"]]]";
    // exactly ONE canned response: a second request would hang the server
    // (and fail the assertion) — the cache must serve the repeat
    let (addr, captured, server) = serve_responses(vec![json_response(wire)]).await;
    let session: SharedSession = Arc::new(RwLock::new(Session::default()));

    let cache_dir = tempfile::tempdir().unwrap();
    let local = format!("http://{addr}");
    let mut overrides = HashMap::new();
    overrides.insert("https://music.youtube.com".to_owned(), local);
    let mut config = TransportConfig {
        cache_dir: cache_dir.path().join("http_cache"),
        cache_enabled: true,
        ..TransportConfig::default()
    };
    config.origin_overrides = overrides;
    config.retry = mochy_net::RetryPolicy {
        max_attempts: 1,
        ..mochy_net::RetryPolicy::upstream()
    };
    let transport = InnerTubeTransport::with_config(session, config).unwrap();

    let first = transport.scrape_visitor_data().await.unwrap();
    assert_eq!(first, "CgtCACHEDVISITOR");
    let second = transport.scrape_visitor_data().await.unwrap();
    assert_eq!(second, "CgtCACHEDVISITOR");
    // exactly ONE request went out: the second read came from the cache.
    // (the server is detached — it would wait for a second connection
    // forever, which is the point)
    drop(server);
    assert_eq!(captured.lock().unwrap().len(), 1);
}

/// The retry wrapper retries IO failures and succeeds on a later attempt
/// (InnerTube.kt:204-223): a connection that resets once, then serves.
#[tokio::test]
async fn io_errors_retry_with_backoff() {
    // a server that closes the first connection without answering, then
    // serves the second
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        // first accept: read a bit, then abort (an IO error for reqwest)
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buf = [0u8; 1024];
        let _ = socket.read(&mut buf).await;
        socket.shutdown().await.unwrap();
        drop(socket);
        // second accept: serve
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut head = Vec::new();
        let mut buf = [0u8; 8192];
        loop {
            let n = socket.read(&mut buf).await.unwrap_or(0);
            if n == 0 {
                break;
            }
            head.extend_from_slice(&buf[..n]);
            let text = String::from_utf8_lossy(&head).into_owned();
            if let Some(split) = text.find("\r\n\r\n") {
                let content_length = text
                    .lines()
                    .find_map(|line| {
                        let (k, v) = line.split_once(':')?;
                        if k.trim().eq_ignore_ascii_case("content-length") {
                            v.trim().parse::<usize>().ok()
                        } else {
                            None
                        }
                    })
                    .unwrap_or(0);
                if head.len() >= split + 4 + content_length {
                    break;
                }
            }
        }
        socket.write_all(empty_json().as_bytes()).await.unwrap();
        socket.flush().await.unwrap();
    });

    let session: SharedSession = Arc::new(RwLock::new(Session::default()));
    let local = format!("http://{addr}");
    let mut overrides = HashMap::new();
    overrides.insert("https://music.youtube.com".to_owned(), local);
    let mut config = TransportConfig {
        cache_enabled: false,
        ..TransportConfig::default()
    };
    config.origin_overrides = overrides;
    // fast backoff: 10ms initial
    config.retry = mochy_net::RetryPolicy {
        max_attempts: 3,
        initial_delay: std::time::Duration::from_millis(10),
        factor: 2.0,
    };
    let transport = InnerTubeTransport::with_config(session, config).unwrap();
    let client = ClientRegistry::new().get(ClientKey::WebRemix);
    let result = transport.search(&client, Some("q"), None, None).await;
    assert!(result.is_ok(), "the retry must recover: {result:?}");
    server.await.unwrap();
}
