//! # mochy-net — the shared HTTP layer
//!
//! reqwest factory with proxy/DNS/IP-version policy, response cache, offline
//! flag.
//!
//! Upstream absorption (Android, pin `bde01cb4`):
//! - `providers/innertube/.../InnerTube.kt:76-156` (`createClient`):
//!   connection pool, 30 s connect / 60 s read+write timeouts, HTTP/2 +
//!   HTTP/1.1, a 50 MB disk response cache, system-DNS with an
//!   [`IpVersion`] filter, and the app proxy with `Proxy-Authorization`.
//! - `providers/innertube/.../InnerTube.kt:204-223` (`withRetry`):
//!   3-attempt exponential backoff on IO errors only, checked before every
//!   call against the offline flag.
//! - `utils/OfflineMode.kt` + `App.kt:187-235`: a global offline flag set at
//!   startup and on every change of the setting, consulted by every request.
//! - `utils/AppProxy.kt`: explicit SOCKS/HTTP proxy with optional basic
//!   auth; `parseAddress` skips a pasted scheme or login in front of the
//!   `host:port`.
//! - `utils/YTPlayerUtils.kt:1216-1233`: HTTP 429 becomes a typed
//!   `YouTubeRateLimited` error that short-circuits callers — "nothing else
//!   is asked; every client goes to the same host, and more requests only
//!   keep the limit in place".
//! - `providers/musixmatch/.../Musixmatch.kt:56-67` (`searchDns`): DNS over
//!   HTTPS via Cloudflare (`https://1.1.1.1/dns-query`, IPv4 only) for the
//!   search hostname, system DNS for everything else, system fallback when
//!   DoH fails.
//!
//! Layering: infrastructure crate — async (tokio) APIs are allowed here
//! (PORT_SPEC §3.1 rule 1 exempts unavoidable cases; the app's dedicated
//! tokio runtime owns networking per rule 4).

mod cache;
mod client;
mod doh;
mod error;
mod offline;
mod policy;
mod proxy;

pub use cache::{ByteRange, DiskCache, DiskCacheError, INNERTUBE_DISK_CACHE_BYTES, ResponseCache};
pub use client::{ClientFactory, ClientSpec, DEFAULT_USER_AGENT};
pub use doh::{CLOUDFLARE_DOH_URL, DohResolver};
pub use error::NetError;
pub use offline::{OfflineFlag, set_offline};
pub use policy::{RetryDecision, RetryPolicy, retry_on_io_errors};
pub use proxy::{ProxyConfig, ProxyKind};

/// Serializes the unit tests that flip or read the process-global offline
/// flag.
///
/// The flag is deliberately global (the faithful port of the upstream
/// `OfflineMode` singleton), but the tests of this crate's single test
/// binary run in parallel: a test that flips the flag while another is
/// inside `retry_on_io_errors` makes that sibling fail with `Net(Offline)`.
/// That exact race took down the phase-5 macOS CI run in the sibling
/// `mochy-innertube` crate (see `tests/offline_mode.rs` there). Every test
/// that calls `set_offline` or executes `retry_on_io_errors` takes this
/// lock for its whole body.
#[cfg(test)]
pub(crate) mod test_support {
    pub static OFFLINE_FLAG_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
}

/// IP version preference (providers/innertube `IpVersion`; read through the
/// `ipVersion` preference). `Auto` uses whatever the resolver returns;
/// `Ipv4`/`Ipv6` filter the resolver's answers, falling back to the full
/// list when the filter would be empty — upstream InnerTube.kt:118-127.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IpVersion {
    #[default]
    Auto,
    Ipv4,
    Ipv6,
}

impl IpVersion {
    /// Filter `addrs` per the upstream DNS override
    /// (InnerTube.kt:121-124: `addresses.filter { ... }.ifEmpty { addresses }`).
    pub fn filter(self, addrs: Vec<std::net::SocketAddr>) -> Vec<std::net::SocketAddr> {
        match self {
            IpVersion::Auto => addrs,
            IpVersion::Ipv4 => {
                let v4: Vec<_> = addrs.iter().copied().filter(|a| a.is_ipv4()).collect();
                if v4.is_empty() { addrs } else { v4 }
            }
            IpVersion::Ipv6 => {
                let v6: Vec<_> = addrs.iter().copied().filter(|a| a.is_ipv6()).collect();
                if v6.is_empty() { addrs } else { v6 }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

    fn v4(p: u16) -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), p)
    }
    fn v6(p: u16) -> SocketAddr {
        SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), p)
    }

    #[test]
    fn ip_version_filters_with_empty_fallback() {
        let both = vec![v4(1), v6(2), v4(3)];
        assert_eq!(IpVersion::Auto.filter(both.clone()).len(), 3);
        assert_eq!(IpVersion::Ipv4.filter(both.clone()), vec![v4(1), v4(3)]);
        assert_eq!(IpVersion::Ipv6.filter(both.clone()), vec![v6(2)]);
        // empty filter falls back to everything (upstream ifEmpty)
        let only_v4 = vec![v4(9)];
        assert_eq!(IpVersion::Ipv6.filter(only_v4.clone()), only_v4);
    }
}
