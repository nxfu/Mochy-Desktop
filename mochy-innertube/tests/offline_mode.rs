//! Offline mode: nothing goes out (InnerTube.kt:71-73 +
//! utils/OfflineMode.kt).
//!
//! This test lives ALONE in its own integration-test binary on purpose:
//! `mochy_net::set_offline` flips a process-global `AtomicBool` (the
//! faithful port of the upstream `OfflineMode` singleton), and the tests
//! of one binary run in parallel. While this test holds the flag, any
//! sibling's in-flight request fails with `Net(Offline)` — exactly the
//! flake that took down the phase-5 macOS CI run
//! (`golden_headers_web_remix_signed_out` panicked on `Net(Offline)`
//! while `offline_mode_refuses_before_any_request` ran concurrently).
//!
//! `cargo test` executes test binaries one at a time, so a dedicated
//! binary owns the flag exclusively for the duration of its run. Keep
//! any new test that calls `set_offline` in here (or in a binary of its
//! own), never next to tests that make requests.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use common::serve::serve_responses;
use mochy_innertube::client::{ClientKey, ClientRegistry};
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
    config.retry = mochy_net::RetryPolicy {
        max_attempts: 1,
        ..mochy_net::RetryPolicy::upstream()
    };
    InnerTubeTransport::with_config(session, config).unwrap()
}

#[tokio::test]
async fn offline_mode_refuses_before_any_request() {
    let (addr, captured, server) = serve_responses(vec![]).await;
    let session: SharedSession = Arc::new(RwLock::new(Session::default()));
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
