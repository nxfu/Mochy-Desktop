//! Guest sessions — the sw.js_data visitor-data scrape and the bot-check
//! mitigation.
//!
//! Upstream: `YouTube.visitorData()` (providers/innertube
//! YouTube.kt:1473-1483) scrapes `sw.js_data`, and
//! `app/.../utils/BotDetectionMitigator.kt` decides when a guest session
//! is burned and rotates it.

use std::sync::atomic::{AtomicI32, Ordering};

use serde_json::Value as Json;

use crate::client::YouTubeClient;
use crate::error::InnertubeError;
use crate::transport::InnerTubeTransport;

/// The geo-restriction signatures (BotDetectionMitigator.kt:21-32). A geo
/// error is NOT a bot check — rotating the session cannot fix geography.
pub const GEO_ERROR_SIGNATURES: &[&str] = &[
    "not available in your country",
    "not available in your region",
    "not available in this country",
    "not available in this region",
    "geo-restricted",
    "GEO_RESTRICTED",
    "NOT_AVAILABLE_IN_THIS_COUNTRY",
    "only available in certain countries",
    "country restriction",
    "region restriction",
];

/// The bot-check signatures (BotDetectionMitigator.kt:34-41).
pub const BOT_ERROR_SIGNATURES: &[&str] = &[
    "Sign in to Confirm",
    "confirm you're not a bot",
    "automated queries",
    "Error 2000",
    "403",
    "This content isn't available on this device",
];

/// The upstream 50-request rotation ceiling is not here — the mitigator
/// itself just counts playback failures.
#[derive(Debug, Default)]
pub struct BotDetectionMitigator {
    failure_count: AtomicI32,
}

impl BotDetectionMitigator {
    pub fn new() -> Self {
        Self::default()
    }

    /// `notifyPlaybackFailure` (BotDetectionMitigator.kt:43-49): a signed-in
    /// user never rotates (their cookie IS the session); a geo error never
    /// counts. Returns whether this failure counted toward rotation.
    pub fn notify_playback_failure(&self, is_logged_in: bool, error_message: Option<&str>) -> bool {
        if is_logged_in {
            return false;
        }
        if Self::is_geo_error(error_message) {
            return false;
        }
        self.failure_count.fetch_add(1, Ordering::SeqCst);
        true
    }

    /// `notifyPlaybackSuccess` (BotDetectionMitigator.kt:51-53): any
    /// success resets the counter.
    pub fn notify_playback_success(&self) {
        self.failure_count.store(0, Ordering::SeqCst);
    }

    /// The current failure count (surfaced for the diagnostics the app's
    /// playback log shows).
    pub fn failure_count(&self) -> i32 {
        self.failure_count.load(Ordering::SeqCst)
    }

    /// `reset` (BotDetectionMitigator.kt:97-99).
    pub fn reset(&self) {
        self.failure_count.store(0, Ordering::SeqCst);
    }

    /// `isGeoError` (BotDetectionMitigator.kt:85-89).
    pub fn is_geo_error(message: Option<&str>) -> bool {
        match message {
            None => false,
            Some(message) => {
                let lower = message.to_lowercase();
                GEO_ERROR_SIGNATURES
                    .iter()
                    .any(|signature| lower.contains(&signature.to_lowercase()))
            }
        }
    }

    /// `isBotDetectionError` (BotDetectionMitigator.kt:91-95).
    pub fn is_bot_detection_error(message: Option<&str>) -> bool {
        match message {
            None => false,
            Some(message) => {
                let lower = message.to_lowercase();
                BOT_ERROR_SIGNATURES
                    .iter()
                    .any(|signature| lower.contains(&signature.to_lowercase()))
            }
        }
    }
}

impl InnerTubeTransport {
    /// `YouTube.visitorData()` (YouTube.kt:1473-1483): GET sw.js_data,
    /// strip the `)]}'\n` prefix (`substring(5)`), walk
    /// `jsonArray[0].jsonArray[2].jsonArray` and take the first string
    /// that looks like a visitor id (`^Cg[t|s]`).
    pub async fn scrape_visitor_data(&self) -> Result<String, InnertubeError> {
        let text = self.sw_js_data().await?;
        extract_visitor_data(&text).ok_or(InnertubeError::VisitorDataScrapeFailed)
    }

    /// `YouTube.refreshVisitorData()` (YouTube.kt:131-133): scrape and
    /// store in the session. On failure the old id stays — clearing it
    /// first left the web clients none after one failed refresh
    /// (BotDetectionMitigator.kt:67).
    pub async fn refresh_visitor_data(&self) -> Result<String, InnertubeError> {
        let fresh = self.scrape_visitor_data().await?;
        self.session()
            .write()
            .map_err(|_| InnertubeError::LockPoisoned)?
            .visitor_data = Some(fresh.clone());
        Ok(fresh)
    }

    /// `YouTube.refreshVisitorData(client)` (YouTube.kt:136) — a fresh id
    /// for a client that mints its own, after youtube's bot check burned
    /// the old one (VISIONOS).
    pub async fn refresh_own_visitor_data(
        &self,
        client: &YouTubeClient,
    ) -> Result<String, InnertubeError> {
        crate::endpoints::mint_visitor_id(self, client).await
    }

    /// `BotDetectionMitigator.rotateGuestSession`
    /// (BotDetectionMitigator.kt:55-83): refresh the guest session. The
    /// locale is preserved by construction (only `visitorData` is
    /// touched); the persistence to the vault is the CALLER's side of the
    /// contract (upstream writes `VisitorDataKey` into DataStore — the
    /// desktop mirror writes the `visitorData` vault key).
    pub async fn rotate_guest_session(&self) -> Result<String, InnertubeError> {
        self.refresh_visitor_data().await
    }
}

