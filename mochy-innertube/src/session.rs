//! Session state and the settings mirror.
//!
//! Upstream the whole session lives as mutable fields on the `YouTube`
//! object + `InnerTube` singleton (YouTube.kt:78-129), fed by the
//! `initializeSettings` one-shot (App.kt:187-235) and the
//! `observeSettingsChanges` collectors (App.kt:236-330). The desktop port
//! keeps the state in one [`Session`] behind an `Arc<RwLock<..>>`
//! (short critical sections, never held across an await) and provides
//! [`SessionMirror`], the one-shot + watcher-driven push of settings and
//! vault values into the session.
//!
//! Secrets live in `mochy-vault` on desktop (Phase 02 decision F-004:
//! the 12 Android secret keys round-trip the settings registry but never
//! serialize into `settings.json`); the mirror reads
//! `innerTubeCookie`/`visitorData`/`dataSyncId` from there and writes a
//! freshly scraped `visitorData` back on rotation.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use mochy_config::{SettingId, SettingValue, Settings, SettingsStore, WatchEvent};
use mochy_net::{IpVersion, ProxyConfig, ProxyKind};
use mochy_vault::{Vault, VaultKey};

use crate::auth::parse_cookie_string;
use crate::context::YouTubeLocale;

/// The shared session cell. Every transport request clones what it needs
/// out of one short-lived lock; nothing holds the guard across an await.
pub type SharedSession = Arc<RwLock<Session>>;

/// The mutable per-app YouTube session — the fields of `InnerTube.kt`
/// (`locale`, `visitorData`, `dataSyncId`, `cookie` + `cookieMap`,
/// `proxy`, `proxyAuth`, `ipVersion`, `useLoginForBrowse`, `offline`) and
/// `YouTube.kt:78-116`'s passthrough accessors, collected into one
/// struct.
#[derive(Debug, Clone)]
pub struct Session {
    /// `InnerTube.kt:40-43` (system locale default fed by the mirror).
    pub locale: YouTubeLocale,
    /// `InnerTube.kt:44` — the guest session id (vault-owned on desktop).
    pub visitor_data: Option<String>,
    /// `InnerTube.kt:45` — the signed-in account sync id.
    pub data_sync_id: Option<String>,
    /// `InnerTube.kt:46-51` — the raw cookie string + its parsed map
    /// (the setter re-parses; upstream throws on garbage and the app
    /// forgets the account — the port logs and clears instead, see
    /// `set_cookie`).
    pub cookie: Option<String>,
    pub cookie_map: HashMap<String, String>,
    /// `InnerTube.kt:53-58` (proxy setter rebuilds the http client).
    pub proxy: Option<ProxyConfig>,
    /// `InnerTube.kt:62-67` (ipVersion setter rebuilds the http client).
    pub ip_version: IpVersion,
    /// `InnerTube.kt:69` — browse/search send login headers when on.
    pub use_login_for_browse: bool,
    /// `InnerTube.kt:71-73` — every request fails fast when on.
    pub offline: bool,
    /// `InnerTube.kt:184-202` — visitor ids minted by the clients that
    /// bring their own (VISIONOS), keyed by client name.
    pub own_visitor_ids: HashMap<String, String>,
}

impl Default for Session {
    fn default() -> Self {
        Session {
            locale: YouTubeLocale::default(),
            visitor_data: None,
            data_sync_id: None,
            cookie: None,
            cookie_map: HashMap::new(),
            proxy: None,
            ip_version: IpVersion::Auto,
            use_login_for_browse: false,
            offline: false,
            own_visitor_ids: HashMap::new(),
        }
    }
}

impl Session {
    /// The cookie setter (`InnerTube.kt:46-51`): stores the string and
    /// re-parses the map. Upstream's parse cannot fail
    /// (`parseCookieString` never throws); the port keeps a Result shape
    /// anyway so a future stricter parser has somewhere to signal, and
    /// the failure mode matches the App.kt:268-274 collector ("could not
    /// parse cookie" -> forget the account) by clearing the value.
    pub fn set_cookie(&mut self, cookie: Option<String>) {
        self.cookie_map = cookie
            .as_deref()
            .map(parse_cookie_string)
            .unwrap_or_default();
        self.cookie = cookie;
    }

    /// `YouTube.kt:138-141` (`clearGuestSession`) — sign-out forgets the
    /// guest session id and the account sync id.
    pub fn clear_guest_session(&mut self) {
        self.visitor_data = None;
        self.data_sync_id = None;
    }

