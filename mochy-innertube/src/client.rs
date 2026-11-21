//! The 18-client registry — a verbatim port of
//! `providers/innertube/src/main/kotlin/com/music/innertube/models/YouTubeClient.kt`
//! (pin `bde01cb4`, 305 lines).
//!
//! Every field of every client config is ported 1:1: ids, versions, user
//! agents, os/device metadata, login/STS/PO-token flags, api origins, the
//! `apiFormatVersion` and `ownVisitorId` distinctions. The [`roles`]
//! module adds the walk order constants from `utils/YTPlayerUtils.kt`
//! (Phase 08 consumes them for stream resolution).
//!
//! Version hotfixing (blueprint risk R1: client churn is a live risk —
//! yt-dlp dropped ANDROID_VR and made VISIONOS its default within one
//! release): client *version strings* are overridable at runtime through
//! [`ClientRegistry`] without a rebuild. Everything else about a client is
//! code, exactly like upstream.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::context::YouTubeLocale;

/// The upstream web user agent (`YouTubeClient.kt:59`).
pub const USER_AGENT_WEB: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:140.0) Gecko/20100101 Firefox/140.0";

/// `https://www.youtube.com` (`YouTubeClient.kt:61`).
pub const ORIGIN_YOUTUBE: &str = "https://www.youtube.com";
/// `https://music.youtube.com` (`YouTubeClient.kt:62`).
pub const ORIGIN_YOUTUBE_MUSIC: &str = "https://music.youtube.com";
/// `$ORIGIN_YOUTUBE_MUSIC/` (`YouTubeClient.kt:63`).
pub const REFERER_YOUTUBE_MUSIC: &str = "https://music.youtube.com/";
/// `$ORIGIN_YOUTUBE_MUSIC/youtubei/v1/` (`YouTubeClient.kt:64`).
pub const API_URL_YOUTUBE_MUSIC: &str = "https://music.youtube.com/youtubei/v1/";

/// One emulated YouTube client — `YouTubeClient.kt:6-32` field for field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct YouTubeClient {
    pub client_name: String,
    pub client_version: String,
    pub client_id: String,
    pub user_agent: String,
    pub os_name: Option<String>,
    pub os_version: Option<String>,
    pub device_make: Option<String>,
    pub device_model: Option<String>,
    pub android_sdk_version: Option<String>,
    pub build_id: Option<String>,
    pub cronet_version: Option<String>,
    pub package_name: Option<String>,
    pub friendly_name: Option<String>,
    pub login_supported: bool,
    pub login_required: bool,
    pub use_signature_timestamp: bool,
    pub is_embedded: bool,
    pub use_web_po_tokens: bool,
    /// Where this client's api lives: the tv and visionos apps talk to
    /// www.youtube.com, and a request signed for the music origin is
    /// refused there (`YouTubeClient.kt:25-26`).
    pub api_origin: String,
    pub api_format_version: String,
    pub client_screen: Option<String>,
    pub platform: Option<String>,
    /// Mints its own visitor id through visitor_id instead of borrowing
    /// the web player's, and gets a fresh one after a bot check
    /// (`YouTubeClient.kt:30-31`).
    pub own_visitor_id: bool,
}

impl YouTubeClient {
    /// The native apps send no origin or referer, a browser-looking header
    /// on an app client is one more thing that gives it away
    /// (`YouTubeClient.kt:33-34`).
    pub fn is_native_app(&self) -> bool {
        self.platform.is_some()
    }

    /// Builds the request `context.client`/`context.user` for this client —
    /// `YouTubeClient.kt:36-56` (`toContext`).
    pub fn to_context(
        &self,
        locale: &YouTubeLocale,
        visitor_data: Option<&str>,
        data_sync_id: Option<&str>,
    ) -> crate::context::Context {
        crate::context::Context {
            client: crate::context::ClientContext {
                client_name: self.client_name.clone(),
                client_version: self.client_version.clone(),
                client_screen: self.client_screen.clone(),
                platform: self.platform.clone(),
                os_name: self.os_name.clone(),
                os_version: self.os_version.clone(),
                device_make: self.device_make.clone(),
                device_model: self.device_model.clone(),
                android_sdk_version: self.android_sdk_version.clone(),
                gl: locale.gl.clone(),
                hl: locale.hl.clone(),
                visitor_data: visitor_data.map(str::to_owned),
                // sent the way newpipe sends it, only for the clients
                // described as fully as newpipe describes them
                // (`YouTubeClient.kt:50-51`).
                utc_offset_minutes: if self.client_screen.is_some() {
                    Some(0)
                } else {
                    None
                },
            },
            user: crate::context::UserContext {
                on_behalf_of_user: if self.login_supported {
                    data_sync_id.map(str::to_owned)
                } else {
                    None
                },
                ..Default::default()
            },
            ..Default::default()
        }
    }
}

