//! The settings mirror test — `App.kt initializeSettings` (187-235) +
//! `observeSettingsChanges` (236-330) semantics, offline: a temp
//! SettingsStore, an in-memory vault, and one pump thread.

mod common;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use mochy_config::{SettingId, SettingValue, SettingsStore};
use mochy_innertube::session::{
    Session, SessionMirror, SharedSession, SystemLocale, normalize_data_sync_id,
};
use mochy_vault::{Vault, VaultKey};

fn temp_store() -> SettingsStore {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    // keep the dir alive for the process lifetime (the store only reads
    // at load; persistence goes to the same path)
    std::mem::forget(dir);
    SettingsStore::load(&path)
}

#[test]
fn initialize_mirrors_settings_and_vault_into_the_session() {
    let store = temp_store();
    let vault = Vault::open_in_memory();
    let session: SharedSession = Arc::new(RwLock::new(Session::default()));

    // vault-owned session secrets
    vault
        .set(VaultKey::VisitorData, "CgtSTOREDVISITOR")
        .unwrap();
    vault.set(VaultKey::DataSyncId, "prefix||suffix").unwrap();
    vault
        .set(VaultKey::InnerTubeCookie, "SAPISID=abc; __Secure-YEC=Ok")
        .unwrap();

    // registry-owned session inputs (upstream App.kt:214-225)
    store
        .set_by_id(SettingId::UseLoginForBrowse, SettingValue::Bool(true))
        .unwrap();
    store
        .set_by_id(SettingId::IpVersion, SettingValue::Str("IPV6".into()))
        .unwrap();
    store
        .set_by_id(SettingId::ContentCountry, SettingValue::Str("DE".into()))
        .unwrap();

    let scrape = SessionMirror::initialize(
        &store,
        &vault,
        &session,
        &SystemLocale {
            country: "FR".into(),
            language_tag: "fr".into(),
        },
    )
    .unwrap();

    // visitor data present -> no scrape needed (App.kt:240-247)
    assert!(scrape.is_none());
    let guard = session.read().unwrap();
    assert_eq!(guard.visitor_data.as_deref(), Some("CgtSTOREDVISITOR"));
    // the "||" split (App.kt:253-260)
    assert_eq!(guard.data_sync_id.as_deref(), Some("suffix"));
    // cookie + parsed map (InnerTube.kt:46-51)
    assert!(guard.cookie.is_some());
    assert_eq!(
        guard.cookie_map.get("SAPISID").map(String::as_str),
        Some("abc")
    );
    assert!(guard.use_login_for_browse);
    assert_eq!(guard.ip_version, mochy_net::IpVersion::Ipv6);
    // content country beats the system locale (App.kt:277-291)
    assert_eq!(guard.locale.gl, "DE");
    assert_eq!(guard.locale.hl, "fr");
    drop(guard);
}

#[test]
fn initialize_flags_a_scrape_when_visitor_data_missing() {
    let store = temp_store();
    let vault = Vault::open_in_memory();
    let session: SharedSession = Arc::new(RwLock::new(Session::default()));
    let scrape =
        SessionMirror::initialize(&store, &vault, &session, &SystemLocale::default()).unwrap();
    assert!(scrape.is_some(), "absent visitorData must request a scrape");

    // the literal "null" is absent too (App.kt:240-241)
    vault.set(VaultKey::VisitorData, "null").unwrap();
    let scrape =
        SessionMirror::initialize(&store, &vault, &session, &SystemLocale::default()).unwrap();
    assert!(scrape.is_some());
}

#[test]
fn system_default_locale_falls_back_to_the_system() {
    let store = temp_store();
    let vault = Vault::open_in_memory();
    let session: SharedSession = Arc::new(RwLock::new(Session::default()));
    SessionMirror::initialize(
        &store,
        &vault,
        &session,
        &SystemLocale {
            country: "JP".into(),
            language_tag: "ja".into(),
        },
    )
    .unwrap();
    let guard = session.read().unwrap();
    assert_eq!(
        (guard.locale.gl.as_str(), guard.locale.hl.as_str()),
        ("JP", "ja")
    );
}

