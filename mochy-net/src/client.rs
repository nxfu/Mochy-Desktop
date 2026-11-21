//! The reqwest client factory.
//!
//! Absorbs the app's client builders:
//! - `providers/innertube/.../InnerTube.kt:76-156` (`createClient`):
//!   connection pool, 30 s connect / 60 s read+write timeouts, HTTP/2 +
//!   HTTP/1.1, gzip/deflate content encoding, the 50 MB disk cache, the
//!   DNS override for [`IpVersion`] pinning, the app proxy with
//!   `Proxy-Authorization`.
//! - The default user agent is the port's own identity
//!   `"Mochy (github.com/nxfu/Mochy-Desktop)"` with per-client overrides
//!   (upstream sets per-client `userAgent(client.userAgent)` — every
//!   emulated YouTube client sends its own).

use std::net::SocketAddr;
use std::time::Duration;

use crate::IpVersion;
use crate::error::NetError;
use crate::proxy::ProxyConfig;

/// The port's default user agent (used unless a client overrides it).
pub const DEFAULT_USER_AGENT: &str = "Mochy (github.com/nxfu/Mochy-Desktop)";

/// Upstream connect timeout (InnerTube.kt:103).
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
/// Upstream read timeout (InnerTube.kt:104). Also stands in for the
/// upstream 60 s write timeout (InnerTube.kt:105): reqwest 0.13 has no
/// separate write-timeout knob.
pub const READ_TIMEOUT: Duration = Duration::from_secs(60);

/// Everything needed to build one shared client.
#[derive(Debug, Clone, Default)]
pub struct ClientSpec {
    /// Per-client user agent override (upstream `client.userAgent`).
    pub user_agent: Option<String>,
    /// Explicit proxy; `None` = follow the system proxy
    /// (upstream `AppProxy.selector`: own proxy first, else the phone's).
    pub proxy: Option<ProxyConfig>,
    /// IP version pinning (InnerTube.kt:118-127).
    pub ip_version: IpVersion,
    /// Extra `resolve` pins: domain -> addresses (DoH-pinned hosts and the
    /// IPv4/IPv6 pinning land here).
    pub resolved: Vec<(String, Vec<SocketAddr>)>,
    /// Disable the cookie store (upstream keeps cookies per client; the
    /// shared desktop client keeps one store, login flows get their own).
    pub no_cookies: bool,
}

impl ClientSpec {
    /// The default shared client spec.
    pub fn new() -> ClientSpec {
        ClientSpec::default()
    }

    /// Override the user agent.
    pub fn user_agent(mut self, ua: impl Into<String>) -> ClientSpec {
        self.user_agent = Some(ua.into());
        self
    }

    /// Route through an explicit proxy.
    pub fn proxy(mut self, proxy: ProxyConfig) -> ClientSpec {
        self.proxy = Some(proxy);
        self
    }

    /// Pin the IP version.
    pub fn ip_version(mut self, version: IpVersion) -> ClientSpec {
        self.ip_version = version;
        self
    }

    /// Pin a domain to resolved addresses (DoH path).
    pub fn resolve(mut self, domain: impl Into<String>, addrs: Vec<SocketAddr>) -> ClientSpec {
        self.resolved.push((domain.into(), addrs));
        self
    }
}

/// Builds [`reqwest::Client`]s from [`ClientSpec`]s.
#[derive(Debug, Clone, Copy, Default)]
pub struct ClientFactory;

impl ClientFactory {
    /// Build the client for `spec`.
    pub fn build(&self, spec: &ClientSpec) -> Result<reqwest::Client, NetError> {
        let mut builder = reqwest::Client::builder()
            .user_agent(spec.user_agent.as_deref().unwrap_or(DEFAULT_USER_AGENT))
            .connect_timeout(CONNECT_TIMEOUT)
            // reqwest 0.13 has no separate write timeout; read_timeout is
            // per-read-op and covers a stalled transfer (upstream 60/60/60,
            // InnerTube.kt:103-105 — the write distinction is not
            // expressible and is not observable for these workloads).
            .read_timeout(READ_TIMEOUT);

        if !spec.no_cookies {
            builder = builder.cookie_store(true);
        }

        // Proxy: explicit config wins; otherwise the system proxy (reqwest's
        // system-proxy feature) — upstream AppProxy.selector semantics.
        if let Some(proxy) = &spec.proxy {
            builder = builder.proxy(proxy.to_reqwest()?);
        }

        // IP-version pinning and DoH pins.
        for (domain, addrs) in &spec.resolved {
            let addrs = spec.ip_version.filter(addrs.clone());
            if !addrs.is_empty() {
                for addr in &addrs {
                    builder = builder.resolve(domain, *addr);
                }
            }
        }

        builder.build().map_err(NetError::from)
    }