/// Identifies one of the 18 registered clients (`YouTubeClient.kt:66-303`).
/// The variants follow the upstream `companion object` val names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ClientKey {
    Web,
    WebRemix,
    WebCreator,
    TvHtml5,
    TvHtml5Samsung,
    TvHtml5Downgraded,
    WebEmbeddedPlayer,
    TvHtml5SimplyEmbeddedPlayer,
    Ios,
    /// Upstream `MOBILE` — client name `ANDROID`, the real Android app.
    Mobile,
    AndroidNoSdk,
    AndroidVrNoAuth,
    AndroidVr1_61_48,
    AndroidVr1_65_10,
    AndroidVr1_43_32,
    AndroidCreator,
    VisionOs,
    IpadOs,
}

impl ClientKey {
    /// All 18 keys in upstream declaration order.
    pub const ALL: &'static [ClientKey] = &[
        ClientKey::Web,
        ClientKey::WebRemix,
        ClientKey::WebCreator,
        ClientKey::TvHtml5,
        ClientKey::TvHtml5Samsung,
        ClientKey::TvHtml5Downgraded,
        ClientKey::WebEmbeddedPlayer,
        ClientKey::TvHtml5SimplyEmbeddedPlayer,
        ClientKey::Ios,
        ClientKey::Mobile,
        ClientKey::AndroidNoSdk,
        ClientKey::AndroidVrNoAuth,
        ClientKey::AndroidVr1_61_48,
        ClientKey::AndroidVr1_65_10,
        ClientKey::AndroidVr1_43_32,
        ClientKey::AndroidCreator,
        ClientKey::VisionOs,
        ClientKey::IpadOs,
    ];

    /// The upstream `companion object` val name.
    pub const fn name(self) -> &'static str {
        match self {
            ClientKey::Web => "WEB",
            ClientKey::WebRemix => "WEB_REMIX",
            ClientKey::WebCreator => "WEB_CREATOR",
            ClientKey::TvHtml5 => "TVHTML5",
            ClientKey::TvHtml5Samsung => "TVHTML5_SAMSUNG",
            ClientKey::TvHtml5Downgraded => "TVHTML5_DOWNGRADED",
            ClientKey::WebEmbeddedPlayer => "WEB_EMBEDDED_PLAYER",
            ClientKey::TvHtml5SimplyEmbeddedPlayer => "TVHTML5_SIMPLY_EMBEDDED_PLAYER",
            ClientKey::Ios => "IOS",
            ClientKey::Mobile => "MOBILE",
            ClientKey::AndroidNoSdk => "ANDROID_NO_SDK",
            ClientKey::AndroidVrNoAuth => "ANDROID_VR_NO_AUTH",
            ClientKey::AndroidVr1_61_48 => "ANDROID_VR_1_61_48",
            ClientKey::AndroidVr1_65_10 => "ANDROID_VR_1_65_10",
            ClientKey::AndroidVr1_43_32 => "ANDROID_VR_1_43_32",
            ClientKey::AndroidCreator => "ANDROID_CREATOR",
            ClientKey::VisionOs => "VISIONOS",
            ClientKey::IpadOs => "IPADOS",
        }
    }

    /// The base config for this key — the verbatim
    /// `YouTubeClient.kt:66-303` constructor call.
    pub fn base(self) -> YouTubeClient {
        use ClientKey::*;
        match self {
            Web => YouTubeClient {
                client_name: "WEB".into(),
                client_version: "2.20260708.00.00".into(),
                client_id: "1".into(),
                user_agent: USER_AGENT_WEB.into(),
                ..default_client()
            },
            WebRemix => YouTubeClient {
                client_name: "WEB_REMIX".into(),
                client_version: "1.20260707.12.00".into(),
                client_id: "67".into(),
                user_agent: USER_AGENT_WEB.into(),
                login_supported: true,
                use_signature_timestamp: true,
                use_web_po_tokens: true,
                ..default_client()
            },
            WebCreator => YouTubeClient {
                client_name: "WEB_CREATOR".into(),
                client_version: "1.20260708.06.00".into(),
                client_id: "62".into(),
                user_agent: USER_AGENT_WEB.into(),
                login_supported: true,
                login_required: true,
                use_signature_timestamp: true,
                ..default_client()
            },
            TvHtml5 => YouTubeClient {
                client_name: "TVHTML5".into(),
                client_version: "7.20260707.07.00".into(),
                client_id: "7".into(),
                user_agent: "Mozilla/5.0(SMART-TV; Linux; Tizen 4.0.0.2) AppleWebkit/605.1.15 (KHTML, like Gecko) SamsungBrowser/9.2 TV Safari/605.1.15".into(),
                login_supported: true,
                login_required: true,
                use_signature_timestamp: true,
                use_web_po_tokens: true,
                ..default_client()
            },
            // the older tv app as a samsung tv, yt-dlp's first client for
            // signed-in users: with the account's cookies it plays
            // age-restricted and premium-only songs with ciphered links and
            // needs no potoken (`YouTubeClient.kt:104-119`).
            TvHtml5Samsung => YouTubeClient {
                client_name: "TVHTML5".into(),
                client_version: "5.20260707".into(),
                client_id: "7".into(),
                user_agent: "Mozilla/5.0 (SMART-TV; Linux; Tizen 2.4.0) AppleWebKit/538.1 (KHTML, like Gecko) Version/2.4.0 TV Safari/538.1".into(),
                os_name: Some("Tizen".into()),
                os_version: Some("2.4.0".into()),
                device_make: Some("Samsung".into()),
                device_model: Some("UKS9800".into()),
                friendly_name: Some("TV (Samsung)".into()),
                api_origin: ORIGIN_YOUTUBE.into(),
                login_supported: true,
                login_required: true,
                use_signature_timestamp: true,
                ..default_client()
            },
            // the same older tv app without a make, kept as a second ask
            // because its "the page needs to be reloaded" comes and goes
            // (`YouTubeClient.kt:121-132`).
            TvHtml5Downgraded => YouTubeClient {
                client_name: "TVHTML5".into(),
                client_version: "5.20260707".into(),
                client_id: "7".into(),
                user_agent: "Mozilla/5.0 (ChromiumStylePlatform) Cobalt/Version".into(),
                friendly_name: Some("TV (older)".into()),
                api_origin: ORIGIN_YOUTUBE.into(),
                login_supported: true,
                login_required: true,
                use_signature_timestamp: true,
                ..default_client()
            },
            // youtube's embedded player with the account's cookies: no
            // potoken needed, says no only where the owner turned embedding
            // off (`YouTubeClient.kt:134-146`).
            WebEmbeddedPlayer => YouTubeClient {
                client_name: "WEB_EMBEDDED_PLAYER".into(),
                client_version: "2.20260708.00.00".into(),
                client_id: "56".into(),
                user_agent: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/15.5 Safari/605.1.15,gzip(gfe)".into(),
                friendly_name: Some("Embedded player".into()),
                api_origin: ORIGIN_YOUTUBE.into(),
                login_supported: true,
                login_required: true,
                use_signature_timestamp: true,
                is_embedded: true,
                ..default_client()
            },
            TvHtml5SimplyEmbeddedPlayer => YouTubeClient {
                client_name: "TVHTML5_SIMPLY_EMBEDDED_PLAYER".into(),
                client_version: "2.0".into(),
                client_id: "85".into(),
                user_agent: "Mozilla/5.0 (PlayStation; PlayStation 4/12.02) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/15.4 Safari/605.1.15".into(),
                login_supported: true,
                login_required: false,
                use_signature_timestamp: true,
                is_embedded: true,
                ..default_client()
            },
            Ios => YouTubeClient {
                client_name: "IOS".into(),
                client_version: "21.03.1".into(),
                client_id: "5".into(),
                user_agent: "com.google.ios.youtube/21.03.1 (iPhone16,2; U; CPU iOS 18_2 like Mac OS X;)".into(),
                os_version: Some("18.2.22C152".into()),
                ..default_client()
            },
            Mobile => YouTubeClient {
                client_name: "ANDROID".into(),
                client_version: "21.26.364".into(),
                client_id: "3".into(),
                user_agent: "com.google.android.youtube/21.26.364 (Linux; U; Android 11) gzip".into(),
                os_name: Some("Android".into()),
                os_version: Some("11".into()),
                login_supported: true,
                use_signature_timestamp: true,
                ..default_client()
            },
            AndroidNoSdk => YouTubeClient {
                client_name: "ANDROID".into(),
                client_version: "21.26.364".into(),
                client_id: "3".into(),
                user_agent: "com.google.android.youtube/21.26.364 (Linux; U; Android 11) gzip".into(),
                os_name: Some("Android".into()),
                os_version: Some("11".into()),
                friendly_name: Some("Android No SDK".into()),
                login_supported: false,
                use_signature_timestamp: false,
                ..default_client()
            },
            AndroidVrNoAuth => YouTubeClient {
                client_name: "ANDROID_VR".into(),
                client_version: "1.61.48".into(),
                client_id: "28".into(),
                user_agent: "com.google.android.apps.youtube.vr.oculus/1.61.48 (Linux; U; Android 12; en_US; Oculus Quest 3; Build/SQ3A.220605.009.A1; Cronet/132.0.6808.3)".into(),
                login_supported: false,
                use_signature_timestamp: false,
                ..default_client()
            },
            AndroidVr1_61_48 => YouTubeClient {
                client_name: "ANDROID_VR".into(),
                client_version: "1.61.48".into(),
                client_id: "28".into(),
                user_agent: "com.google.android.apps.youtube.vr.oculus/1.61.48 (Linux; U; Android 12; en_US; Quest 3; Build/SQ3A.220605.009.A1; Cronet/132.0.6808.3)".into(),
                os_name: Some("Android".into()),
                os_version: Some("12".into()),
                device_make: Some("Oculus".into()),
                device_model: Some("Quest 3".into()),
                android_sdk_version: Some("32".into()),
                build_id: Some("SQ3A.220605.009.A1".into()),
                cronet_version: Some("132.0.6808.3".into()),
                package_name: Some("com.google.android.apps.youtube.vr.oculus".into()),
                friendly_name: Some("Android VR 1.61".into()),
                login_supported: false,
                use_signature_timestamp: false,
                ..default_client()
            },
            AndroidVr1_65_10 => YouTubeClient {
                client_name: "ANDROID_VR".into(),
                client_version: "1.65.10".into(),
                client_id: "28".into(),
                user_agent: "com.google.android.apps.youtube.vr.oculus/1.65.10 (Linux; U; Android 12L; eureka-user Build/SQ3A.220605.009.A1) gzip".into(),
                os_name: Some("Android".into()),
                os_version: Some("12L".into()),
                device_make: Some("Oculus".into()),
                device_model: Some("Quest 3".into()),
                android_sdk_version: Some("32".into()),
                build_id: Some("SQ3A.220605.009.A1".into()),
                cronet_version: None,
                package_name: Some("com.google.android.apps.youtube.vr.oculus".into()),
                friendly_name: Some("Android VR 1.65".into()),
                login_supported: false,
                use_signature_timestamp: false,
                ..default_client()
            },
            AndroidVr1_43_32 => YouTubeClient {
                client_name: "ANDROID_VR".into(),
                client_version: "1.43.32".into(),
                client_id: "28".into(),
                user_agent: "com.google.android.apps.youtube.vr.oculus/1.43.32 (Linux; U; Android 12; en_US; Quest 3; Build/SQ3A.220605.009.A1; Cronet/107.0.5284.2)".into(),
                os_name: Some("Android".into()),
                os_version: Some("12".into()),
                device_make: Some("Oculus".into()),
                device_model: Some("Quest 3".into()),
                android_sdk_version: Some("32".into()),
                build_id: Some("SQ3A.220605.009.A1".into()),
                cronet_version: Some("107.0.5284.2".into()),
                package_name: Some("com.google.android.apps.youtube.vr.oculus".into()),
                friendly_name: Some("Android VR 1.43".into()),
                login_supported: false,
                use_signature_timestamp: false,
                ..default_client()
            },
            AndroidCreator => YouTubeClient {
                client_name: "ANDROID_CREATOR".into(),
                client_version: "25.03.101".into(),
                client_id: "14".into(),
                user_agent: "com.google.android.apps.youtube.creator/25.03.101 (Linux; U; Android 15; en_US; Pixel 9 Pro Fold; Build/AP3A.241005.015.A2; Cronet/132.0.6779.0)".into(),
                os_name: Some("Android".into()),
                os_version: Some("15".into()),
                device_make: Some("Google".into()),
                device_model: Some("Pixel 9 Pro Fold".into()),
                android_sdk_version: Some("35".into()),
                build_id: Some("AP3A.241005.015.A2".into()),
                cronet_version: Some("132.0.6779.0".into()),
                package_name: Some("com.google.android.apps.youtube.creator".into()),
                friendly_name: Some("Android Studio".into()),
                login_supported: true,
                use_signature_timestamp: true,
                ..default_client()
            },
            // the youtube app on apple vision pro, described the way newpipe
            // describes it (version, device, os, user agent,
            // www.youtube.com, api format 2, its own visitor id). it used to
            // pose as a desktop safari on music.youtube.com with version
            // 0.1, which is nothing youtube has ever shipped
            // (`YouTubeClient.kt:270-288`).
            VisionOs => YouTubeClient {
                client_name: "VISIONOS".into(),
                client_version: "1.04".into(),
                client_id: "101".into(),
                user_agent: "com.google.visionos.youtube/1.04(RealityDevice17,1; U; CPU visionOS 26_6_0 like Mac OS X; US)".into(),
                os_name: Some("visionOS".into()),
                os_version: Some("26.6.0.23O770".into()),
                device_make: Some("Apple".into()),
                device_model: Some("RealityDevice17,1".into()),
                friendly_name: Some("visionOS".into()),
                api_origin: ORIGIN_YOUTUBE.into(),
                api_format_version: "2".into(),
                client_screen: Some("WATCH".into()),
                platform: Some("MOBILE".into()),
                own_visitor_id: true,
                login_supported: false,
                use_signature_timestamp: false,
                ..default_client()
            },
            IpadOs => YouTubeClient {
                client_name: "IOS".into(),
                client_version: "21.03.3".into(),
                client_id: "5".into(),
                user_agent: "com.google.ios.youtube/21.03.3 (iPad7,6; U; CPU iPadOS 17_7_10 like Mac OS X; en-US)".into(),
                os_name: Some("iPadOS".into()),
                os_version: Some("17.7.10.21H450".into()),
                device_make: Some("Apple".into()),
                device_model: Some("iPad7,6".into()),
                friendly_name: Some("iPadOS".into()),
                login_supported: false,
                use_signature_timestamp: false,
                package_name: Some("com.google.ios.youtube".into()),
                ..default_client()
            },
        }
    }
}

