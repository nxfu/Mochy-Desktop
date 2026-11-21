//! Stream-URL validation and cache correctness —
//! `validateStatus` (YTPlayerUtils.kt:1049-1099) plus the
//! stale-splice eviction rule the audio cache replays in Phase 09.
//!
//! The probe asks for the song's **last byte**: untokened urls
//! (IPADOS, ANDROID_VR) serve only the first ~786 KB and answer 403
//! past it, so probing the first byte — what this used to do —
//! accepted a url that dies ~30 seconds in. The headers matter
//! (googlevideo validates Origin/Referer against the client named in
//! the url's own `c=` parameter) and HEAD is unusable (403 even for
//! good urls), hence a ranged GET.
//!
//! The stale-splice rule ([`stale_splice`]): when a cached stream's
//! itag or content-length changed between resolutions, every cached
//! byte span of the old stream is worthless — splicing new bytes into
//! old ones produces a corrupt file. Phase 09's ranged byte cache
//! replays this exact rule on its subranges; the resolution-level
//! [`PlaybackCache`] applies it to whole resolved streams.

use std::collections::HashMap;
use std::time::Instant;

use crate::error::InnertubeError;
use crate::models::response::player::Format;
use crate::transport::InnerTubeTransport;

/// Past the ~786 KB an untokened url serves; probed when the song's
/// length isn't known (YTPlayerUtils.kt:1194).
pub const UNTOKENED_WINDOW_PROBE_BYTE: u64 = 1_048_576;

/// The outcome of one stream probe — the tri-state
/// `validateStatus`'s callers act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeOutcome {
    /// The url serves the whole song (2xx/206, or 416 with no known
    /// length: a song shorter than the probe is served, not refused —
    /// a cut-off url answers 403).
    Served,
    /// The url is cut off (403 past the untokened window) or the
    /// request failed outright.
    CutOff,
}

/// `validateStatus` (YTPlayerUtils.kt:1064-1099).
///
/// * `offset` = the last byte when the content length is known
///   (`contentLength - 1`), else the 1 MiB probe byte;
/// * a ranged GET `bytes=offset-offset`, HEAD is not usable;
/// * the stream headers ride along, plus the web UA default and the
///   session cookie when signed in (the account's urls check it).
pub async fn validate_stream_url(
    transport: &InnerTubeTransport,
    url: &str,
    headers: &[(String, String)],
    content_length: Option<u64>,
) -> ProbeOutcome {
    let offset = match content_length {
        Some(length) if length > 1 => length - 1,
        _ => UNTOKENED_WINDOW_PROBE_BYTE,
    };
    let mut all_headers: Vec<(String, String)> = headers.to_vec();
    if !all_headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("user-agent"))
    {
        all_headers.push((
            "User-Agent".to_owned(),
            crate::client::USER_AGENT_WEB.to_owned(),
        ));
    }
    if let Some(cookie) = transport
        .session()
        .read()
        .ok()
        .and_then(|guard| guard.cookie.clone())
    {
        all_headers.push(("Cookie".to_owned(), cookie));
    }
    let status = transport
        .probe_status(url, &all_headers, &offset.to_string())
        .await;
    match status {
        // 2xx or 206: served. 416 with no known length: the song is
        // shorter than the probe — served, not refused.
        Some(code) if (200..300).contains(&code) => ProbeOutcome::Served,
        Some(416) if content_length.is_none() => ProbeOutcome::Served,
        Some(_) => ProbeOutcome::CutOff,
        None => ProbeOutcome::CutOff,
    }
}

/// The stale-splice rule: has the stream changed under a cache?
///
/// Returns `true` when the cached spans must be **evicted** — the
/// itag or the content-length differs between what was cached and
/// what the new resolution says. A same-itag, same-length url refresh
/// (the common expiry refresh) keeps every cached byte valid because
/// the bytes themselves are identical.
///
/// Upstream states the rule at the walk level ("if itag or
/// content-length changed, evict cached spans"); Phase 09's audio
/// cache (`mochy-audio`, ranged byte cache with stale-splice
/// eviction) consumes this same function.
pub fn stale_splice(cached: &Format, fresh: &Format) -> bool {
    cached.itag != fresh.itag || cached.content_length != fresh.content_length
}

