//! The raw transport — the port of `providers/innertube/.../InnerTube.kt`
//! (pin `bde01cb4`, 836 lines) minus its endpoint methods (those live in
//! [`crate::endpoints`]) and minus the response parsers (Phase 05).
//!
//! What is absorbed here:
//! - the http client shape (`createClient`, InnerTube.kt:76-156):
//!   gzip/deflate, HTTP/2 + HTTP/1.1, 30 s connect / 60 s read timeouts,
//!   the 50 MB disk cache for GETs, IPv4/IPv6 pinning, the app proxy with
//!   auth — all through [`mochy_net`]'s factory, one level down;
//! - the per-request header injection (`ytClient`, InnerTube.kt:158-182):
//!   client name+version, per-client user agent, X-Origin/Referer for
//!   browser clients, visitor id, the cookie + origin-bound SAPISIDHASH
//!   Authorization for signed-in requests;
//! - the retry wrapper (`withRetry`, InnerTube.kt:204-223): 3 attempts,
//!   exponential backoff on IO errors only, offline checked before every
//!   attempt;
//! - 429 -> [`mochy_net::NetError::YouTubeRateLimited`] short-circuit
//!   (utils/YTPlayerUtils.kt:1229-1233);
//! - own-visitor-id minting for the clients that bring their own
//!   (InnerTube.kt:184-202) — VISIONOS's `visitor_id` round trip.

use std::path::PathBuf;
use std::sync::RwLock;
use std::time::{SystemTime, UNIX_EPOCH};

use mochy_net::{
    ClientFactory, ClientSpec, DiskCache, INNERTUBE_DISK_CACHE_BYTES, NetError, RetryPolicy,
    retry_on_io_errors,
};
use serde_json::Value as Json;

use crate::client::{API_URL_YOUTUBE_MUSIC, YouTubeClient};
use crate::error::InnertubeError;
use crate::session::{ClientSeed, Session, SharedSession};

/// The default disk-cache directory — upstream `java.io.tmpdir/http_cache`
/// (InnerTube.kt:113).
pub fn default_cache_dir() -> PathBuf {
    std::env::temp_dir().join("http_cache")
}

/// Transport configuration. The origin overrides exist so tests (and only
/// tests) can point music/www origins at a local server; production code
/// leaves the map empty and every URL is the real one.
#[derive(Debug, Clone)]
pub struct TransportConfig {
    /// Where the 50 MB GET disk cache lives
    /// (default [`default_cache_dir`]).
    pub cache_dir: PathBuf,
    /// Disable the disk cache entirely (tests).
    pub cache_enabled: bool,
    /// `real origin -> replacement origin`, applied to every request URL
    /// before it is sent. Empty in production.
    pub origin_overrides: std::collections::HashMap<String, String>,
    /// The retry policy (upstream defaults by construction).
    pub retry: RetryPolicy,
}

impl Default for TransportConfig {
    fn default() -> Self {
        TransportConfig {
            cache_dir: default_cache_dir(),
            cache_enabled: true,
            origin_overrides: std::collections::HashMap::new(),
            retry: RetryPolicy::upstream(),
        }
    }
}

/// The built client plus the session snapshot it was built from — compared
/// on every request so a proxy/ipVersion change rebuilds the client
/// (InnerTube.kt:53-67 setter side effects).
struct ActiveClient {
    seed: ClientSeed,
    client: reqwest::Client,
}

/// The InnerTube transport. Clone-safe: the session, client cell and cache
/// are shared.
#[derive(Clone)]
pub struct InnerTubeTransport {
    session: SharedSession,
    http: std::sync::Arc<RwLock<ActiveClient>>,
    disk_cache: Option<std::sync::Arc<DiskCache>>,
    config: TransportConfig,
    /// Response headers the last request was asked to capture (the
    /// resumable-upload handshake reads `x-guploader-uploadid` off the
    /// start call, InnerTube.kt:674-682). Sequential by construction —
    /// the handshake is one caller's two-step flow.
    captured: std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, String>>>,
}

impl InnerTubeTransport {
    /// Build the transport around `session` with production defaults.
    pub fn new(session: SharedSession) -> Result<Self, InnertubeError> {
        Self::with_config(session, TransportConfig::default())
    }

