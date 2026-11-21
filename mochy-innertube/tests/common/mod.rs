//! The fixture harness — record/replay for innertube responses
//! (blueprint ch. 15: "innertube contract tests over recorded redacted
//! JSON").
//!
//! Offline by default: tests read `tests/fixtures/<name>.json` and skip
//! (with a visible message) when a fixture is absent. Under
//! `cargo test --features live` (tests/live.rs) the same fixtures are
//! RECORDED from real music.youtube.com round trips and redacted by
//! [`redact`] before writing — never in CI (the CI matrix runs plain
//! `cargo test --workspace`).
//!
//! The committed fixtures are SYNTHETIC: structurally faithful minimal
//! payloads hand-built from the upstream parser expectations (the field
//! paths each ported parser walks), containing no user data. They exist
//! so the offline suite exercises the full transport path; `--features
//! live` replaces them with real captures. See `tests/fixtures/README.md`.

#![allow(dead_code)]

use std::path::PathBuf;

use serde_json::Value as Json;

pub fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// Replay a fixture by name; `None` when it has not been recorded yet.
pub fn load_fixture(name: &str) -> Option<Json> {
    let path = fixtures_dir().join(format!("{name}.json"));
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Replay a fixture's raw text (for non-JSON-top-level payloads like
/// sw.js_data).
pub fn load_fixture_text(name: &str) -> Option<String> {
    let path = fixtures_dir().join(format!("{name}.json"));
    std::fs::read_to_string(path).ok()
}

/// Record a fixture (live tests only). Redacted before writing.
pub fn save_fixture(name: &str, value: &Json) -> std::io::Result<()> {
    std::fs::create_dir_all(fixtures_dir())?;
    let redacted = redact(value);
    std::fs::write(
        fixtures_dir().join(format!("{name}.json")),
        serde_json::to_string_pretty(&redacted)?,
    )
}

/// Record a raw-text fixture (sw.js_data) after redaction.
pub fn save_fixture_text(name: &str, text: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(fixtures_dir())?;
    // try JSON redaction; fall back to writing the text as-is (the
    // sw.js_data prefix is re-added by the recorder caller)
    if let Ok(value) = serde_json::from_str::<Json>(text.trim_start_matches(")]}'\n")) {
        return save_fixture(name, &value);
    }
    std::fs::write(fixtures_dir().join(format!("{name}.json")), text)
}

/// The sensitive keys whose VALUES must never land in a committed
/// fixture: session/account identifiers, tokens and tracking params. The
/// KEY STRUCTURE stays (parsers depend on it); values are replaced.
const REDACT_KEYS: &[&str] = &[
    "visitorData",
    "onBehalfOfUser",
    "poToken",
    "clickTrackingParams",
    "trackingParams",
    "cpn",
    "authorization",
    "cookie",
    "X-Goog-Visitor-Id",
];

/// Recursively redact a recorded response: known-sensitive keys get a
/// placeholder value of the same JSON type, so every parser path stays
/// exercisable while no real identifier ships in the repo.
pub fn redact(value: &Json) -> Json {
    match value {
        Json::Object(map) => {
            let mut out = serde_json::Map::new();
            for (key, item) in map {
                if REDACT_KEYS.iter().any(|k| key.eq_ignore_ascii_case(k)) {
                    out.insert(
                        key.clone(),
                        Json::String(format!("<redacted:{}>", key.to_lowercase())),
                    );
                } else {
                    out.insert(key.clone(), redact(item));
                }
            }
            Json::Object(out)
        }
        Json::Array(items) => Json::Array(items.iter().map(redact).collect()),
        other => other.clone(),
    }
}

/// Assert-or-skip helper for offline replay tests: yields the fixture,
/// or `None` (with a visible skip note) when it has not been recorded.
pub fn fixture_or_skip(name: &str) -> Option<Json> {
    match load_fixture(name) {
        Some(value) => Some(value),
        None => {
            eprintln!(
                "SKIP: fixture '{name}' not recorded yet — run \
                 `cargo test -p mochy-innertube --features live --test live` \
                 to record it from the real service"
            );
            None
        }
    }
}

/// A local test HTTP server that serves N canned responses and records
/// every request head (method, path+query, headers). The innertube
/// transport is pointed at it through `origin_overrides`, which is the
/// only way header injection is observable on the wire.
pub mod serve {
    use std::sync::{Arc, Mutex};

    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// One captured request.
    #[derive(Debug, Clone)]
    pub struct CapturedRequest {
        pub head: String,
    }

    impl CapturedRequest {
        /// The request line: `METHOD /path?query HTTP/1.1`.
        pub fn request_line(&self) -> &str {
            self.head.lines().next().unwrap_or_default()
        }

        /// The path without the query string.
        pub fn path(&self) -> String {
            self.request_line()
                .split_whitespace()
                .nth(1)
                .unwrap_or_default()
                .split('?')
                .next()
                .unwrap_or_default()
                .to_owned()
        }

        /// One header value, case-insensitive (None when absent).
        pub fn header(&self, name: &str) -> Option<String> {
            self.head.lines().find_map(|line| {
                let (key, value) = line.split_once(':')?;
                if key.trim().eq_ignore_ascii_case(name) {
                    Some(value.trim().to_owned())
                } else {
                    None
                }
            })
        }

        /// All values of a repeated header.
        pub fn headers(&self, name: &str) -> Vec<String> {
            self.head
                .lines()
                .filter_map(|line| {
                    let (key, value) = line.split_once(':')?;
                    if key.trim().eq_ignore_ascii_case(name) {
                        Some(value.trim().to_owned())
                    } else {
                        None
                    }
                })
                .collect()
        }

        /// The decoded query parameters.
        pub fn query_pairs(&self) -> Vec<(String, String)> {
            let line = self.request_line();
            let target = line.split_whitespace().nth(1).unwrap_or_default();
            match target.split_once('?') {
                Some((_, query)) => query
                    .split('&')
                    .filter(|part| !part.is_empty())
                    .map(|pair| match pair.split_once('=') {
                        Some((k, v)) => (urldecode(k), urldecode(v)),
                        None => (urldecode(pair), String::new()),
                    })
                    .collect(),
                None => Vec::new(),
            }
        }

        /// The query value of `name`.
        pub fn query(&self, name: &str) -> Option<String> {
            self.query_pairs()
                .into_iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v)
        }

        /// The request body (after the blank line), for POSTs.
        pub fn body(&self) -> String {
            match self.head.split_once("\r\n\r\n") {
                Some((_, body)) => body.to_owned(),
                None => String::new(),
            }
        }
    }

    fn urldecode(text: &str) -> String {
        let mut out = Vec::with_capacity(text.len());
        let bytes = text.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'%' => {
                    let hex = bytes
                        .get(i + 1..i + 3)
                        .and_then(|h| std::str::from_utf8(h).ok())
                        .and_then(|h| u8::from_str_radix(h, 16).ok());
                    match hex {
                        Some(byte) => {
                            out.push(byte);
                            i += 3;
                        }
                        None => {
                            out.push(bytes[i]);
                            i += 1;
                        }
                    }
                }
                b'+' => {
                    out.push(b' ');
                    i += 1;
                }
                byte => {
                    out.push(byte);
                    i += 1;
                }
            }
        }
        String::from_utf8_lossy(&out).into_owned()
    }

    /// Serve `responses.len()` requests, each answered with the
    /// corresponding canned response, then shut down. Returns the listen
    /// address and the captured requests.
    pub async fn serve_responses(
        responses: Vec<String>,
    ) -> (
        std::net::SocketAddr,
        Arc<Mutex<Vec<CapturedRequest>>>,
        tokio::task::JoinHandle<()>,
    ) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let captured: Arc<Mutex<Vec<CapturedRequest>>> = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&captured);
        let handle = tokio::spawn(async move {
            for response in responses {
                let (mut socket, _) = match listener.accept().await {
                    Ok(pair) => pair,
                    Err(_) => break,
                };
                let mut head = Vec::new();
                let mut buf = [0u8; 8192];
                // read the head, then exactly content-length body bytes
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
                let head_text = String::from_utf8_lossy(&head).into_owned();
                seen.lock()
                    .unwrap()
                    .push(CapturedRequest { head: head_text });
                socket.write_all(response.as_bytes()).await.unwrap();
                socket.flush().await.unwrap();
                let _ = socket.shutdown().await;
            }
        });
        (addr, captured, handle)
    }

    /// A canned 200 JSON response.
    pub fn json_response(body: &str) -> String {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        )
    }

    /// A canned 200 response with an extra header.
    pub fn json_response_with_header(body: &str, header: &str, value: &str) -> String {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n{header}: {value}\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        )
    }

    /// A 200 response with an empty JSON object (for videostats GETs).
    pub fn empty_json() -> String {
        json_response("{}")
    }
}