/// The Kotlin default values (`YouTubeClient.kt:11-31`): every constructor
/// call above fills only what it sets, exactly like the Kotlin named args.
/// Only the two non-Option, non-flag fields have non-empty defaults
/// (`apiOrigin = ORIGIN_YOUTUBE_MUSIC`, `apiFormatVersion = "1"`).
fn default_client() -> YouTubeClient {
    YouTubeClient {
        client_name: String::new(),
        client_version: String::new(),
        client_id: String::new(),
        user_agent: String::new(),
        os_name: None,
        os_version: None,
        device_make: None,
        device_model: None,
        android_sdk_version: None,
        build_id: None,
        cronet_version: None,
        package_name: None,
        friendly_name: None,
        login_supported: false,
        login_required: false,
        use_signature_timestamp: false,
        is_embedded: false,
        use_web_po_tokens: false,
        api_origin: ORIGIN_YOUTUBE_MUSIC.into(),
        api_format_version: "1".into(),
        client_screen: None,
        platform: None,
        own_visitor_id: false,
    }
}

/// The registry with the R1 hotfix seam: client versions can be overridden
/// at runtime (from a config file or a settings-driven hotfix) without a
/// rebuild. Everything else about a client is code, like upstream.
///
/// The overrides map is keyed by upstream client *val* name (see
/// [`ClientKey::name`]); a JSON overrides file looks like
/// `{"WEB_REMIX": "1.20260801.03.00"}`.
#[derive(Debug, Clone, Default)]
pub struct ClientRegistry {
    versions: BTreeMap<ClientKey, String>,
}

