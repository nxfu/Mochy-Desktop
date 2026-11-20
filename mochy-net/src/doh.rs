//! DNS over HTTPS for the Musixmatch path
//! (providers/musixmatch/.../Musixmatch.kt:56-67 `searchDns`):
//!
//! ```kotlin
//! val overHttps = DnsOverHttps.Builder()
//!     .url("https://1.1.1.1/dns-query".toHttpUrl())
//!     .includeIPv6(false)
//!     .build()
//! // indonesian isps send duckduckgo to a block page; cloudflare's dns
//! // over https still knows where it really is
//! if (!hostname.endsWith("duckduckgo.com")) Dns.SYSTEM.lookup(hostname)
//! else runCatching { overHttps.lookup(hostname) }
//!     .getOrElse { Dns.SYSTEM.lookup(hostname) }
//! ```
//!
//! Desktop port: a `hickory-resolver` configured with a single DoH
//! nameserver (Cloudflare 1.1.1.1, IPv4 answers only — upstream
//! `includeIPv6(false)`), used for the routed hostnames with system-DNS
//! fallback, exactly like the okhttp Dns override.

use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;

use hickory_resolver::TokioResolver;
use hickory_resolver::config::{NameServerConfig, ResolverConfig};

/// Cloudflare's DoH endpoint, as pinned upstream.
pub const CLOUDFLARE_DOH_URL: &str = "https://1.1.1.1/dns-query";

/// A DoH resolver with a per-hostname routing table:
/// routed suffixes resolve through DoH (IPv4 only), everything else and any
/// DoH failure falls back to the system resolver.
#[derive(Debug, Clone)]
pub struct DohResolver {
    doh: Arc<TokioResolver>,
    system: Arc<TokioResolver>,
    /// Hostname suffixes routed through DoH (upstream: duckduckgo.com).
    routed_suffixes: Vec<String>,
}

impl DohResolver {
    /// The upstream Musixmatch configuration: Cloudflare DoH, IPv4 answers,
    /// routing only `duckduckgo.com`.
    pub async fn musixmatch() -> Result<DohResolver, crate::error::NetError> {
        Self::new(CLOUDFLARE_DOH_URL, &["duckduckgo.com"]).await
    }

    /// Build a DoH resolver for `doh_url` routing `suffixes`.
    /// `doh_url` must be `https://<ip-or-host>/[path]`; a bare IP is used
    /// directly as the nameserver address, a hostname is resolved through
    /// the system resolver first (bootstrap). Async because the bootstrap
    /// lookup is.
    pub async fn new(
        doh_url: &str,
        suffixes: &[&str],
    ) -> Result<DohResolver, crate::error::NetError> {
        let (host, port, path) = parse_doh_url(doh_url)
            .ok_or_else(|| crate::error::NetError::Other(format!("bad DoH url: {doh_url}")))?;

        // system resolver for fallback + bootstrap
        let system = Arc::new(build_system_resolver()?);

        // the DoH nameserver itself must be reachable over IPv4
        // (upstream pins the 1.1.1.1 literal)
        let ns_ip = nameserver_v4(&host, &system).await?;
        let server_name: Arc<str> = Arc::from(format!("{host}:{port}"));

        let config = ResolverConfig::from_name_servers(vec![NameServerConfig::https(
            IpAddr::V4(ns_ip),
            server_name,
            path,
        )]);
        let doh = TokioResolver::builder_with_config(config, Default::default())
            .build()
            .map_err(|e| crate::error::NetError::Other(e.to_string()))?;

        Ok(DohResolver {
            doh: Arc::new(doh),
            system,
            routed_suffixes: suffixes.iter().map(|s| s.to_ascii_lowercase()).collect(),
        })
    }

    /// Whether `hostname` routes through DoH (upstream:
    /// `hostname.endsWith("duckduckgo.com")`).
    pub fn routes(&self, hostname: &str) -> bool {
        let hostname = hostname.to_ascii_lowercase();
        self.routed_suffixes
            .iter()
            .any(|suffix| hostname == *suffix || hostname.ends_with(&format!(".{suffix}")))
    }

    /// Resolve `hostname` per the routing table: DoH (IPv4 answers only —
    /// upstream `includeIPv6(false)`) for routed hosts, system DNS for the
    /// rest; DoH failure falls back to the system resolver
    /// (`runCatching ... getOrElse { Dns.SYSTEM.lookup }`).
    pub async fn lookup(&self, hostname: &str) -> Result<Vec<IpAddr>, crate::error::NetError> {
        if !self.routes(hostname) {
            return self.system_lookup(hostname).await;
        }
        match self.doh.lookup_ip(hostname).await {
            Ok(lookup) => {
                let v4: Vec<IpAddr> = lookup.iter().filter(|ip| ip.is_ipv4()).collect();
                if v4.is_empty() {
                    self.system_lookup(hostname).await
                } else {
                    Ok(v4)
                }
            }
            Err(_) => self.system_lookup(hostname).await,
        }
    }

