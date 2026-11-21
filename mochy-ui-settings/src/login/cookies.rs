//! Cookie-jar → header extraction — the `CookieManager.getCookie(url)`
//! half of `YouTubeLoginScreen.finishLogin` (lines 160-166).
//!
//! Upstream reads the cookie string for the current page URL from the
//! Android `CookieManager`, which includes **HttpOnly** cookies — the
//! load-bearing property (SAPISID is HttpOnly). On desktop, wry's
//! `cookies_for_url` provides the same (WebView2 / webkit2gtk /
//! WKWebView all hand the full jar); the pure functions here turn a
//! jar snapshot into the exact `name=value; name2=value2` string the
//! app stores, so the mapping is testable against a recorded fixture
//! without any webview.
//!
//! Matching follows RFC 6265 §5.4 the way cookie managers apply it:
//! domain-match (a `.youtube.com` cookie matches `music.youtube.com`;
//! a host-only cookie matches its exact host), path-match (`/`
//! matches everything), and expiry (`None` = session cookie, alive
//! until the jar closes). HttpOnly and Secure are NOT filters here —
//! `getCookie` returns them all (Secure only restricts transport, and
//! the harvest happens over https anyway).

use serde::{Deserialize, Serialize};

/// One cookie from a webview jar snapshot — the fields wry's
/// `cookie::Cookie` exposes, plain so the core stays webview-free.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JarCookieRecord {
    pub name: String,
    pub value: String,
    /// As stored: leading dot for domain cookies (`.youtube.com`),
    /// none for host-only ones.
    pub domain: String,
    /// Defaults to `/` when the jar omits it.
    pub path: String,
    /// Expiry in Unix seconds; `None` = session cookie.
    pub expires_at: Option<i64>,
    pub http_only: bool,
    pub secure: bool,
}

/// The URL the harvest targets — upstream passes the page URL to
/// `getCookie`; the port always asks for the music.youtube.com origin
/// (the page the sign-in lands on).
pub const HARVEST_URL: &str = "https://music.youtube.com/";
const HARVEST_HOST: &str = "music.youtube.com";
const HARVEST_PATH: &str = "/";

/// `CookieManager.getCookie(url)` — every jar cookie that would be sent
/// to `url`, joined `name=value; ...` in jar order (the order the
/// webview returns them; upstream keeps CookieManager's order too).
pub fn cookie_header_for(jar: &[JarCookieRecord], now_unix: i64) -> String {
    jar.iter()
        .filter(|cookie| alive(cookie, now_unix))
        .filter(|cookie| domain_matches(HARVEST_HOST, &cookie.domain))
        .filter(|cookie| path_matches(HARVEST_PATH, &cookie.path))
        .map(|cookie| format!("{}={}", cookie.name, cookie.value))
        .collect::<Vec<_>>()
        .join("; ")
}

/// Whether the harvested header carries the one cookie the login
/// actually requires — `finishLogin`'s guard
/// (YouTubeLoginScreen.kt:165-166):
/// `if ("SAPISID" !in parseCookieString(cookie)) return` — no
/// SAPISID means sign-in is not finished yet; keep waiting.
pub fn has_sapisid(cookie_header: &str) -> bool {
    mochy_innertube::parse_cookie_string(cookie_header).contains_key("SAPISID")
}

fn alive(cookie: &JarCookieRecord, now_unix: i64) -> bool {
    match cookie.expires_at {
        Some(expires_at) => now_unix < expires_at,
        None => true,
    }
}

/// RFC 6265 §5.1.3 domain-match against the harvest host.
fn domain_matches(host: &str, cookie_domain: &str) -> bool {
    let domain = cookie_domain.trim_start_matches('.');
    if domain.is_empty() {
        return false;
    }
    if host == domain {
        return true;
    }
    host.ends_with(&format!(".{domain}"))
}

/// RFC 6265 §5.1.4 path-match against the harvest path.
fn path_matches(request_path: &str, cookie_path: &str) -> bool {
    let cookie_path = if cookie_path.is_empty() {
        "/"
    } else {
        cookie_path
    };
    if request_path == cookie_path {
        return true;
    }
    request_path.starts_with(cookie_path)
        && (cookie_path.ends_with('/')
            || request_path.as_bytes().get(cookie_path.len()) == Some(&b'/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(name: &str, domain: &str) -> JarCookieRecord {
        JarCookieRecord {
            name: name.into(),
            value: format!("v-{name}"),
            domain: domain.into(),
            path: "/".into(),
            expires_at: None,
            http_only: false,
            secure: false,
        }
    }

    /// The domain/path/expiry filters, table-driven.
    #[test]
    fn harvest_keeps_matching_live_cookies() {
        let now = 1_700_000_000;
        let jar = vec![
            record("SAPISID", ".youtube.com"),
            JarCookieRecord {
                http_only: true,
                ..record("HSID", ".youtube.com")
            },
            // wrong domain trees
            record("SID", ".google.com"),
            record("OTHER", "youtube.com.duplicate.example"),
            // host-only cookie for a sibling subdomain
            record("WWW_ONLY", "www.youtube.com"),
            // host-only cookie for the exact harvest host
            JarCookieRecord {
                secure: true,
                ..record("MUSIC_ONLY", "music.youtube.com")
            },
            // expired
            JarCookieRecord {
                expires_at: Some(now - 1),
                ..record("EXPIRED", ".youtube.com")
            },
            // path outside
            JarCookieRecord {
                path: "/other".into(),
                ..record("PATHBOUND", ".youtube.com")
            },
        ];

        let header = cookie_header_for(&jar, now);
        assert_eq!(
            header,
            "SAPISID=v-SAPISID; HSID=v-HSID; MUSIC_ONLY=v-MUSIC_ONLY"
        );
        assert!(has_sapisid(&header));
    }

    /// No SAPISID in the jar -> the header must fail the guard (the
    /// window keeps waiting, exactly like finishLogin's early return).
    #[test]
    fn missing_sapisid_fails_the_guard() {
        let jar = vec![
            record("SID", ".youtube.com"),
            record("HSID", ".youtube.com"),
        ];
        let header = cookie_header_for(&jar, 0);
        assert!(!has_sapisid(&header));
        assert!(!has_sapisid("SAPISIDX=1"), "prefix must not match");
        assert!(has_sapisid("a=1; SAPISID=real; b=2"));
    }

    /// An empty jar is an empty string (getCookie's null case maps to
    /// no cookies — the caller keeps waiting).
    #[test]
    fn empty_jar_yields_empty_header() {
        assert_eq!(cookie_header_for(&[], 0), "");
    }
}