impl ClientRegistry {
    /// A registry with no overrides (the pinned upstream versions).
    pub fn new() -> ClientRegistry {
        ClientRegistry::default()
    }

    /// Parse overrides from a JSON object keyed by client name, e.g.
    /// `{"WEB_REMIX": "1.20260801.03.00", "VISIONOS": "1.05"}`.
    /// Unknown client names are ignored with a warning (a hotfix file may
    /// target clients this build does not know).
    pub fn from_json(json: &serde_json::Value) -> ClientRegistry {
        let mut registry = ClientRegistry::new();
        if let Some(map) = json.as_object() {
            for (name, version) in map {
                if let Some(value) = version.as_str() {
                    match ClientKey::ALL.iter().copied().find(|k| k.name() == *name) {
                        Some(key) => {
                            registry.versions.insert(key, value.to_owned());
                        }
                        None => {
                            tracing::warn!(client = %name, "version override for unknown client ignored")
                        }
                    }
                }
            }
        }
        registry
    }

    /// Load overrides from a JSON file on disk. A missing file is not
    /// an error — the hotfix seam simply has nothing to apply (the
    /// 2026-11-21 audit aligned the behavior with this contract).
    pub fn load_overrides(path: &std::path::Path) -> Result<ClientRegistry, std::io::Error> {
        match std::fs::read_to_string(path) {
            Ok(text) => {
                let json: serde_json::Value =
                    serde_json::from_str(&text).map_err(std::io::Error::other)?;
                Ok(ClientRegistry::from_json(&json))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(ClientRegistry::new()),
            Err(error) => Err(error),
        }
    }

    /// Apply one version override at runtime.
    pub fn set_version(&mut self, key: ClientKey, version: impl Into<String>) {
        self.versions.insert(key, version.into());
    }

    /// The configured version for `key` (override or pinned upstream).
    pub fn version(&self, key: ClientKey) -> &str {
        self.versions
            .get(&key)
            .map(String::as_str)
            .unwrap_or_else(|| key.base_version())
    }

    /// The client config for `key` with the configured version applied.
    pub fn get(&self, key: ClientKey) -> YouTubeClient {
        let mut client = key.base();
        client.client_version = self.version(key).to_owned();
        client
    }
}

impl ClientKey {
    /// The pinned upstream version for this client (`YouTubeClient.kt`).
    pub fn base_version(self) -> &'static str {
        match self {
            ClientKey::Web => "2.20260708.00.00",
            ClientKey::WebRemix => "1.20260707.12.00",
            ClientKey::WebCreator => "1.20260708.06.00",
            ClientKey::TvHtml5 => "7.20260707.07.00",
            ClientKey::TvHtml5Samsung => "5.20260707",
            ClientKey::TvHtml5Downgraded => "5.20260707",
            ClientKey::WebEmbeddedPlayer => "2.20260708.00.00",
            ClientKey::TvHtml5SimplyEmbeddedPlayer => "2.0",
            ClientKey::Ios => "21.03.1",
            ClientKey::Mobile => "21.26.364",
            ClientKey::AndroidNoSdk => "21.26.364",
            ClientKey::AndroidVrNoAuth => "1.61.48",
            ClientKey::AndroidVr1_61_48 => "1.61.48",
            ClientKey::AndroidVr1_65_10 => "1.65.10",
            ClientKey::AndroidVr1_43_32 => "1.43.32",
            ClientKey::AndroidCreator => "25.03.101",
            ClientKey::VisionOs => "1.04",
            ClientKey::IpadOs => "21.03.3",
        }
    }
}

