//! Request context models — `providers/innertube/.../models/Context.kt` and
//! `models/YouTubeLocale.kt` (pin `bde01cb4`), field for field.

use serde::{Deserialize, Serialize};

/// `YouTubeLocale.kt` — the `gl` (content country) / `hl` (content
/// language) pair every request context carries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct YouTubeLocale {
    pub gl: String,
    pub hl: String,
}

impl Default for YouTubeLocale {
    /// Upstream default: `Locale.getDefault().country` +
    /// `Locale.getDefault().toLanguageTag()` (InnerTube.kt:40-43). The
    /// desktop port cannot read the JVM default; the session mirror feeds
    /// the OS locale in (App.kt:196-205) and this fallback matches the
    /// upstream hard floor (`"US"` / `"en"`, App.kt:200/205).
    fn default() -> Self {
        YouTubeLocale {
            gl: "US".into(),
            hl: "en".into(),
        }
    }
}

/// `Context.kt:5-11` — the top-level request context.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Context {
    pub client: ClientContext,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub third_party: Option<ThirdParty>,
    #[serde(rename = "request")]
    pub request: RequestContext,
    pub user: UserContext,
}

/// `Context.kt:12-28` — `Context.Client`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientContext {
    pub client_name: String,
    pub client_version: String,
    /// the native apps say which screen they're on and that they're a
    /// phone-like device, a request without them doesn't look like the app
    /// (`Context.kt:16-17`)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_screen: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_make: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub android_sdk_version: Option<String>,
    pub gl: String,
    pub hl: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visitor_data: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub utc_offset_minutes: Option<i32>,
}

/// `Context.kt:30-33` — `Context.ThirdParty` (the embedUrl for the
/// embedded-player clients, InnerTube.kt:262-268).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThirdParty {
    pub embed_url: String,
}

/// `Context.kt:35-39` — `Context.Request` (Kotlin defaults:
/// empty flags, `useSsl = true`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestContext {
    #[serde(default)]
    pub internal_experiment_flags: Vec<String>,
    #[serde(rename = "useSsl", default = "default_true")]
    pub use_ssl: bool,
}

fn default_true() -> bool {
    true
}

impl Default for RequestContext {
    fn default() -> Self {
        RequestContext {
            internal_experiment_flags: Vec::new(),
            use_ssl: true,
        }
    }
}

/// `Context.kt:41-45` — `Context.User` (Kotlin defaults:
/// `lockedSafetyMode = false`, no `onBehalfOfUser`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserContext {
    #[serde(default)]
    pub locked_safety_mode: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_behalf_of_user: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::{ClientKey, ClientRegistry};

    /// `YouTubeClient.kt:36-56` (`toContext`) — including the two branch
    /// behaviors: `utcOffsetMinutes` only for clientScreen clients,
    /// `onBehalfOfUser` only when login is supported.
    #[test]
    fn to_context_branches_match_upstream() {
        let locale = YouTubeLocale {
            gl: "DE".into(),
            hl: "de".into(),
        };
        // VISIONOS: clientScreen set -> utcOffsetMinutes 0; login NOT
        // supported -> no onBehalfOfUser even with a dataSyncId
        let vision = ClientRegistry::new().get(ClientKey::VisionOs);
        let ctx = vision.to_context(&locale, Some("Cgt Visitor"), Some("sync||id"));
        assert_eq!(ctx.client.utc_offset_minutes, Some(0));
        assert_eq!(ctx.client.visitor_data.as_deref(), Some("Cgt Visitor"));
        assert!(ctx.user.on_behalf_of_user.is_none());
        assert_eq!(ctx.client.client_screen.as_deref(), Some("WATCH"));
        assert_eq!(ctx.client.platform.as_deref(), Some("MOBILE"));

        // WEB_REMIX: no clientScreen -> no utcOffsetMinutes; login
        // supported -> onBehalfOfUser carries the dataSyncId
        let remix = ClientRegistry::new().get(ClientKey::WebRemix);
        let ctx = remix.to_context(&locale, None, Some("sync-id"));
        assert_eq!(ctx.client.utc_offset_minutes, None);
        assert_eq!(ctx.user.on_behalf_of_user.as_deref(), Some("sync-id"));
        assert_eq!(ctx.client.gl, "DE");
        assert_eq!(ctx.client.hl, "de");
    }

    /// The wire shape must match kotlinx-serialization: camelCase keys,
    /// explicitNulls = false (nulls omitted), encodeDefaults = true
    /// (InnerTube.kt:80-84).
    #[test]
    fn context_serializes_like_kotlinx() {
        let locale = YouTubeLocale::default();
        let remix = ClientRegistry::new().get(ClientKey::WebRemix);
        let ctx = remix.to_context(&locale, Some("CgtsXYZ"), None);
        let json = serde_json::to_value(&ctx).unwrap();
        let client = json.get("client").unwrap();
        assert_eq!(client["clientName"], "WEB_REMIX");
        assert_eq!(client["clientVersion"], "1.20260707.12.00");
        assert_eq!(client["visitorData"], "CgtsXYZ");
        // explicitNulls=false: absent fields are NOT emitted
        assert!(client.get("osName").is_none());
        assert!(client.get("utcOffsetMinutes").is_none());
        // encodeDefaults=true: defaults ARE emitted
        assert_eq!(json["request"]["useSsl"], true);
        assert_eq!(json["user"]["lockedSafetyMode"], false);
        // onBehalfOfUser=null is omitted entirely (null)
        assert!(json["user"].get("onBehalfOfUser").is_none());
    }
}