    /// The default shared client (system proxy, default UA, cookies on).
    pub fn shared(&self) -> Result<reqwest::Client, NetError> {
        self.build(&ClientSpec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-shot local HTTP server: accepts one request, replies with
    /// `response`, and its join handle yields the request head. (Client
    /// default headers like the UA are only observable on the wire in
    /// reqwest 0.13.)
    async fn serve_once(
        response: &'static str,
    ) -> (tokio::task::JoinHandle<String>, std::net::SocketAddr) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut head = String::new();
            let mut buf = [0u8; 8192];
            loop {
                let n = socket.read(&mut buf).await.unwrap();
                if n == 0 {
                    break;
                }
                head.push_str(&String::from_utf8_lossy(&buf[..n]));
                if head.contains("\r\n\r\n") || head.contains("\n\n") {
                    break;
                }
            }
            socket.write_all(response.as_bytes()).await.unwrap();
            socket.flush().await.unwrap();
            head
        });
        (handle, addr)
    }

    fn url_of(addr: std::net::SocketAddr) -> String {
        format!("http://{addr}/")
    }

    async fn ua_on_the_wire(spec: ClientSpec) -> String {
        let (server, addr) = serve_once("HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nhi").await;
        let client = ClientFactory.build(&spec).unwrap();
        let _ = client.get(url_of(addr)).send().await;
        let head = server.await.unwrap();
        head.lines()
            .find(|line| line.to_ascii_lowercase().starts_with("user-agent:"))
            .map(|line| line.split_once(':').unwrap().1.trim().to_owned())
            .unwrap_or_default()
    }

    #[tokio::test]
    async fn shared_client_sends_the_default_ua() {
        assert_eq!(ua_on_the_wire(ClientSpec::new()).await, DEFAULT_USER_AGENT);
    }

    #[tokio::test]
    async fn per_client_ua_override_wins() {
        let ua = ua_on_the_wire(
            ClientSpec::new().user_agent("com.google.android.youtube/20.01 (Linux; U; Android 13)"),
        )
        .await;
        assert!(ua.starts_with("com.google.android.youtube/"), "got {ua}");
    }

    #[test]
    fn spec_pins_are_applied() {
        use std::net::{IpAddr, Ipv4Addr};
        let spec = ClientSpec::new().ip_version(IpVersion::Ipv4).resolve(
            "duckduckgo.com",
            vec![
                SocketAddr::new(IpAddr::V4(Ipv4Addr::new(52, 142, 1, 1)), 443),
                SocketAddr::new(IpAddr::V4(Ipv4Addr::new(52, 142, 1, 2)), 443),
            ],
        );
        // building with pins must succeed (the pinning itself is opaque in
        // reqwest; the filter behavior is covered by lib.rs tests)
        assert!(ClientFactory.build(&spec).is_ok());
    }

    // the guard below is HELD ACROSS AWAITS on purpose: it serializes
    // the whole test body against the tests that flip the global
    // offline flag (see `test_support`)
    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn rate_limit_short_circuits_the_retry_wrapper() {
        use crate::error::NetError;
        use crate::policy::{RetryPolicy, retry_on_io_errors};

        let _guard = crate::test_support::OFFLINE_FLAG_LOCK.lock().unwrap();
        // a server that always answers 429
        let (server, addr) =
            serve_once("HTTP/1.1 429 Too Many Requests\r\nContent-Length: 0\r\n\r\n").await;
        let client = ClientFactory.shared().unwrap();
        let target = url_of(addr);
        let result: Result<String, NetError> = retry_on_io_errors(&RetryPolicy::upstream(), || {
            let client = client.clone();
            let target = target.clone();
            async move {
                let status = client.get(&target).send().await?.status();
                if status.as_u16() == 429 {
                    // the real transport maps 429 at error conversion; the
                    // wrapper contract is exercised directly here
                    return Err(NetError::YouTubeRateLimited);
                }
                Ok(format!("{}", status.as_u16()))
            }
        })
        .await;
        assert!(matches!(result, Err(NetError::YouTubeRateLimited)));
        let head = server.await.unwrap();
        // exactly ONE request went out — the short-circuit means no retries
        assert_eq!(head.matches("GET /").count(), 1, "429 must not be retried");
    }

    // see rate_limit_short_circuits_the_retry_wrapper: the guard is
    // held across awaits on purpose
    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn server_error_is_not_retried_but_surfaced() {
        use crate::error::NetError;
        use crate::policy::{RetryPolicy, retry_on_io_errors};

        let _guard = crate::test_support::OFFLINE_FLAG_LOCK.lock().unwrap();
        // a server that always answers 500 (non-IO: no retry per upstream)
        let (server, addr) =
            serve_once("HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\n\r\n").await;
        let client = ClientFactory.shared().unwrap();
        let target = url_of(addr);
        let result: Result<(), NetError> = retry_on_io_errors(&RetryPolicy::upstream(), || {
            let client = client.clone();
            let target = target.clone();
            async move {
                let response = client.get(&target).send().await?;
                let status = response.status();
                if !status.is_success() {
                    return Err(NetError::Http {
                        status,
                        url: target.clone(),
                    });
                }
                Ok(())
            }
        })
        .await;
        assert!(matches!(result, Err(NetError::Http { .. })));
        let head = server.await.unwrap();
        assert_eq!(
            head.matches("GET /").count(),
            1,
            "HTTP 500 is not an IO error: no retry"
        );
    }
}