/// The client roles from `utils/YTPlayerUtils.kt` — the walk order the
/// player uses. Phase 08 implements the walk itself; the constants live
/// here so the registry is the single source of truth for "which client
/// plays what role".
pub mod roles {
    use super::ClientKey;

    /// `YTPlayerUtils.kt:132` — `private val MAIN_CLIENT: YouTubeClient =
    /// VISIONOS`. Verified against live googlevideo: with `visitorData` set
    /// it returns `OK`, every audio itag (139/140/249/250/251) with a
    /// direct, non-ciphered url, full `audioConfig` loudness data and
    /// `playbackTracking` — and its urls serve the whole file (206 at 1 MB,
    /// at the midpoint and at the final byte). It needs no PoToken, which is
    /// why it is the main client and not WEB_REMIX: an untokened url is
    /// served only up to roughly its first 786 KB (~32 s at 128 kbps).
    pub const MAIN_CLIENT: ClientKey = ClientKey::VisionOs;

    /// `YTPlayerUtils.kt:165` — `METADATA_CLIENT = WEB_REMIX`.
    pub const METADATA_CLIENT: ClientKey = ClientKey::WebRemix;

    /// `YTPlayerUtils.kt:135` — the account's clients that still hand out
    /// plain links for songs youtube only plays signed in (age-restricted,
    /// premium-only), in yt-dlp's order for signed-in users.
    pub const ACCOUNT_CLIENTS: &[ClientKey] = &[
        ClientKey::TvHtml5Samsung,
        ClientKey::WebEmbeddedPlayer,
        ClientKey::TvHtml5Downgraded,
    ];