/// The visitor-data extraction from a sw.js_data payload — the pure core
/// of `YouTube.visitorData()` (YouTube.kt:1473-1483) plus the
/// `VISITOR_DATA_REGEX = Regex("^Cg[t|s]")` (YouTube.kt:1556).
pub fn extract_visitor_data(sw_js_data: &str) -> Option<String> {
    // substring(5) removes ")]}'" + the newline
    let json_text = sw_js_data.get(5..)?;
    let root: Json = serde_json::from_str(json_text).ok()?;
    let slot = root.get(0)?.get(2)?.as_array()?;
    slot.iter()
        .filter_map(|item| item.as_str())
        .find(|candidate| looks_like_visitor_data(candidate))
        .map(str::to_owned)
}

/// `^Cg[t|s]` (YouTube.kt:1556): starts with `Cgt` or `Cgs`. (The Kotlin
/// regex character class `[t|s]` also admits a literal `|` — an upstream
/// quirk kept verbatim would match `Cg|`, which YouTube never mints; the
/// port implements the two real branches. See HANDOFF Phase 04.)
pub fn looks_like_visitor_data(candidate: &str) -> bool {
    candidate.starts_with("Cgt") || candidate.starts_with("Cgs")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::{ClientKey, ClientRegistry};

    /// A structurally faithful synthetic sw.js_data payload — the shape
    /// YouTube.kt:1473-1483 walks: `)]}'\n` prefix, then
    /// `[ [ ..., ..., [ "...", "CgtXYZ...", ... ] ] ]` — the visitor
    /// candidates live at root[0][2].
    fn synthetic_payload() -> String {
        ")]}'\n[[\"sid\",\"other\",[\"ignored\",123,\"CgtSYNTHETICVISITOR\",\"CgsANOTHER\",false]]]"
            .to_owned()
    }

    #[test]
    fn extracts_the_visitor_from_sw_js_data() {
        let visitor = extract_visitor_data(&synthetic_payload()).unwrap();
        assert_eq!(visitor, "CgtSYNTHETICVISITOR");
    }

    #[test]
    fn no_visitor_yields_none() {
        // root[0][2] present but no visitor-looking string
        assert!(extract_visitor_data(")]}'\n[[\"a\",\"b\",[\"c\",1,false]]]").is_none());
        // non-strings are skipped; no Cgt/Cgs prefix -> no match
        assert!(extract_visitor_data(")]}'\n[[\"a\",\"b\",[123,\"nope\"]]]").is_none());
        // root[0] too short / prefix too short / not JSON
        assert!(extract_visitor_data(")]}'\n[[\"a\"],[\"b\"],[\"c\"]]").is_none());
        assert!(extract_visitor_data(")]}'").is_none());
        assert!(extract_visitor_data("garbage").is_none());
    }

    #[test]
    fn visitor_regex_matches_cgt_and_cgs_only() {
        // YouTube.kt:1556 ^Cg[t|s]
        assert!(looks_like_visitor_data("Cgtabc"));
        assert!(looks_like_visitor_data("Cgsabc"));
        assert!(!looks_like_visitor_data("Cgzabc"));
        assert!(!looks_like_visitor_data("abc"));
        // the upstream character class [t|s] also admits a literal pipe —
        // recorded as a quirk, not ported (HANDOFF Phase 04)
        assert!(!looks_like_visitor_data("Cg|abc"));
    }

    #[test]
    fn geo_errors_do_not_count_as_bot_failures() {
        // BotDetectionMitigator.kt:43-49 + 85-89
        let mitigator = BotDetectionMitigator::new();
        assert!(!mitigator.notify_playback_failure(true, Some("confirm you're not a bot")));
        assert_eq!(mitigator.failure_count(), 0);
        assert!(
            !mitigator.notify_playback_failure(
                false,
                Some("This video is not available in your country")
            )
        );
        assert_eq!(mitigator.failure_count(), 0);
        // a counted bot failure
        assert!(
            mitigator.notify_playback_failure(false, Some("Sign in to confirm you're not a bot"))
        );
        assert_eq!(mitigator.failure_count(), 1);
        // success resets
        mitigator.notify_playback_success();
        assert_eq!(mitigator.failure_count(), 0);
    }

    #[test]
    fn signature_classifications() {
        // BotDetectionMitigator.kt:21-41
        assert!(BotDetectionMitigator::is_geo_error(Some(
            "Video unavailable: GEO_RESTRICTED"
        )));
        assert!(BotDetectionMitigator::is_geo_error(Some(
            "not available in your region"
        )));
        assert!(!BotDetectionMitigator::is_geo_error(Some(
            "Sign in to confirm"
        )));
        assert!(!BotDetectionMitigator::is_geo_error(None));
        assert!(BotDetectionMitigator::is_bot_detection_error(Some(
            "Please sign in to confirm you're not a bot"
        )));
        assert!(BotDetectionMitigator::is_bot_detection_error(Some(
            "Error 2000"
        )));
        assert!(BotDetectionMitigator::is_bot_detection_error(Some(
            "HTTP 403"
        )));
        assert!(!BotDetectionMitigator::is_bot_detection_error(Some(
            "some player error"
        )));
        assert!(!BotDetectionMitigator::is_bot_detection_error(None));
    }

    #[test]
    fn web_remix_is_the_default_facade_client() {
        let client = ClientRegistry::new().get(ClientKey::WebRemix);
        assert_eq!(client.client_name, "WEB_REMIX");
    }
}