/// One cached resolution — the play-data cache the player consults
/// before walking, and the thing a 403 from the audio layer must
/// drop ("drop URL cache and force fresh resolution").
#[derive(Debug, Clone)]
pub struct CachedPlayback {
    /// The resolved stream url.
    pub url: String,
    /// The format it was resolved for (drives the stale-splice rule).
    pub format: Format,
    /// `streamExpiresInSeconds` at resolution time.
    pub expires_in_seconds: i64,
    /// When this entry was cached.
    pub cached_at: Instant,
}

impl CachedPlayback {
    /// Whether the cached url is still inside its expiry window (a
    /// small margin early, like the video cache's one-minute head
    /// start, so a song that is just starting doesn't run into a dead
    /// link halfway — VideoStreams.kt:20).
    pub fn is_live(&self) -> bool {
        let margin = std::time::Duration::from_secs(60);
        self.cached_at + std::time::Duration::from_secs(self.expires_in_seconds.max(1) as u64)
            > Instant::now() + margin
    }
}

/// The per-video resolved-stream cache.
///
/// Upstream kept resolved urls in the player service's queue state;
/// the port makes the rule explicit: consult before resolving,
/// populate after, **invalidate on 403** (the audio layer's signal
/// that the url died mid-play) and let the stale-splice check guard
/// reuse across qualities.
#[derive(Debug, Default)]
pub struct PlaybackCache {
    entries: HashMap<String, CachedPlayback>,
}

impl PlaybackCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// The cached stream for `video_id`, when it is still live and the
    /// fresh format has not gone stale against it.
    pub fn get(&self, video_id: &str, fresh_format: &Format) -> Option<&CachedPlayback> {
        let entry = self.entries.get(video_id)?;
        if !entry.is_live() {
            return None;
        }
        if stale_splice(&entry.format, fresh_format) {
            return None;
        }
        Some(entry)
    }

    /// Remember a resolution (the walk's success path).
    pub fn put(&mut self, video_id: &str, url: String, format: Format, expires_in_seconds: i64) {
        self.entries.insert(
            video_id.to_owned(),
            CachedPlayback {
                url,
                format,
                expires_in_seconds,
                cached_at: Instant::now(),
            },
        );
    }

    /// Drop one video's url — the 403 handler ("drop URL cache and
    /// force fresh resolution"). Returns whether anything was cached.
    pub fn invalidate(&mut self, video_id: &str) -> bool {
        self.entries.remove(video_id).is_some()
    }

    /// Drop everything — the 416 handler clears the cache and retries
    /// at position with a fresh resolution.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// How many videos are cached (diagnostics/tests).
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// `streamHeaders` (YTPlayerUtils.kt:208-230): the headers
/// googlevideo expects alongside a stream url — it checks them
/// against the client named in the url's own `c=` parameter, so a
/// WEB_REMIX url fetched without the YouTube Music origin is refused.
pub fn stream_headers(client: &crate::client::YouTubeClient) -> Vec<(String, String)> {
    let mut headers = vec![
        ("User-Agent".to_owned(), client.user_agent.clone()),
        ("Accept".to_owned(), "*/*".to_owned()),
        ("Accept-Language".to_owned(), "en-US,en;q=0.9".to_owned()),
    ];
    let (referer, origin) = match client.client_name.as_str() {
        "WEB_REMIX" => ("https://music.youtube.com/", "https://music.youtube.com"),
        "WEB_CREATOR" => ("https://studio.youtube.com/", "https://studio.youtube.com"),
        _ => ("https://www.youtube.com/", "https://www.youtube.com"),
    };
    headers.push(("Referer".to_owned(), referer.to_owned()));
    headers.push(("Origin".to_owned(), origin.to_owned()));
    headers
}

/// The InnertubeError the walk turns into per-client failures —
/// kept here so tests can shape fault injection without a transport.
pub type ProbeError = InnertubeError;