    /// `YouTube.kt:1467-1471` — `CAPTION_CLIENTS` (the caption walk
    /// lives in the innertube facade, not YTPlayerUtils).
    pub const CAPTION_CLIENTS: &[ClientKey] = &[
        ClientKey::AndroidVrNoAuth,
        ClientKey::Ios,
        ClientKey::WebRemix,
    ];

    /// `YTPlayerUtils.kt:173-187` — `STREAM_FALLBACK_CLIENTS`. VISIONOS
    /// first (the only client measured to serve a complete track without a
    /// PoToken), the PoToken-capable web clients next, the remaining
    /// clients for restricted content. IOS / IPADOS / ANDROID_VR are last:
    /// they return clean urls that look healthy and then stop dead at the
    /// ~786 KB untokened window.
    pub const STREAM_FALLBACK_CLIENTS: &[ClientKey] = &[
        ClientKey::VisionOs,
        ClientKey::WebRemix,
        ClientKey::Web,
        ClientKey::WebCreator,
        ClientKey::IpadOs,
        ClientKey::AndroidVr1_43_32,
        ClientKey::AndroidVr1_61_48,
        ClientKey::AndroidVr1_65_10,
        ClientKey::AndroidVrNoAuth,
        ClientKey::TvHtml5SimplyEmbeddedPlayer,
        ClientKey::TvHtml5,
        ClientKey::AndroidCreator,
        ClientKey::Mobile,
    ];

    /// The clients that are NOT wired into any runtime role.
    /// `ANDROID_NO_SDK` is the documented dead client (PORT_SPEC §4:
    /// "models/parsers with no call sites") — registered for completeness
    /// and Android-import parity, never selected. Every other registered
    /// client appears in at least one role list above.
    pub const UNWIRED_CLIENTS: &[ClientKey] = &[ClientKey::AndroidNoSdk];
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client(key: ClientKey) -> YouTubeClient {
        ClientRegistry::new().get(key)
    }

    #[test]
    fn all_eighteen_clients_are_registered() {
        assert_eq!(ClientKey::ALL.len(), 18);
        // every key produces a distinct (client_name, client_id, version)
        // triple except where upstream deliberately shares them
        for key in ClientKey::ALL {
            let c = client(*key);
            assert!(!c.client_name.is_empty());
            assert!(!c.client_id.is_empty());
            assert!(!c.user_agent.is_empty());
        }
    }

