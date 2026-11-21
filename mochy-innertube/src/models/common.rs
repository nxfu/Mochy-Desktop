//! The small shared models: `models/Icon.kt`, `models/Badges.kt`,
//! `models/Button.kt`, `models/SubscriptionButton.kt`,
//! `models/ResponseContext.kt`, `models/Continuation.kt`,
//! `models/ContinuationItemRenderer.kt`.

use serde::{Deserialize, Serialize};

use crate::models::endpoint::NavigationEndpoint;
use crate::models::runs::Runs;
use crate::models::thumbnails::Thumbnails;

/// `Icon.kt`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Icon {
    pub icon_type: String,
}

/// `Badges.kt`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Badges {
    #[serde(default)]
    pub music_inline_badge_renderer: Option<MusicInlineBadgeRenderer>,
}

/// `Badges.kt:10-12`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicInlineBadgeRenderer {
    pub icon: Icon,
}

/// `Button.kt`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Button {
    pub button_renderer: ButtonRenderer,
}

/// `Button.kt:10-15`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ButtonRenderer {
    pub text: Runs,
    #[serde(default)]
    pub navigation_endpoint: Option<NavigationEndpoint>,
    #[serde(default)]
    pub command: Option<NavigationEndpoint>,
    #[serde(default)]
    pub icon: Option<Icon>,
}

/// `SubscriptionButton.kt`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionButton {
    pub subscribe_button_renderer: SubscribeButtonRenderer,
}

/// `SubscriptionButton.kt:9-15`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscribeButtonRenderer {
    pub subscribed: bool,
    pub channel_id: String,
    #[serde(default)]
    pub long_subscriber_count_text: Option<Runs>,
    #[serde(default)]
    pub short_subscriber_count_text: Option<Runs>,
}

/// `ResponseContext.kt`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ResponseContext {
    #[serde(default)]
    pub visitor_data: Option<String>,
    #[serde(default)]
    pub service_tracking_params: Option<Vec<ServiceTrackingParam>>,
}

/// `ResponseContext.kt:10-20`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceTrackingParam {
    #[serde(default)]
    pub params: Vec<Param>,
    pub service: String,
}

/// `ResponseContext.kt:15-19`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Param {
    pub key: String,
    pub value: String,
}

/// `Continuation.kt:8-17` — `@JsonNames("nextContinuationData",
/// "nextRadioContinuationData")`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Continuation {
    #[serde(default, alias = "nextRadioContinuationData")]
    pub next_continuation_data: Option<NextContinuationData>,
}

/// `Continuation.kt:13-16`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NextContinuationData {
    pub continuation: String,
}

/// `List<Continuation>.getContinuation()` (`Continuation.kt:19-20`).
pub fn get_continuation(continuations: &[Continuation]) -> Option<&str> {
    continuations
        .first()?
        .next_continuation_data
        .as_ref()
        .map(|data| data.continuation.as_str())
}

/// `ContinuationItemRenderer.kt`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ContinuationItemRenderer {
    #[serde(default)]
    pub continuation_endpoint: Option<ContinuationEndpoint>,
}

/// `ContinuationItemRenderer.kt:10-15`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ContinuationEndpoint {
    #[serde(default)]
    pub continuation_command: Option<ContinuationCommand>,
}

/// `ContinuationItemRenderer.kt:14-17`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ContinuationCommand {
    pub token: Option<String>,
}

/// Keeps the `Thumbnails` import used even under `cfg(test)`-free builds
/// where no current model needs it (mirrors upstream's import surface).
#[allow(dead_code)]
fn _thumbnails_wired(_t: &Thumbnails) {}

#[cfg(test)]
mod tests {
    use super::*;

    /// Continuation.kt:19-20 + the radio alias.
    #[test]
    fn continuation_reads_both_names() {
        let plain: Vec<Continuation> = serde_json::from_value(
            serde_json::json!([{ "nextContinuationData": { "continuation": "tok1" } }]),
        )
        .unwrap();
        assert_eq!(get_continuation(&plain), Some("tok1"));

        let radio: Vec<Continuation> = serde_json::from_value(
            serde_json::json!([{ "nextRadioContinuationData": { "continuation": "tok2" } }]),
        )
        .unwrap();
        assert_eq!(get_continuation(&radio), Some("tok2"));

        assert_eq!(get_continuation(&[]), None);
    }
}