    /// Build with an explicit config (tests set `origin_overrides`).
    pub fn with_config(
        session: SharedSession,
        config: TransportConfig,
    ) -> Result<Self, InnertubeError> {
        let seed = Self::read_seed(&session);
        let client = build_client(&seed)?;
        let disk_cache = match config.cache_enabled {
            true => Some(std::sync::Arc::new(
                DiskCache::open(&config.cache_dir, INNERTUBE_DISK_CACHE_BYTES)
                    .map_err(InnertubeError::Cache)?,
            )),
            false => None,
        };
        Ok(InnerTubeTransport {
            session,
            http: std::sync::Arc::new(RwLock::new(ActiveClient { seed, client })),
            disk_cache,
            config,
            captured: std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
        })
    }

    /// The shared session (for the mirror and the facade).
    pub fn session(&self) -> &SharedSession {
        &self.session
    }

    fn read_seed(session: &SharedSession) -> ClientSeed {
        let guard = session
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        ClientSeed {
            proxy: guard.proxy.clone(),
            ip_version: guard.ip_version,
        }
    }

    /// Rebuild the http client when the session's proxy/ipVersion changed
    /// (InnerTube.kt:53-58 and 62-67: the setters close and recreate the
    /// client). Called before every request; a matching seed is a no-op.
    pub(crate) fn ensure_client(&self) -> Result<reqwest::Client, InnertubeError> {
        let seed = Self::read_seed(&self.session);
        let mut guard = self
            .http
            .write()
            .map_err(|_| InnertubeError::LockPoisoned)?;
        if guard.seed != seed {
            guard.client = build_client(&seed)?;
            guard.seed = seed;
        }
        Ok(guard.client.clone())
    }

    /// The transport config (tests read the overrides).
    pub fn config(&self) -> &TransportConfig {
        &self.config
    }

    /// The `Session` snapshot the endpoint layer reads for context
    /// building — one short lock, cloned out.
    pub(crate) fn session_snapshot(&self) -> Result<Session, InnertubeError> {
        self.session
            .read()
            .map(|guard| guard.clone())
            .map_err(|_| InnertubeError::LockPoisoned)
    }

    /// Apply the test origin overrides to `url`.
    pub(crate) fn rewrite(&self, url: String) -> String {
        if self.config.origin_overrides.is_empty() {
            return url;
        }
        let mut url = url;
        for (real, replacement) in &self.config.origin_overrides {
            if url.starts_with(real.as_str()) {
                url = url.replacen(real, replacement, 1);
                break;
            }
        }
        url
    }