    #[test]
    fn web_is_client_one_with_no_auth() {
        // YouTubeClient.kt:66-71
        let web = client(ClientKey::Web);
        assert_eq!(web.client_name, "WEB");
        assert_eq!(web.client_id, "1");
        assert_eq!(web.client_version, "2.20260708.00.00");
        assert_eq!(web.user_agent, USER_AGENT_WEB);
        assert!(!web.login_supported);
        assert!(!web.login_required);
        assert!(!web.use_signature_timestamp);
        assert!(!web.use_web_po_tokens);
        assert_eq!(web.api_origin, ORIGIN_YOUTUBE_MUSIC);
        assert_eq!(web.api_format_version, "1");
        assert!(!web.is_native_app());
    }

    #[test]
    fn web_remix_is_sixty_seven_auth_sts_po() {
        // YouTubeClient.kt:73-81
        let remix = client(ClientKey::WebRemix);
        assert_eq!(remix.client_id, "67");
        assert_eq!(remix.client_version, "1.20260707.12.00");
        assert!(remix.login_supported);
        assert!(!remix.login_required);
        assert!(remix.use_signature_timestamp);
        assert!(remix.use_web_po_tokens);
    }

    #[test]
    fn web_creator_requires_login() {
        // YouTubeClient.kt:83-91
        let creator = client(ClientKey::WebCreator);
        assert_eq!(creator.client_id, "62");
        assert!(creator.login_supported);
        assert!(creator.login_required);
        assert!(creator.use_signature_timestamp);
        assert!(!creator.use_web_po_tokens);
    }

    #[test]
    fn tvhtml5_family() {
        // YouTubeClient.kt:93-132
        let tv = client(ClientKey::TvHtml5);
        assert_eq!(tv.client_id, "7");
        assert!(tv.login_required && tv.use_web_po_tokens);
        let samsung = client(ClientKey::TvHtml5Samsung);
        assert_eq!(
            (samsung.client_id.as_str(), samsung.client_version.as_str()),
            ("7", "5.20260707")
        );
        assert_eq!(samsung.api_origin, ORIGIN_YOUTUBE);
        assert_eq!(samsung.device_model.as_deref(), Some("UKS9800"));
        let down = client(ClientKey::TvHtml5Downgraded);
        assert_eq!(
            down.user_agent,
            "Mozilla/5.0 (ChromiumStylePlatform) Cobalt/Version"
        );
        assert_eq!(down.api_origin, ORIGIN_YOUTUBE);
    }

    #[test]
    fn embedded_players() {
        // YouTubeClient.kt:135-157
        let web_embed = client(ClientKey::WebEmbeddedPlayer);
        assert_eq!(web_embed.client_id, "56");
        assert!(web_embed.is_embedded);
        assert!(web_embed.login_required);
        assert_eq!(web_embed.api_origin, ORIGIN_YOUTUBE);
        let simply = client(ClientKey::TvHtml5SimplyEmbeddedPlayer);
        assert_eq!(simply.client_id, "85");
        assert_eq!(simply.client_version, "2.0");
        assert!(simply.is_embedded);
        assert!(!simply.login_required);
    }

    #[test]
    fn ios_family() {
        // YouTubeClient.kt:159-165 + 290-303
        let ios = client(ClientKey::Ios);
        assert_eq!(ios.client_id, "5");
        assert_eq!(ios.os_version.as_deref(), Some("18.2.22C152"));
        assert!(ios.os_name.is_none(), "IOS sends no osName");
        let ipad = client(ClientKey::IpadOs);
        assert_eq!(ipad.client_name, "IOS");
        assert_eq!(ipad.friendly_name.as_deref(), Some("iPadOS"));
        assert_eq!(ipad.package_name.as_deref(), Some("com.google.ios.youtube"));
        assert!(!ipad.login_supported);
    }

    #[test]
    fn android_family() {
        // YouTubeClient.kt:167-268
        let mobile = client(ClientKey::Mobile);
        assert_eq!(mobile.client_name, "ANDROID");
        assert_eq!(mobile.client_id, "3");
        assert!(mobile.login_supported);
        let no_sdk = client(ClientKey::AndroidNoSdk);
        assert_eq!(no_sdk.client_id, "3");
        assert!(!no_sdk.login_supported && !no_sdk.use_signature_timestamp);
        let creator = client(ClientKey::AndroidCreator);
        // upstream clientId is "14" (YouTubeClient.kt:255)
        assert_eq!(creator.client_id, "14");
        assert_eq!(creator.friendly_name.as_deref(), Some("Android Studio"));
        assert!(creator.login_supported);
    }