#[test]
fn watchers_push_changes_into_the_live_session() {
    let store = temp_store();
    let vault = Vault::open_in_memory();
    let session: SharedSession = Arc::new(RwLock::new(Session::default()));
    SessionMirror::initialize(&store, &vault, &session, &SystemLocale::default()).unwrap();

    // subscribe FIRST (deterministic: no event can be missed)
    let (receiver, _guard) = SessionMirror::subscribe(&store).unwrap();
    let scrape_called = Arc::new(AtomicBool::new(false));
    let scrape_flag = Arc::clone(&scrape_called);
    let drain = |receiver: &std::sync::mpsc::Receiver<mochy_config::WatchEvent>| {
        while let Ok(event) = receiver.try_recv() {
            SessionMirror::handle_one(
                &event,
                &store,
                &vault,
                &session,
                &SystemLocale::default(),
                &|_session| scrape_flag.store(true, Ordering::SeqCst),
            )
            .unwrap();
        }
    };

    // a cookie change lands in the session (App.kt:268-274)
    store
        .set_by_id(
            SettingId::InnerTubeCookie,
            SettingValue::Str("SAPISID=xyz; LOGIN_INFO=1".into()),
        )
        .unwrap();
    drain(&receiver);
    assert_eq!(
        session
            .read()
            .unwrap()
            .cookie_map
            .get("SAPISID")
            .map(String::as_str),
        Some("xyz")
    );

    // a locale change re-resolves (App.kt:277-291)
    store
        .set_by_id(SettingId::ContentLanguage, SettingValue::Str("ja".into()))
        .unwrap();
    drain(&receiver);
    assert_eq!(session.read().unwrap().locale.hl, "ja");

    // clearing visitor data requests a scrape (App.kt:240-247)
    store
        .set_by_id(SettingId::VisitorData, SettingValue::Str(String::new()))
        .unwrap();
    drain(&receiver);
    assert!(scrape_called.load(Ordering::SeqCst));

    // a stored visitor data lands verbatim
    store
        .set_by_id(SettingId::VisitorData, SettingValue::Str("CgtNEW".into()))
        .unwrap();
    drain(&receiver);
    assert_eq!(
        session.read().unwrap().visitor_data.as_deref(),
        Some("CgtNEW")
    );

    // ip version (App.kt:314-319)
    store
        .set_by_id(SettingId::IpVersion, SettingValue::Str("IPV4".into()))
        .unwrap();
    drain(&receiver);
    assert_eq!(
        session.read().unwrap().ip_version,
        mochy_net::IpVersion::Ipv4
    );

    // useLoginForBrowse (App.kt:214)
    store
        .set_by_id(SettingId::UseLoginForBrowse, SettingValue::Bool(true))
        .unwrap();
    drain(&receiver);
    assert!(session.read().unwrap().use_login_for_browse);

    // dataSyncId with the || split (App.kt:253-260)
    store
        .set_by_id(SettingId::DataSyncId, SettingValue::Str("pre||post".into()))
        .unwrap();
    drain(&receiver);
    assert_eq!(
        session.read().unwrap().data_sync_id.as_deref(),
        Some("post")
    );

    // offline mirrors into the global gate AND the session
    // (App.kt:236-241)
    store
        .set_by_id(SettingId::OfflineMode, SettingValue::Bool(true))
        .unwrap();
    drain(&receiver);
    assert!(mochy_net::OfflineFlag.is_on());
    assert!(session.read().unwrap().offline);
    mochy_net::set_offline(false);
}

#[test]
fn data_sync_id_normalization() {
    // App.kt:253-260, three branches
    assert_eq!(normalize_data_sync_id("plain"), "plain");
    assert_eq!(normalize_data_sync_id("prefix||"), "prefix");
    assert_eq!(normalize_data_sync_id("prefix||suffix"), "suffix");
}

#[test]
fn proxy_change_with_signed_out_session_triggers_rotation() {
    // App.kt:300-306 — "a new way out gets a fresh guest session too,
    // the old one may be what youtube flagged"
    let store = temp_store();
    let vault = Vault::open_in_memory();
    let session: SharedSession = Arc::new(RwLock::new(Session::default()));
    SessionMirror::initialize(&store, &vault, &session, &SystemLocale::default()).unwrap();

    let (receiver, _guard) = SessionMirror::subscribe(&store).unwrap();
    let rotations = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = Arc::clone(&rotations);
    let drain = |receiver: &std::sync::mpsc::Receiver<mochy_config::WatchEvent>| {
        while let Ok(event) = receiver.try_recv() {
            SessionMirror::handle_one(
                &event,
                &store,
                &vault,
                &session,
                &SystemLocale::default(),
                &|_session| {
                    count.fetch_add(1, Ordering::SeqCst);
                },
            )
            .unwrap();
        }
    };

    store
        .set_by_id(SettingId::ProxyEnabled, SettingValue::Bool(true))
        .unwrap();
    store
        .set_by_id(
            SettingId::ProxyUrl,
            SettingValue::Str("127.0.0.1:9050".into()),
        )
        .unwrap();
    store
        .set_by_id(SettingId::ProxyType, SettingValue::Str("SOCKS".into()))
        .unwrap();
    drain(&receiver);

    let guard = session.read().unwrap();
    let proxy = guard
        .proxy
        .as_ref()
        .expect("proxy must land in the session");
    assert_eq!(proxy.kind, mochy_net::ProxyKind::Socks);
    assert_eq!(proxy.host, "127.0.0.1");
    assert_eq!(proxy.port, 9050);
    assert!(
        rotations.load(Ordering::SeqCst) >= 1,
        "a signed-out proxy change must rotate the guest session"
    );
}

/// Phase 07: the mirror also feeds the data-saver gate
/// (utils/DataSaver.kt reads the store live on Android; the desktop gate
/// is atomic and this pump is the equivalent of the store read).
#[test]
fn initialize_and_watchers_feed_the_data_saver_gate() {
    let store = temp_store();
    let vault = Vault::open_in_memory();
    let session: SharedSession = Arc::new(RwLock::new(Session::default()));

    // defaults: switch off -> gate never active
    SessionMirror::initialize(&store, &vault, &session, &SystemLocale::default()).unwrap();
    assert!(!mochy_net::is_active_with(true));

    // switch on, mobile-only on: active only while metered
    store
        .set_by_id(SettingId::DataSaver, SettingValue::Bool(true))
        .unwrap();
    SessionMirror::initialize(&store, &vault, &session, &SystemLocale::default()).unwrap();
    assert!(mochy_net::is_active_with(true));
    assert!(!mochy_net::is_active_with(false));

    // live change through the watcher: "mobile only" off -> always active
    let (receiver, _guard) = SessionMirror::subscribe(&store).unwrap();
    store
        .set_by_id(SettingId::DataSaverMobileOnly, SettingValue::Bool(false))
        .unwrap();
    while let Ok(event) = receiver.try_recv() {
        SessionMirror::handle_one(
            &event,
            &store,
            &vault,
            &session,
            &SystemLocale::default(),
            &|_session| {},
        )
        .unwrap();
    }
    assert!(mochy_net::is_active_with(false), "always-on data saver");

    // restore the global defaults for any later test in this binary
    mochy_net::apply_data_saver_settings(false, true);
}