    async fn system_lookup(&self, hostname: &str) -> Result<Vec<IpAddr>, crate::error::NetError> {
        self.system
            .lookup_ip(hostname)
            .await
            .map(|lookup| lookup.iter().collect())
            .map_err(|e| crate::error::NetError::Other(e.to_string()))
    }
}

fn build_system_resolver() -> Result<TokioResolver, crate::error::NetError> {
    TokioResolver::builder_tokio()
        .map_err(|e| crate::error::NetError::Other(e.to_string()))?
        .build()
        .map_err(|e| crate::error::NetError::Other(e.to_string()))
}

/// `(host, port, path)` of a DoH URL; the host may be an IP literal or a
/// name (bootstrapped through the system resolver).
fn parse_doh_url(url: &str) -> Option<(String, u16, Option<Arc<str>>)> {
    let rest = url.strip_prefix("https://")?;
    let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
    let authority = authority.rsplit_once('@').map_or(authority, |(_, a)| a);
    let (host, port) = if let Some(stripped) = authority.strip_prefix('[') {
        let (host, rest) = stripped.split_once(']')?;
        let port = rest.strip_prefix(':').and_then(|p| p.parse::<u16>().ok());
        (host, port)
    } else {
        match authority.rsplit_once(':') {
            Some((host, port)) => (host, port.parse::<u16>().ok()),
            None => (authority, None),
        }
    };
    if host.is_empty() {
        return None;
    }
    let port = port.unwrap_or(443);
    let path: Option<Arc<str>> = if path.is_empty() {
        None
    } else {
        Some(Arc::from(format!("/{path}")))
    };
    Some((host.to_owned(), port, path))
}

/// Resolve the DoH nameserver host to an IPv4 address (literal first).
async fn nameserver_v4(
    host: &str,
    system: &TokioResolver,
) -> Result<Ipv4Addr, crate::error::NetError> {
    if let Ok(IpAddr::V4(literal)) = host.parse::<IpAddr>() {
        return Ok(literal);
    }
    let lookup = system
        .lookup_ip(host)
        .await
        .map_err(|e| crate::error::NetError::Other(e.to_string()))?;
    lookup
        .iter()
        .find_map(|ip| match ip {
            IpAddr::V4(v4) => Some(v4),
            _ => None,
        })
        .ok_or_else(|| {
            crate::error::NetError::Other(format!("DoH nameserver {host} has no IPv4 address"))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routing_matches_upstream_suffix_rule() {
        let resolver = DohResolver {
            doh: panic_free_resolver(),
            system: panic_free_resolver(),
            routed_suffixes: vec!["duckduckgo.com".to_owned()],
        };
        assert!(resolver.routes("duckduckgo.com"));
        assert!(resolver.routes("html.duckduckgo.com"));
        assert!(resolver.routes("HTML.DuckDuckGo.com"));
        assert!(!resolver.routes("duckduckgo.com.evil.example"));
        assert!(!resolver.routes("notduckduckgo.com"));
        assert!(!resolver.routes("musixmatch.com"));
    }

    #[test]
    fn doh_url_parses() {
        let (host, port, path) = parse_doh_url("https://1.1.1.1/dns-query").unwrap();
        assert_eq!(host, "1.1.1.1");
        assert_eq!(port, 443);
        assert_eq!(path.as_deref(), Some("/dns-query"));

        let (host, port, path) = parse_doh_url("https://one.one.one.one/dns-query").unwrap();
        assert_eq!(host, "one.one.one.one");
        assert_eq!(port, 443);
        assert_eq!(path.as_deref(), Some("/dns-query"));

        let (host, port, path) = parse_doh_url("https://1.1.1.1:8443/other").unwrap();
        assert_eq!(host, "1.1.1.1");
        assert_eq!(port, 8443);
        assert_eq!(path.as_deref(), Some("/other"));

        // no path -> None path
        let (_, _, path) = parse_doh_url("https://1.1.1.1").unwrap();
        assert!(path.is_none());

        assert!(parse_doh_url("http://1.1.1.1/dns-query").is_none());
        assert!(parse_doh_url("https:///dns-query").is_none());
    }

    #[tokio::test]
    async fn nameserver_literal_short_circuits() {
        let system = build_system_resolver().unwrap();
        let ip = nameserver_v4("1.1.1.1", &system).await.unwrap();
        assert_eq!(ip, Ipv4Addr::new(1, 1, 1, 1));
    }

    fn panic_free_resolver() -> Arc<TokioResolver> {
        // an unstarted config is fine: the routing tests never resolve
        Arc::new(
            TokioResolver::builder_with_config(ResolverConfig::default(), Default::default())
                .build()
                .expect("empty resolver config builds"),
        )
    }
}