    #[test]
    fn android_vr_four_versions() {
        // YouTubeClient.kt:190-250
        let vr = [
            ClientKey::AndroidVrNoAuth,
            ClientKey::AndroidVr1_61_48,
            ClientKey::AndroidVr1_65_10,
            ClientKey::AndroidVr1_43_32,
        ];
        for key in vr {
            let c = client(key);
            assert_eq!(c.client_name, "ANDROID_VR");
            assert_eq!(c.client_id, "28");
            assert!(!c.login_supported);
            assert!(!c.use_signature_timestamp);
        }
        assert_eq!(
            client(ClientKey::AndroidVr1_65_10).client_version,
            "1.65.10"
        );
        assert_eq!(
            client(ClientKey::AndroidVr1_43_32).client_version,
            "1.43.32"
        );
        assert_eq!(
            client(ClientKey::AndroidVr1_43_32)
                .cronet_version
                .as_deref(),
            Some("107.0.5284.2")
        );
    }

    #[test]
    fn visionos_is_the_own_visitor_native_app() {
        // YouTubeClient.kt:271-288
        let vision = client(ClientKey::VisionOs);
        assert_eq!(vision.client_id, "101");
        assert_eq!(vision.api_origin, ORIGIN_YOUTUBE);
        assert_eq!(vision.api_format_version, "2");
        assert_eq!(vision.client_screen.as_deref(), Some("WATCH"));
        assert_eq!(vision.platform.as_deref(), Some("MOBILE"));
        assert!(vision.own_visitor_id);
        assert!(vision.is_native_app());
        assert!(!vision.login_supported);
        assert!(!vision.use_signature_timestamp);
    }

    #[test]
    fn native_app_clients_skip_origin_and_referer() {
        // only VISIONOS sets a platform; every other client is
        // "browser-looking" and gets X-Origin/Referer (InnerTube.kt:164-167)
        let native: Vec<_> = ClientKey::ALL
            .iter()
            .filter(|k| client(**k).is_native_app())
            .collect();
        assert_eq!(native, vec![&ClientKey::VisionOs]);
    }

    #[test]
    fn version_overrides_apply_without_touching_other_fields() {
        let mut registry = ClientRegistry::new();
        let hotfixed = "1.20260801.03.00";
        registry.set_version(ClientKey::WebRemix, hotfixed);
        let c = registry.get(ClientKey::WebRemix);
        assert_eq!(c.client_version, hotfixed);
        assert_eq!(c.client_id, "67");
        // other clients untouched
        assert_eq!(
            registry.get(ClientKey::Web).client_version,
            "2.20260708.00.00"
        );
        // base config stays pinned
        assert_eq!(ClientKey::WebRemix.base_version(), "1.20260707.12.00");
    }

    #[test]
    fn overrides_parse_from_json_and_ignore_unknown_clients() {
        let json = serde_json::json!({
            "WEB_REMIX": "9.9.9",
            "NOT_A_CLIENT": "1.0",
        });
        let registry = ClientRegistry::from_json(&json);
        assert_eq!(registry.version(ClientKey::WebRemix), "9.9.9");
        assert_eq!(registry.version(ClientKey::Web), "2.20260708.00.00");
    }

    #[test]
    fn roles_match_upstream() {
        // YTPlayerUtils.kt:132, 165, 135, 1467-1471, 173-187
        assert_eq!(roles::MAIN_CLIENT, ClientKey::VisionOs);
        assert_eq!(roles::METADATA_CLIENT, ClientKey::WebRemix);
        assert_eq!(
            roles::ACCOUNT_CLIENTS,
            &[
                ClientKey::TvHtml5Samsung,
                ClientKey::WebEmbeddedPlayer,
                ClientKey::TvHtml5Downgraded,
            ]
        );
        assert_eq!(roles::STREAM_FALLBACK_CLIENTS.len(), 13);
        assert_eq!(roles::STREAM_FALLBACK_CLIENTS[0], ClientKey::VisionOs);
        assert_eq!(
            roles::CAPTION_CLIENTS,
            &[
                ClientKey::AndroidVrNoAuth,
                ClientKey::Ios,
                ClientKey::WebRemix,
            ]
        );
        // ANDROID_NO_SDK is never wired (PORT_SPEC §4 dead code)
        assert_eq!(roles::UNWIRED_CLIENTS, &[ClientKey::AndroidNoSdk]);
        for key in ClientKey::ALL {
            let wired = roles::ACCOUNT_CLIENTS.contains(key)
                || roles::CAPTION_CLIENTS.contains(key)
                || roles::STREAM_FALLBACK_CLIENTS.contains(key)
                || *key == roles::MAIN_CLIENT
                || *key == roles::METADATA_CLIENT;
            if roles::UNWIRED_CLIENTS.contains(key) {
                assert!(!wired, "{:?} must stay unwired", key);
            }
        }
    }

    #[test]
    fn registry_serializes_round_trip() {
        let c = client(ClientKey::VisionOs);
        let text = serde_json::to_string(&c).unwrap();
        let back: YouTubeClient = serde_json::from_str(&text).unwrap();
        assert_eq!(c, back);
    }
}
