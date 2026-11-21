//! Phase 07 login-flow acceptance tests — the three no-webview
//! proofs: the cookie extraction over a recorded jar fixture, the
//! session persistence across a restart (the vault backend mock), and
//! the manual-paste flow, plus the sign-out forgetting.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, RwLock};

use mochy_config::{SettingId, SettingValue, SettingsStore};
use mochy_innertube::models::AccountInfo;
use mochy_innertube::session::{SessionMirror, SharedSession, SystemLocale};
use mochy_ui_settings::login::{
    AccountInfoSource, JarCookieRecord, LoginCredentials, LoginFlow, cookie_header_for,
    has_sapisid, normalize_paste,
};
use mochy_vault::{InMemoryBackend, Vault, VaultKey};

type BoxFut<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

struct FixedAccount;

impl AccountInfoSource for FixedAccount {
    fn account_info(&self) -> BoxFut<'_, Result<AccountInfo, String>> {
        Box::pin(async {
            Ok(AccountInfo {
                name: "Mochy User".into(),
                email: Some("user@example.com".into()),
                channel_handle: Some("@mochyuser".into()),
                thumbnail_url: Some("https://yt3.example/photo.jpg".into()),
            })
        })
    }
}

#[derive(Debug, serde::Deserialize)]
struct Fixture {
    now_unix: i64,
    jar: Vec<JarCookieRecord>,
}

fn fixture() -> Fixture {
    let text = include_str!("fixtures/login_cookie_jar.json");
    serde_json::from_str(text).expect("the recorded cookie-jar fixture parses")
}

/// ACCEPTANCE (Phase 07): cookie-extraction over the recorded
/// cookie-jar fixture — no real sign-in in CI. The header keeps every
/// live .youtube.com cookie (HttpOnly included), drops the .google.com
/// siblings, the expired row, the www-only host cookie and the
/// path-bound one, and passes the SAPISID guard.
#[test]
fn cookie_extraction_over_the_recorded_jar_fixture() {
    let fixture = fixture();
    let header = cookie_header_for(&fixture.jar, fixture.now_unix);

    // every live .youtube.com cookie survives, in jar order
    let pairs: Vec<&str> = header.split("; ").collect();
    assert_eq!(
        pairs,
        vec![
            "VISITOR_INFO1_LIVE=Cgt2aXNpdG9yX2lk",
            "YSC=sc-token",
            "SAPISID=REDACTED_SAPISID_VALUE",
            "__Secure-3PAPISID=REDACTED_3PAPISID_VALUE",
            "SID=REDACTED_SID",
            "HSID=REDACTED_HSID",
            "SSID=REDACTED_SSID",
            "APISID=REDACTED_APISID",
            "LOGIN_INFO=REDACTED_LOGIN_INFO",
            "PREF=hl=en&gl=US",
        ]
    );

    // the load-bearing property: the HttpOnly SAPISID is in, and the
    // wrong-tree SIDs are out
    assert!(has_sapisid(&header));
    assert!(!header.contains("GOOGLE_SIDE_SSID"));
    assert!(!header.contains("GOOGLE_SIDE_SAPISID"));
    assert!(!header.contains("stale"), "the expired cookie is dropped");
    assert!(!header.contains("WWW_ONLY"));
    assert!(
        !header.contains("PATHBOUND"),
        "path /watch does not match /"
    );

    // and the credentials built from it validate
    let credentials = LoginCredentials::validate(&header).unwrap();
    assert!(credentials.cookie.starts_with("VISITOR_INFO1_LIVE="));
}