    /// The per-request header injection — `InnerTube.kt:158-182`
    /// (`ytClient`), plus the client defaults of `createClient`
    /// (InnerTube.kt:149-155: Accept, Accept-Language, Cache-Control) and
    /// the `prettyPrint=false` parameter (InnerTube.kt:181).
    fn apply_client_headers(
        &self,
        builder: reqwest::RequestBuilder,
        client: &YouTubeClient,
        set_login: bool,
        visitor: Option<&str>,
    ) -> reqwest::RequestBuilder {
        let builder = builder
            .header("Accept", "application/json")
            .header("Accept-Language", "en-US,en;q=0.9")
            .header("Cache-Control", "no-cache")
            // InnerTube.kt:180 — userAgent(client.userAgent): every
            // emulated client sends its own
            .header("User-Agent", &client.user_agent)
            .header("X-Goog-Api-Format-Version", &client.api_format_version)
            .header("X-YouTube-Client-Name", &client.client_id)
            .header("X-YouTube-Client-Version", &client.client_version)
            .query(&[("prettyPrint", "false")]);

        // the native apps send no origin or referer, a browser-looking
        // header on an app client is one more thing that gives it away
        // (InnerTube.kt:164-167 via YouTubeClient.isNativeApp)
        let mut builder = builder;
        if !client.is_native_app() {
            builder = builder
                .header("X-Origin", &client.api_origin)
                .header("Referer", format!("{}/", client.api_origin));
        }
        if let Some(visitor) = visitor {
            builder = builder.header("X-Goog-Visitor-Id", visitor);
        }
        if set_login && client.login_supported {
            // InnerTube.kt:169-178 — cookie + the origin-bound SAPISIDHASH
            let (cookie, sapisid) = {
                let guard = self
                    .session
                    .read()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                (
                    guard.cookie.clone(),
                    guard.cookie_map.get("SAPISID").cloned(),
                )
            };
            if let Some(cookie) = cookie {
                builder = builder.header("cookie", &cookie);
                if let Some(sapisid) = sapisid {
                    let time = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0);
                    builder = builder.header(
                        "Authorization",
                        crate::auth::authorization_header(time, &sapisid, &client.api_origin),
                    );
                }
            }
        }
        builder
    }

    /// Send one shaped request under the retry wrapper — the
    /// `withRetry` port (InnerTube.kt:204-223) with the offline gate
    /// re-checked before every attempt and 429 mapped by [`NetError`].
    pub(crate) async fn send_json(&self, request: ShapedRequest) -> Result<Json, InnertubeError> {
        let this = self.clone();
        let value = retry_on_io_errors(&self.config.retry, move || {
            let this = this.clone();
            let request = request.clone();
            async move { this.attempt(&request).await }
        })
        .await?;
        Ok(value)
    }

    /// Send one shaped request, returning the raw body text (used by
    /// sw.js_data and any non-JSON endpoint).
    pub(crate) async fn send_text(&self, request: ShapedRequest) -> Result<String, InnertubeError> {
        let this = self.clone();
        retry_on_io_errors(&self.config.retry, move || {
            let this = this.clone();
            let request = request.clone();
            async move { this.attempt_text(&request).await }
        })
        .await
        .map_err(InnertubeError::from)
    }

    /// One attempt: GET -> disk cache check; POST -> straight out; the
    /// response status is checked `expectSuccess`-style (InnerTube.kt:77)
    /// with 429 mapped to the typed rate-limit error by [`NetError`].
    async fn attempt(&self, request: &ShapedRequest) -> Result<Json, NetError> {
        let text = self.attempt_text(request).await?;
        serde_json::from_str(&text).map_err(|e| NetError::Other(format!("invalid JSON body: {e}")))
    }

    async fn attempt_text(&self, request: &ShapedRequest) -> Result<String, NetError> {
        // the disk cache is a GET cache (upstream okhttp Cache semantics)
        let cache_key = (request.method.as_str(), request.url.as_str());
        if request.method == reqwest::Method::GET
            && let Some(cache) = &self.disk_cache
            && let Some(body) = cache.get(cache_key.0, cache_key.1)
        {
            return String::from_utf8(body)
                .map_err(|e| NetError::Other(format!("cached body not utf-8: {e}")));
        }

        let client = self
            .ensure_client()
            .map_err(|e| NetError::Other(e.to_string()))?;

        let url = self.rewrite(request.url.clone());
        let mut builder = client.request(request.method.clone(), &url);

        if !request.use_client_headers {
            // the raw requests (sw.js_data) still get the default headers
            builder = builder
                .header("Accept", "application/json")
                .header("Accept-Language", "en-US,en;q=0.9")
                .header("Cache-Control", "no-cache");
        } else {
            builder = self.apply_client_headers(
                builder,
                &request.client,
                request.set_login,
                request.visitor.as_deref(),
            );
        }

        for (name, value) in &request.headers {
            builder = builder.header(name, value);
        }
        if !request.query.is_empty() {
            builder = builder.query(&request.query);
        }
        if let Some(body) = &request.body {
            builder = builder.json(body);
        } else if request.json_content_type {
            builder = builder.header(reqwest::header::CONTENT_TYPE, "application/json");
        }

        let response = builder.send().await?;
        let status = response.status();
        if status.as_u16() == 429 {
            // utils/YTPlayerUtils.kt:1229-1233 — nothing else is asked
            return Err(NetError::YouTubeRateLimited);
        }
        if !status.is_success() {
            return Err(NetError::Http {
                status,
                url: request.url.clone(),
            });
        }
        let headers = response.headers().clone();
        let text = response.text().await?;

        if request.method == reqwest::Method::GET
            && request.cacheable
            && let Some(cache) = &self.disk_cache
            && let Err(error) = cache.put(cache_key.0, cache_key.1, text.as_bytes())
        {
            tracing::debug!(?error, "innertube GET disk-cache write failed");
        }
        // surface the upload-id style response headers the caller asked for
        for wanted in &request.capture_headers {
            if let Some(value) = headers.get(wanted.as_str())
                && let Ok(value) = value.to_str()
            {
                self.captured
                    .lock()
                    .map_err(|_| NetError::Other("capture lock poisoned".into()))?
                    .insert(wanted.clone(), value.to_owned());
            }
        }
        Ok(text)
    }

    /// Read (and clear) headers captured by the last request that asked
    /// for them (the resumable upload handshake's
    /// `x-guploader-uploadid`, InnerTube.kt:674-682).
    pub(crate) fn take_captured_headers(
        &self,
    ) -> Result<std::collections::HashMap<String, String>, InnertubeError> {
        self.captured
            .lock()
            .map(|mut map| std::mem::take(&mut *map))
            .map_err(|_| InnertubeError::LockPoisoned)
    }
}

