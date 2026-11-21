//! Cookie auth — the SAPISIDHASH login header and cookie parsing.
//!
//! Upstream: `providers/innertube/.../InnerTube.kt:44-51` (the cookie
//! setter + `cookieMap`), `InnerTube.kt:169-178` (the header injection),
//! and `providers/innertube/.../utils/Utils.kt:76-88` (`sha1` +
//! `parseCookieString`).

use std::collections::HashMap;

use sha1::{Digest, Sha1};

/// `Utils.kt:76` — `ByteArray.toHex()`.
pub fn to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// `Utils.kt:78` — `sha1(str)` = hex SHA-1 of the UTF-8 bytes.
pub fn sha1_hex(input: &str) -> String {
    let mut hasher = Sha1::new();
    hasher.update(input.as_bytes());
    to_hex(&hasher.finalize())
}

/// `Utils.kt:80-88` — `parseCookieString`: split on `"; "`, keep parts
/// containing `'='`, first `'='` splits name/value (later `'='`s stay in
/// the value). Duplicate names keep the LAST value (Kotlin `.toMap()`).
pub fn parse_cookie_string(cookie: &str) -> HashMap<String, String> {
    cookie
        .split("; ")
        .filter(|part| !part.is_empty())
        .filter_map(|part| {
            part.find('=').map(|split_index| {
                (
                    part[..split_index].to_owned(),
                    part[split_index + 1..].to_owned(),
                )
            })
        })
        .collect()
}

/// The SAPISIDHASH login header value — `InnerTube.kt:172-176`:
///
/// ```kotlin
/// if ("SAPISID" !in cookieMap) return@let
/// val currentTime = System.currentTimeMillis() / 1000
/// // the hash is tied to the origin it goes to, one made for
/// // music.youtube.com is a 401 on www.youtube.com
/// val sapisidHash = sha1("$currentTime ${cookieMap["SAPISID"]} ${client.apiOrigin}")
/// append("Authorization", "SAPISIDHASH ${currentTime}_${sapisidHash}")
/// ```
///
/// The hash is origin-bound: the same `(time, SAPISID)` pair yields a
/// different header for `https://music.youtube.com` than for
/// `https://www.youtube.com`, and a music-origin hash is a 401 on
/// www.youtube.com.
pub fn sapisidhash(time_unix: u64, sapisid: &str, origin: &str) -> String {
    let digest = sha1_hex(&format!("{time_unix} {sapisid} {origin}"));
    format!("{time_unix}_{digest}")
}

/// The full `Authorization` header value (with the `SAPISIDHASH ` prefix).
pub fn authorization_header(time_unix: u64, sapisid: &str, origin: &str) -> String {
    format!("SAPISIDHASH {}", sapisidhash(time_unix, sapisid, origin))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Known-answer vector for the hash pipeline itself: the exact
    /// string `"{time} {SAPISID} {origin}"` is SHA-1'd (not the header
    /// form). Verified against an independent SHA-1 implementation
    /// (`echo -n ... | sha1sum`).
    #[test]
    fn sha1_hex_known_answer() {
        assert_eq!(sha1_hex(""), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        assert_eq!(sha1_hex("abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(
            sha1_hex("1700000000 SAPISIDVALUE https://music.youtube.com"),
            "7ef7126262ab644136fc1ceef792ef2dac969ed5"
        );
    }

    /// SAPISIDHASH vector test with a known input triple — the acceptance
    /// test for the auth scheme. The expected digest was produced with
    /// `printf '%s' '1700000000 xYz123 https://music.youtube.com' | sha1sum`
    /// (independent of this crate's sha1 dependency).
    #[test]
    fn sapisidhash_vector_known_triple() {
        let header = sapisidhash(1_700_000_000, "xYz123", "https://music.youtube.com");
        assert_eq!(
            header,
            "1700000000_5f4cda8eda03266db676753eae4874d3d10e30a7"
        );
        assert_eq!(
            authorization_header(1_700_000_000, "xYz123", "https://music.youtube.com"),
            "SAPISIDHASH 1700000000_5f4cda8eda03266db676753eae4874d3d10e30a7"
        );
        // the same triple against the www origin is a different digest —
        // a music-origin hash is a 401 there (InnerTube.kt:174)
        assert_eq!(
            sapisidhash(1_700_000_000, "xYz123", "https://www.youtube.com"),
            "1700000000_af769e6e3d97c47c3eea8cfafe87077abd047a38"
        );
    }

    /// The origin-bound property (InnerTube.kt:174): the same triple with
    /// a different origin yields a different hash — a music-origin hash is
    /// a 401 on www.youtube.com.
    #[test]
    fn sapisidhash_is_origin_bound() {
        let music = sapisidhash(1_700_000_000, "xYz123", "https://music.youtube.com");
        let www = sapisidhash(1_700_000_000, "xYz123", "https://www.youtube.com");
        assert_ne!(music, www);
        // and time-dependent like upstream (currentTime in the preimage)
        let later = sapisidhash(1_700_000_001, "xYz123", "https://music.youtube.com");
        assert_ne!(music, later);
    }

    /// `Utils.kt:80-88` — separator is `"; "`, empty parts dropped, first
    /// `=` splits, later `=`s stay in the value, last duplicate wins.
    #[test]
    fn cookie_parsing_matches_upstream() {
        let map = parse_cookie_string(
            "SAPISID=xYz123; __Secure-YEC=Ok; VISITOR_INFO1_LIVE=CgtsXYZ; weird=one=two; ",
        );
        assert_eq!(map.get("SAPISID").map(String::as_str), Some("xYz123"));
        assert_eq!(map.get("__Secure-YEC").map(String::as_str), Some("Ok"));
        assert_eq!(
            map.get("VISITOR_INFO1_LIVE").map(String::as_str),
            Some("CgtsXYZ")
        );
        // value may contain '=' (indexOf('='), not split)
        assert_eq!(map.get("weird").map(String::as_str), Some("one=two"));
        // "NAME" without '=' is dropped; trailing "; " produces an empty
        // part that is filtered
        assert!(!parse_cookie_string("NOEQUALS; A=1").contains_key("NOEQUALS"));
        // duplicate: Kotlin toMap() keeps the last
        let dup = parse_cookie_string("A=1; A=2");
        assert_eq!(dup.get("A").map(String::as_str), Some("2"));
        // parts are split on "; " — a lone ";" does not separate
        let no_space = parse_cookie_string("A=1;B=2");
        assert_eq!(no_space.len(), 1);
        assert_eq!(no_space.get("A").map(String::as_str), Some("1;B=2"));
    }

    #[test]
    fn to_hex_lowercases() {
        assert_eq!(to_hex(&[0x00, 0x0f, 0xff]), "000fff");
    }
}