#[cfg(test)]
mod tests {
    use super::*;

    fn format(itag: i64, content_length: Option<i64>) -> Format {
        Format {
            itag,
            content_length,
            ..Default::default()
        }
    }

    #[test]
    fn stale_splice_triggers_on_itag_or_length_change() {
        let cached = format(140, Some(3_000_000));
        // same stream, refreshed url: nothing to evict
        assert!(!stale_splice(&cached, &format(140, Some(3_000_000))));
        // length changed (a different mix or a re-encode)
        assert!(stale_splice(&cached, &format(140, Some(3_500_000))));
        // itag changed (a different rung of the ladder)
        assert!(stale_splice(&cached, &format(251, Some(3_000_000))));
        // unknown lengths never claim a change on their own
        assert!(!stale_splice(&format(140, None), &format(140, None)));
    }

    #[test]
    fn cache_round_trip_and_the_403_invalidation() {
        let mut cache = PlaybackCache::new();
        cache.put(
            "dQw4w9WgXcQ",
            "https://gonecdf/v?itag=140".into(),
            format(140, Some(1_000)),
            21_600,
        );
        let entry = cache
            .get("dQw4w9WgXcQ", &format(140, Some(1_000)))
            .expect("live and not stale");
        assert_eq!(entry.url, "https://gonecdf/v?itag=140");
        // a stale-splice (different itag) refuses the cache
        assert!(
            cache
                .get("dQw4w9WgXcQ", &format(251, Some(1_000)))
                .is_none()
        );
        // the 403 handler drops the url and forces a fresh walk
        assert!(cache.invalidate("dQw4w9WgXcQ"));
        assert!(!cache.invalidate("dQw4w9WgXcQ"));
        assert!(cache.is_empty());
    }

    #[test]
    fn expired_entries_are_not_served() {
        let mut cache = PlaybackCache::new();
        let mut entry = CachedPlayback {
            url: "https://gonecdf/v?itag=140".into(),
            format: format(140, Some(1_000)),
            expires_in_seconds: 10,
            cached_at: Instant::now() - std::time::Duration::from_secs(3600),
        };
        cache.entries.insert("old".into(), entry.clone());
        assert!(cache.get("old", &entry.format).is_none());
        // a long-lived entry stays
        entry.expires_in_seconds = 21_600;
        entry.cached_at = Instant::now();
        cache.entries.insert("fresh".into(), entry);
        assert!(cache.get("fresh", &format(140, Some(1_000))).is_some());
    }

    #[test]
    fn cache_clear_is_the_416_handler() {
        let mut cache = PlaybackCache::new();
        cache.put("a", "u".into(), format(140, None), 100);
        cache.put("b", "u".into(), format(140, None), 100);
        assert_eq!(cache.len(), 2);
        cache.clear();
        assert!(cache.is_empty());
    }

    #[test]
    fn stream_headers_carry_the_client_origin() {
        let registry = crate::client::ClientRegistry::new();
        let remix = stream_headers(&registry.get(crate::client::ClientKey::WebRemix));
        assert_eq!(
            remix
                .iter()
                .find(|(name, _)| name == "Referer")
                .map(|(_, v)| v.clone()),
            Some("https://music.youtube.com/".into())
        );
        let creator = stream_headers(&registry.get(crate::client::ClientKey::WebCreator));
        assert_eq!(
            creator
                .iter()
                .find(|(name, _)| name == "Origin")
                .map(|(_, v)| v.clone()),
            Some("https://studio.youtube.com".into())
        );
        let visionos = stream_headers(&registry.get(crate::client::ClientKey::VisionOs));
        assert_eq!(
            visionos
                .iter()
                .find(|(name, _)| name == "Origin")
                .map(|(_, v)| v.clone()),
            Some("https://www.youtube.com".into())
        );
        // every set carries the client's own user agent
        assert!(
            visionos
                .iter()
                .any(|(name, value)| name == "User-Agent"
                    && value.starts_with("com.google.visionos"))
        );
    }
}