    /// Whether a signed-in cookie is present (`YouTube.cookie != null`).
    pub fn is_logged_in(&self) -> bool {
        self.cookie.is_some()
    }
}

/// The [`Session`] plus the snapshot of proxy/ip_version the current http
/// client was built from — when they change the transport rebuilds
/// (InnerTube.kt:53-67 setter side effects).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ClientSeed {
    pub proxy: Option<ProxyConfig>,
    pub ip_version: IpVersion,
}

/// The runtime settings mirror — the port of `App.kt`'s
/// `initializeSettings` (187-235) + `observeSettingsChanges` (236-330).
///
/// Upstream runs nine eternal collectors on the application scope. The
/// port runs ONE pump over [`SettingsStore::watch_all`] (std mpsc) on a
/// dedicated thread, plus the vault poll for the three secret keys
/// (keyring has no change notification; the vault's in-memory cache makes
/// the poll cheap, and secrets change only through this app).
pub struct SessionMirror;

/// The system locale the mirror falls back to when the content
/// preferences are `SYSTEM_DEFAULT` (App.kt:196-205/277-291:
/// `AppLanguage.systemLocale`). The app shell injects the OS locale at
/// startup; `(US, en)` is the upstream hard floor.
#[derive(Debug, Clone)]
pub struct SystemLocale {
    pub country: String,
    pub language_tag: String,
}

impl Default for SystemLocale {
    fn default() -> Self {
        SystemLocale {
            country: "US".into(),
            language_tag: "en".into(),
        }
    }
}

/// `SYSTEM_DEFAULT` sentinel (constants/PreferenceKeys read sites).
const SYSTEM_DEFAULT: &str = "SYSTEM_DEFAULT";

impl SessionMirror {
    /// The one-shot sync — `App.kt initializeSettings` (187-235):
    /// offline flag, locale, proxy, useLoginForBrowse, ipVersion, then
    /// the three secrets from the vault. When `visitorData` is absent
    /// (or the literal `"null"`) upstream scrapes one and persists it
    /// (App.kt:240-247); the scrape needs the network, so it returns the
    /// pending action for the caller to run on the transport.
    pub fn initialize(
        store: &SettingsStore,
        vault: &Vault,
        session: &SharedSession,
        system: &SystemLocale,
    ) -> Result<Option<GuestScrapeNeeded>, MirrorError> {
        let settings = store.snapshot();
        let mut scrape = None;
        {
            let mut guard = session.write().map_err(|_| MirrorError::Poisoned)?;
            guard.offline = settings.OfflineMode();
            guard.locale = resolve_locale(&settings, system);
            guard.use_login_for_browse = settings.UseLoginForBrowse();
            guard.ip_version = ip_version_from(&settings);
            guard.proxy = proxy_from(&settings, vault);
            // App.kt:240-247 — visitorData, with the literal "null" treated
            // as absent
            let stored = vault.get(VaultKey::VisitorData);
            match stored.filter(|v| !v.is_empty() && v != "null") {
                Some(visitor) => guard.visitor_data = Some(visitor),
                None => scrape = Some(GuestScrapeNeeded),
            }

            // App.kt:253-260 — dataSyncId "||" split
            if let Some(sync) = vault.get(VaultKey::DataSyncId).filter(|v| !v.is_empty()) {
                guard.data_sync_id = Some(normalize_data_sync_id(&sync));
            }

            // App.kt:268-274 — cookie; a parse failure forgets the account
            // (here: clears the session copy; the app-level forget is
            // Phase 05+'s hook)
            if let Some(cookie) = vault
                .get(VaultKey::InnerTubeCookie)
                .filter(|v| !v.is_empty())
            {
                guard.set_cookie(Some(cookie));
            }
        }
        // offline mirrors into the shared gate (App.kt:191,
        // utils/OfflineMode.set)
        mochy_net::set_offline(settings.OfflineMode());
        // the data-saver gate mirrors the same way (utils/DataSaver.kt
        // reads the store per call upstream; the desktop gate is fed by
        // this pump — App.kt's settings observers are the seam)
        mochy_net::apply_data_saver_settings(settings.DataSaver(), settings.DataSaverMobileOnly());
        Ok(scrape)
    }

