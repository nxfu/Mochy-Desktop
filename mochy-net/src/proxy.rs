//! Proxy configuration (utils/AppProxy.kt).
//!
//! Upstream: `ProxyConfig(type, address, username, password)` with
//! `Proxy.Type.HTTP` default; `parseAddress` accepts
//! `"1.2.3.4:8080"`, `"proxy.example.com:1080"`, `"[2001:db8::1]:3128"`,
//! skipping a pasted scheme or login in front; HTTP auth rides the
//! `Proxy-Authorization` header (basic), SOCKS auth rides the JVM
//! authenticator (reqwest: `Proxy::basic_auth`, which applies to both).

use crate::proxy::ProxyKind::{Http, Socks};

/// Proxy protocol (upstream `Proxy.Type`; only HTTP + SOCKS exist there).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProxyKind {
    #[default]
    Http,
    Socks,
}

impl ProxyKind {
    /// The stored preference value (`Proxy.Type.name`).
    pub const fn as_str(self) -> &'static str {
        match self {
            Http => "HTTP",
            Socks => "SOCKS",
        }
    }

    pub fn from_pref(value: &str) -> ProxyKind {
        match value {
            "SOCKS" => Socks,
            _ => Http, // upstream toEnum(defaultValue = Proxy.Type.HTTP)
        }
    }
}

/// An explicit proxy with optional basic auth.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyConfig {
    pub kind: ProxyKind,
    /// Hostname or IP — kept unresolved like upstream
    /// (`InetSocketAddress.createUnresolved`, resolved at connect time).
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
}

impl ProxyConfig {
    /// Whether any credentials were supplied
    /// (upstream `hasLogin`: username OR password non-empty).
    pub fn has_login(&self) -> bool {
        !self.username.is_empty() || !self.password.is_empty()
    }

    /// Parse `"host:port"` / `"[v6]:port"`, skipping a pasted scheme or
    /// userinfo — the port of `AppProxy.parseAddress`
    /// (utils/AppProxy.kt:82-93). Returns `None` when no valid port is
    /// present or the host is blank.
    pub fn parse_address(text: &str) -> Option<(String, u16)> {
        let text = text.trim();
        let host_port = text.split_once("://").map_or(text, |(_, rest)| rest);
        let host_port = host_port
            .rsplit_once('@')
            .map_or(host_port, |(_, rest)| rest);
        let host_port = host_port.trim_end_matches('/');
        let (host, port) = host_port.rsplit_once(':')?;
        let port: u16 = port.parse().ok().filter(|p| (1..=65535).contains(p))?;
        let host = if let Some(stripped) = host.strip_prefix('[') {
            stripped.split_once(']')?.0
        } else {
            host
        };
        if host.is_empty() {
            return None;
        }
        Some((host.to_owned(), port))
    }

    /// Build from the preference values (the port of
    /// `AppProxy.configFrom`, utils/AppProxy.kt:57-66): `None` while the
    /// address isn't a valid `host:port`.
    pub fn from_prefs(
        kind: &str,
        url: &str,
        username: &str,
        password: &str,
    ) -> Option<ProxyConfig> {
        let (host, port) = Self::parse_address(url)?;
        Some(ProxyConfig {
            kind: ProxyKind::from_pref(kind),
            host,
            port,
            username: username.to_owned(),
            password: password.to_owned(),
        })
    }

    /// The address as people write it — an IPv6 host in brackets
    /// (upstream `addressLabel`, utils/AppProxy.kt:96-97).
    pub fn address_label(&self) -> String {
        if self.host.contains(':') {
            format!("[{}]:{}", self.host, self.port)
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }

    /// The `reqwest::Proxy` for this config (scheme + auth). Upstream
    /// proxies ALL app traffic (AppProxy.selector), so `Proxy::all` with
    /// the scheme in the URL (`socks5://` picks the SOCKS transport).
    pub fn to_reqwest(&self) -> Result<reqwest::Proxy, crate::error::NetError> {
        let url = match self.kind {
            Http => format!("http://{}", self.address_label()),
            Socks => format!("socks5://{}", self.address_label()),
        };
        let proxy =
            reqwest::Proxy::all(url).map_err(|e| crate::error::NetError::Other(e.to_string()))?;
        if self.has_login() {
            // Proxy-Authorization basic (AppProxy.kt:71 for HTTP; the SOCKS
            // authenticator maps onto the same reqwest call).
            return Ok(proxy.basic_auth(&self.username, &self.password));
        }
        Ok(proxy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_address_accepts_upstream_forms() {
        // AppProxy.kt:81: "1.2.3.4:8080", "proxy.example.com:1080",
        // "[2001:db8::1]:3128", plus pasted scheme/login
        assert_eq!(
            ProxyConfig::parse_address("1.2.3.4:8080"),
            Some(("1.2.3.4".to_owned(), 8080))
        );
        assert_eq!(
            ProxyConfig::parse_address("proxy.example.com:1080"),
            Some(("proxy.example.com".to_owned(), 1080))
        );
        assert_eq!(
            ProxyConfig::parse_address("[2001:db8::1]:3128"),
            Some(("2001:db8::1".to_owned(), 3128))
        );
        assert_eq!(
            ProxyConfig::parse_address("http://user:pass@1.2.3.4:8080/"),
            Some(("1.2.3.4".to_owned(), 8080))
        );
        assert_eq!(
            ProxyConfig::parse_address("socks5://proxy.example.com:1080"),
            Some(("proxy.example.com".to_owned(), 1080))
        );
    }

    #[test]
    fn parse_address_rejects_garbage() {
        assert_eq!(ProxyConfig::parse_address(""), None);
        assert_eq!(ProxyConfig::parse_address("nohost"), None);
        assert_eq!(ProxyConfig::parse_address("host:"), None);
        assert_eq!(ProxyConfig::parse_address("host:0"), None);
        assert_eq!(ProxyConfig::parse_address("host:70000"), None);
        assert_eq!(ProxyConfig::parse_address(":8080"), None);
    }

    #[test]
    fn from_prefs_matches_config_from() {
        let config = ProxyConfig::from_prefs("HTTP", "1.2.3.4:8080", "u", "p").unwrap();
        assert_eq!(config.kind, Http);
        assert_eq!(config.host, "1.2.3.4");
        assert_eq!(config.port, 8080);
        assert!(config.has_login());

        let socks = ProxyConfig::from_prefs("SOCKS", "[::1]:1080", "", "").unwrap();
        assert_eq!(socks.kind, Socks);
        assert!(!socks.has_login());
        assert_eq!(socks.address_label(), "[::1]:1080");

        // an invalid address yields None even with the switch on
        assert!(ProxyConfig::from_prefs("HTTP", "not an address", "", "").is_none());
    }

    #[test]
    fn to_reqwest_builds_scheme_and_auth() {
        let config = ProxyConfig::from_prefs("HTTP", "1.2.3.4:8080", "u", "p").unwrap();
        assert!(config.to_reqwest().is_ok());
        let socks = ProxyConfig::from_prefs("SOCKS", "proxy.example.com:1080", "", "").unwrap();
        assert!(socks.to_reqwest().is_ok());
    }
}