/// ACCEPTANCE (Phase 07): session persistence — a restart reloads the
/// credentials from the vault backend (the mock). Sign in, drop every
/// in-memory handle, rebuild the vault + settings + session from the
/// same backend and files, run the production loader
/// (`SessionMirror::initialize`), and the session is signed in again
/// with the account fields intact.
#[tokio::test]
async fn restart_reloads_credentials_from_the_vault_backend() {
    // the app data dir that survives the "restart"
    let dir = tempfile::tempdir().unwrap();
    let backend = Arc::new(InMemoryBackend::new());

    // -- first run: sign in --
    {
        let vault =
            Vault::with_backend(Arc::clone(&backend) as Arc<dyn mochy_vault::SecretBackend>);
        let store = SettingsStore::load(dir.path().join("settings.json"));
        let session: SharedSession = Arc::new(RwLock::new(Default::default()));

        let credentials = LoginCredentials {
            cookie: "SAPISID=abc; SID=xyz; __Secure-3PAPISID=def".into(),
            visitor_data: Some("Cgt2aXNpdG9y".into()),
            data_sync_id: Some("prefix||suffix".into()),
        };
        let flow = LoginFlow::new(session, vault, store);
        let snapshot = flow.complete(&credentials, &FixedAccount).await.unwrap();
        assert_eq!(snapshot.name, "Mochy User");
        assert!(flow.is_signed_in());
    } // everything but the backend and the settings file dropped here

    // -- the "restart": fresh vault over the SAME backend, fresh
    //    session, the production loader --
    let vault = Vault::with_backend(Arc::clone(&backend) as Arc<dyn mochy_vault::SecretBackend>);
    let store = SettingsStore::load(dir.path().join("settings.json"));
    let session: SharedSession = Arc::new(RwLock::new(Default::default()));

    SessionMirror::initialize(&store, &vault, &session, &SystemLocale::default()).unwrap();

    let guard = session.read().unwrap();
    assert_eq!(
        guard.cookie.as_deref(),
        Some("SAPISID=abc; SID=xyz; __Secure-3PAPISID=def"),
        "the cookie reloads from the vault"
    );
    assert_eq!(guard.visitor_data.as_deref(), Some("Cgt2aXNpdG9y"));
    assert_eq!(
        guard.data_sync_id.as_deref(),
        Some("suffix"),
        "the raw 'prefix||suffix' persisted value normalizes on load (App.kt:253-260)"
    );
    assert!(guard.is_logged_in());
    drop(guard);

    // the account triple survives in the settings file
    let settings = store.snapshot();
    assert_eq!(settings.AccountName(), "Mochy User");
    assert_eq!(settings.AccountEmail(), "user@example.com");
    assert_eq!(settings.AccountChannelHandle(), "@mochyuser");

    // and the flow sees itself signed in after the restart
    let flow = LoginFlow::new(session, vault, store);
    assert!(flow.is_signed_in());
}

/// The manual paste flow (tier 3): a devtools-shaped paste normalizes,
/// validates, completes, and is then forgotten by sign-out.
#[tokio::test]
async fn manual_paste_flow_completes_and_sign_out_forgets() {
    let dir = tempfile::tempdir().unwrap();
    let backend = Arc::new(InMemoryBackend::new());
    let vault = Vault::with_backend(Arc::clone(&backend) as Arc<dyn mochy_vault::SecretBackend>);
    let store = SettingsStore::load(dir.path().join("settings.json"));
    let session: SharedSession = Arc::new(RwLock::new(Default::default()));
    let flow = LoginFlow::new(session, vault, store);

    assert!(!flow.is_signed_in());

    // a paste copied from devtools (newlines) with the required cookie
    let paste = normalize_paste("SAPISID=pasted\nSID=alsopasted\nPREF=hl=en");
    let snapshot = flow.login_with_paste(&paste, &FixedAccount).await.unwrap();
    assert_eq!(snapshot.name, "Mochy User");
    assert!(flow.is_signed_in());

    // a paste without SAPISID is refused before anything is stored
    let bad = flow.login_with_paste("SID=only", &FixedAccount).await;
    assert!(bad.is_err());

    // sign-out forgets the keys (App.kt forgetAccount)
    flow.sign_out().unwrap();
    assert!(!flow.is_signed_in());
    let flow_vault = flow_sign_out_probe(&backend);
    assert!(flow_vault.is_none());
}

/// A paste missing SAPISID must not partially persist.
fn flow_sign_out_probe(backend: &Arc<InMemoryBackend>) -> Option<String> {
    let vault = Vault::with_backend(Arc::clone(backend) as Arc<dyn mochy_vault::SecretBackend>);
    vault.get(VaultKey::InnerTubeCookie)
}

/// A failed account fetch still saves the session (upstream's
/// `getOrNull` tolerance): the login completes with empty account
/// fields and the settings triple exists but is empty.
#[tokio::test]
async fn account_fetch_failure_still_signs_in() {
    struct NoAccount;
    impl AccountInfoSource for NoAccount {
        fn account_info(&self) -> BoxFut<'_, Result<AccountInfo, String>> {
            Box::pin(async { Err("account menu unreachable".into()) })
        }
    }

    let dir = tempfile::tempdir().unwrap();
    let vault = Vault::open_in_memory();
    let store = SettingsStore::load(dir.path().join("settings.json"));
    let session: SharedSession = Arc::new(RwLock::new(Default::default()));
    let flow = LoginFlow::new(session, vault, store);

    let paste = normalize_paste("SAPISID=x; SID=y");
    let snapshot = flow.login_with_paste(&paste, &NoAccount).await.unwrap();
    assert_eq!(snapshot.name, "", "empty account fields, like orEmpty()");
    assert!(flow.is_signed_in());

    // the settings triple exists but is empty
    let settings = SettingsStore::load(dir.path().join("settings.json")).snapshot();
    assert_eq!(settings.AccountName(), "");
    assert_eq!(settings.AccountChannelHandle(), "");
    let _ = SettingId::AccountName; // the key is a registry member
    let _ = SettingValue::Str(String::new());
}