/// Build the reqwest client for a session seed — the `createClient` port.
/// Per-request user agents are injected per client, so the shared client
/// keeps the default UA; gzip/brotli and HTTP/2 are reqwest features
/// enabled in the workspace dependency.
fn build_client(seed: &ClientSeed) -> Result<reqwest::Client, InnertubeError> {
    let mut spec = ClientSpec::new().ip_version(seed.ip_version);
    if let Some(proxy) = &seed.proxy {
        spec = spec.proxy(proxy.clone());
    }
    ClientFactory.build(&spec).map_err(InnertubeError::from)
}

/// One fully-shaped request — what every endpoint method builds. This is
/// the port's stand-in for the ktor `HttpRequestBuilder` closures: the
/// header injection is decided at build time (so it can be asserted in
/// tests without a server) and executed by [`InnerTubeTransport`].
#[derive(Debug, Clone)]
pub(crate) struct ShapedRequest {
    pub method: reqwest::Method,
    pub url: String,
    /// Apply the per-client header set (InnerTube.kt:158-182). `false`
    /// only for the raw requests (sw.js_data).
    pub use_client_headers: bool,
    pub client: YouTubeClient,
    pub set_login: bool,
    pub visitor: Option<String>,
    pub headers: Vec<(String, String)>,
    pub query: Vec<(String, String)>,
    pub body: Option<Json>,
    /// Send `Content-Type: application/json` with no body
    /// (InnerTube.kt:159 + 412-414).
    pub json_content_type: bool,
    /// Store the response in the GET disk cache.
    pub cacheable: bool,
    /// Response headers to capture for the caller.
    pub capture_headers: Vec<String>,
}

impl ShapedRequest {
    /// A POST to a relative innertube endpoint (joined onto
    /// `API_URL_YOUTUBE_MUSIC`) with the full client header set — the
    /// shape of every endpoint in InnerTube.kt except the absolute-URL
    /// ones.
    pub fn endpoint(
        method: reqwest::Method,
        endpoint: &str,
        client: &YouTubeClient,
        set_login: bool,
        visitor: Option<String>,
    ) -> ShapedRequest {
        ShapedRequest {
            method,
            url: format!("{API_URL_YOUTUBE_MUSIC}{endpoint}"),
            use_client_headers: true,
            client: client.clone(),
            set_login,
            visitor,
            headers: Vec::new(),
            query: Vec::new(),
            body: None,
            json_content_type: false,
            cacheable: false,
            capture_headers: Vec::new(),
        }
    }
}

/// The visitor resolution for `player` — InnerTube.kt:256: clients with
/// `ownVisitorId` use their minted id (falling back to a mint), everyone
/// else uses the session's `visitorData`.
pub(crate) async fn resolve_visitor(
    transport: &InnerTubeTransport,
    client: &YouTubeClient,
) -> Result<Option<String>, InnertubeError> {
    if !client.own_visitor_id {
        let guard = transport
            .session()
            .read()
            .map_err(|_| InnertubeError::LockPoisoned)?;
        return Ok(guard.visitor_data.clone());
    }
    // InnerTube.kt:184-189 — mint on first use; null when youtube
    // wouldn't give one, the request then goes out without
    let existing = {
        let guard = transport
            .session()
            .read()
            .map_err(|_| InnertubeError::LockPoisoned)?;
        guard.own_visitor_ids.get(&client.client_name).cloned()
    };
    if let Some(id) = existing {
        return Ok(Some(id));
    }
    match crate::endpoints::mint_visitor_id(transport, client).await {
        Ok(id) => Ok(Some(id)),
        Err(error) => {
            tracing::debug!(?error, client = %client.client_name, "own-visitor-id mint failed; sending without");
            Ok(None)
        }
    }
}
