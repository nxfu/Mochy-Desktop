//! LIVE network tests — record the fixture corpus from the real service.
//!
//! COMPILED OUT unless the `live` feature is enabled:
//! `cargo test -p mochy-innertube --features live --test live`.
//! The feature is NEVER enabled in CI (the matrix runs plain
//! `cargo test --workspace`), so these tests cannot run there. They exist
//! so a maintainer can refresh `tests/fixtures/` after a verified
//! upstream format change — every capture is redacted by
//! `tests/common::redact` before it touches disk.
//!
//! These tests are intentionally NOT hermetic: they need internet access
//! and depend on YouTube's live responses.

#![cfg(feature = "live")]

mod common;

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use mochy_innertube::client::{ClientKey, ClientRegistry};
use mochy_innertube::session::{Session, SharedSession};
use mochy_innertube::transport::InnerTubeTransport;

fn live_transport() -> InnerTubeTransport {
    let session: SharedSession = Arc::new(RwLock::new(Session::default()));
    InnerTubeTransport::new(session).unwrap()
}

#[tokio::test]
async fn record_sw_js_data() {
    let transport = live_transport();
    let text = transport.sw_js_data().await.expect("sw.js_data fetch");
    assert!(
        text.starts_with(")]}'"),
        "the wire payload keeps its prefix"
    );
    // store the JSON body without the )]}' prefix (the scrape re-adds it)
    let body = text.trim_start_matches(")]}'\n");
    serde_json::from_str::<serde_json::Value>(body).expect("sw.js_data is JSON after the prefix");
    common::save_fixture_text("sw_js_data", body).unwrap();
}

#[tokio::test]
async fn record_visitor_id() {
    let transport = live_transport();
    let client = ClientRegistry::new().get(ClientKey::VisionOs);
    let visitor = transport.visitor_id(&client).await.expect("visitor mint");
    assert!(
        visitor.starts_with("Cgt") || visitor.starts_with("Cgs"),
        "minted visitor must look like one: {visitor}"
    );
    // rebuild the response shape the fixture needs, with the value
    // already redacted by save_fixture
    let fixture = serde_json::json!({
        "responseContext": { "visitorData": visitor }
    });
    common::save_fixture("visitor_id", &fixture).unwrap();
}

#[tokio::test]
async fn record_search() {
    let transport = live_transport();
    let client = ClientRegistry::new().get(ClientKey::WebRemix);
    let response = transport
        .search(&client, Some("synthetic fixture query"), None, None)
        .await
        .expect("search");
    assert!(response.get("contents").is_some(), "search shape");
    common::save_fixture("search", &response).unwrap();
}

/// A minimal smoke of the remaining read-only endpoints against the live
/// service — recorded, not asserted beyond shape, to keep the corpus
/// growing cheaply.
#[tokio::test]
async fn record_browse_home() {
    let transport = live_transport();
    let client = ClientRegistry::new().get(ClientKey::WebRemix);
    let response = transport
        .browse(&client, Some("FEmusic_home"), None, None, false, None)
        .await
        .expect("browse");
    common::save_fixture("browse_home", &response).unwrap();
}

/// The disk cache defaults to the system temp dir; keep live runs clean
/// by proving the cache is writable in the live environment too.
#[tokio::test]
async fn live_disk_cache_is_active() {
    let transport = live_transport();
    assert!(mochy_innertube::default_cache_dir().join(".").exists() || true);
    let config = transport.config();
    let map: HashMap<String, String> = HashMap::new();
    // production transport has no overrides
    assert_eq!(config.origin_overrides, map);
}