    /// Subscribe to every settings change — the channel the pump (and
    /// deterministic tests) consume. Keep the [`mochy_config::WatchGuard`]
    /// alive or the subscription ends.
    pub fn subscribe(
        store: &SettingsStore,
    ) -> Result<
        (
            std::sync::mpsc::Receiver<WatchEvent>,
            mochy_config::WatchGuard,
        ),
        MirrorError,
    > {
        store.watch_all().map_err(|_| MirrorError::WatchFailed)
    }

    /// Apply one settings event — the body of one upstream collector.
    /// Each `SettingId` is mapped exactly like the upstream collector for
    /// that key (see the match arms). The guest scrape runs through
    /// `on_scrape` (async, network) — the handler itself stays IO-free
    /// apart from the vault.
    pub fn handle_one(
        event: &WatchEvent,
        store: &SettingsStore,
        vault: &Vault,
        session: &SharedSession,
        system: &SystemLocale,
        on_scrape: &dyn Fn(&SharedSession),
    ) -> Result<(), MirrorError> {
        let WatchEvent::AnyChanged { id, value } = event else {
            return Ok(());
        };
        match id {
            SettingId::OfflineMode => {
                if let SettingValue::Bool(on) = value {
                    mochy_net::set_offline(*on);
                    if let Ok(mut guard) = session.write() {
                        guard.offline = *on;
                    }
                }
            }
            SettingId::DataSaver | SettingId::DataSaverMobileOnly => {
                // DataSaver.kt reads the store live upstream; the desktop
                // gate is atomic, so push both halves on either change
                let settings = store.snapshot();
                mochy_net::apply_data_saver_settings(
                    settings.DataSaver(),
                    settings.DataSaverMobileOnly(),
                );
            }
            SettingId::VisitorData => {
                // App.kt:240-247: absent/"null" -> scrape + persist
                match value {
                    SettingValue::Str(v) if !v.is_empty() && v != "null" => {
                        if let Ok(mut guard) = session.write() {
                            guard.visitor_data = Some(v.clone());
                        }
                    }
                    _ => on_scrape(session),
                }
            }
            SettingId::DataSyncId => {
                // App.kt:253-260
                if let SettingValue::Str(v) = value
                    && let Ok(mut guard) = session.write()
                {
                    guard.data_sync_id = if v.is_empty() {
                        None
                    } else {
                        Some(normalize_data_sync_id(v))
                    };
                }
            }
            SettingId::InnerTubeCookie => {
                // App.kt:268-274
                if let SettingValue::Str(v) = value
                    && let Ok(mut guard) = session.write()
                {
                    match v.is_empty() {
                        true => guard.set_cookie(None),
                        false => guard.set_cookie(Some(v.clone())),
                    };
                }
            }
            SettingId::ContentCountry | SettingId::ContentLanguage => {
                // App.kt:277-291 — UI language must not silently
                // change content preferences
                let settings = store.snapshot();
                if let Ok(mut guard) = session.write() {
                    guard.locale = resolve_locale(&settings, system);
                }
            }
            SettingId::ProxyEnabled
            | SettingId::ProxyUrl
            | SettingId::ProxyType
            | SettingId::ProxyUsername => {
                // App.kt:300-306 — a new way out gets a fresh guest
                // session too, the old one may be what youtube flagged
                let settings = store.snapshot();
                let new_proxy = proxy_from(&settings, vault);
                if let Ok(mut guard) = session.write() {
                    let changed = guard.proxy != new_proxy;
                    guard.proxy = new_proxy;
                    let signed_out = guard.cookie.is_none();
                    drop(guard);
                    if changed && signed_out {
                        on_scrape(session);
                    }
                }
            }
            SettingId::IpVersion => {
                // App.kt:314-319
                let settings = store.snapshot();
                if let Ok(mut guard) = session.write() {
                    guard.ip_version = ip_version_from(&settings);
                }
            }
            SettingId::UseLoginForBrowse => {
                // (the collector for this key rides initializeSettings
                // upstream; the port watches it live)
                if let SettingValue::Bool(on) = value
                    && let Ok(mut guard) = session.write()
                {
                    guard.use_login_for_browse = *on;
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// The watcher pump — `App.kt observeSettingsChanges` (236-330) as
    /// one eternal loop over [`SettingsStore::watch_all`]. Runs forever
    /// on the calling thread (spawn it).
    pub fn pump(
        store: SettingsStore,
        vault: Vault,
        session: SharedSession,
        system: SystemLocale,
        on_scrape: impl Fn(&SharedSession) + Send + 'static,
    ) -> Result<(), MirrorError> {
        let (receiver, _guard) = Self::subscribe(&store)?;
        for event in receiver {
            Self::handle_one(&event, &store, &vault, &session, &system, &on_scrape)?;
        }
        Ok(())
    }
}

/// Marker returned by [`SessionMirror::initialize`] when the stored
/// visitor data is absent and a guest scrape should run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GuestScrapeNeeded;

/// App.kt:196-205 + 277-291 — content country/language with
/// `SYSTEM_DEFAULT` falling back to the system locale, then the hard
/// floors `"US"`/`"en"`.
fn resolve_locale(settings: &Settings, system: &SystemLocale) -> YouTubeLocale {
    let country = settings.ContentCountry();
    let gl = if country != SYSTEM_DEFAULT && !country.is_empty() {
        country
    } else if !system.country.is_empty() {
        system.country.clone()
    } else {
        "US".into()
    };
    let language = settings.ContentLanguage();
    let hl = if language != SYSTEM_DEFAULT && !language.is_empty() {
        language
    } else if !system.language_tag.is_empty() {
        system.language_tag.clone()
    } else {
        "en".into()
    };
    YouTubeLocale { gl, hl }
}

/// App.kt:253-260 — the dataSyncId `"||"` handling:
/// no `"||"` at all -> as-is; ends with `"||"` -> the part before;
/// otherwise -> the part after.
pub fn normalize_data_sync_id(raw: &str) -> String {
    if !raw.contains("||") {
        raw.to_owned()
    } else if let Some(before) = raw.strip_suffix("||") {
        before.to_owned()
    } else {
        raw.split_once("||")
            .map(|(_, after)| after)
            .unwrap_or(raw)
            .to_owned()
    }
}

/// `AppProxy.configFrom(settings)` (utils/AppProxy.kt:57-67): `None`
/// unless enabled, `None` when the address does not parse; the password
/// comes from the vault (`ProxyPassword` secret key).
fn proxy_from(settings: &Settings, vault: &Vault) -> Option<ProxyConfig> {
    if !settings.ProxyEnabled() {
        return None;
    }
    let (host, port) = ProxyConfig::parse_address(&settings.ProxyUrl())?;
    let mut config = ProxyConfig {
        kind: ProxyKind::from_pref(settings.ProxyType().variant_name()),
        host,
        port,
        username: settings.ProxyUsername(),
        password: String::new(),
    };
    if let Some(password) = vault.get(VaultKey::ProxyPassword) {
        config.password = password;
    }
    Some(config)
}

/// `IpVersionKey.toEnum(defaultValue = AUTO)` (App.kt:225/316).
fn ip_version_from(settings: &Settings) -> IpVersion {
    match settings.IpVersion().variant_name() {
        "IPV4" => IpVersion::Ipv4,
        "IPV6" => IpVersion::Ipv6,
        _ => IpVersion::Auto,
    }
}

/// Mirror failures.
#[derive(Debug, thiserror::Error)]
pub enum MirrorError {
    #[error("session lock poisoned")]
    Poisoned,
    #[error("could not subscribe to settings changes")]
    WatchFailed,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_sync_id_pipe_handling() {
        // App.kt:253-260
        assert_eq!(normalize_data_sync_id("plain"), "plain");
        assert_eq!(normalize_data_sync_id("prefix||"), "prefix");
        assert_eq!(normalize_data_sync_id("prefix||suffix"), "suffix");
        assert_eq!(normalize_data_sync_id("a||b||c"), "b||c");
    }

    #[test]
    fn cookie_setter_reparses_the_map() {
        // InnerTube.kt:46-51
        let mut session = Session::default();
        session.set_cookie(Some("SAPISID=abc; VISITOR=x".into()));
        assert_eq!(
            session.cookie_map.get("SAPISID").map(String::as_str),
            Some("abc")
        );
        assert!(session.is_logged_in());
        session.set_cookie(None);
        assert!(session.cookie_map.is_empty());
        assert!(!session.is_logged_in());
    }

    #[test]
    fn clear_guest_session_forgets_both_ids() {
        // YouTube.kt:138-141
        let mut session = Session {
            visitor_data: Some("Cgt1".into()),
            data_sync_id: Some("sync".into()),
            ..Session::default()
        };
        session.clear_guest_session();
        assert!(session.visitor_data.is_none() && session.data_sync_id.is_none());
    }
}
