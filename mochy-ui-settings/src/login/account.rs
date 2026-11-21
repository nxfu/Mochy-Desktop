//! Account persistence — `finishLogin`'s save half
//! (YouTubeLoginScreen.kt:172-191) and `App.forgetAccount`
//! (App.kt:372-405).
//!
//! One signed-in account, exactly like upstream: the store holds one
//! cookie, one visitor id, one data-sync id and one name/email/handle
//! triple; a new sign-in overwrites them.
//!
//! Where things live on desktop (Phase 02 decision F-004): the three
//! session secrets go to the OS keychain via `mochy-vault`
//! (innerTubeCookie / visitorData / dataSyncId — the same names the
//! Android DataStore used, now vault keys); the account fields stay in
//! the settings registry (accountName / accountEmail /
//! accountChannelHandle). Sign-out forgets all six, clears the shared
//! session, and — where upstream wiped the WebView CookieManager —
//! simply drops the login window: the desktop webview jar lives only
//! as long as the window, the vault was the only persistent copy.
//!
//! The account fetch is behind the [`AccountInfoSource`] trait: the
//! production source is the `YouTube.accountInfo()` facade (the Phase
//! 04 `account/account_menu` endpoint) reading the just-pushed
//! session; tests inject a fake. Upstream tolerates the fetch failing
//! (`getOrNull()`): the login still saves with empty account fields.

use std::future::Future;
use std::pin::Pin;

use mochy_config::{SettingId, SettingValue, SettingsStore};
use mochy_innertube::models::AccountInfo;
use mochy_innertube::session::{SharedSession, normalize_data_sync_id};
use mochy_vault::{Vault, VaultKey};

use super::cookies::has_sapisid;

/// The boxed-future alias for the account-info seam.
type BoxFut<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// `YouTube.accountInfo()` — the account/account_menu read the login
/// runs right after pushing the new session
/// (YouTubeLoginScreen.kt:174-183).
pub trait AccountInfoSource: Send + Sync {
    fn account_info(&self) -> BoxFut<'_, Result<AccountInfo, String>>;
}

/// The production source: the mochy-innertube facade over the
/// mirrored session. Cheap handle (the transport is clone-safe).
pub struct FacadeAccountSource {
    youtube: mochy_innertube::youtube::YouTube,
}

impl FacadeAccountSource {
    pub fn new(youtube: mochy_innertube::youtube::YouTube) -> Self {
        FacadeAccountSource { youtube }
    }
}

impl AccountInfoSource for FacadeAccountSource {
    fn account_info(&self) -> BoxFut<'_, Result<AccountInfo, String>> {
        Box::pin(async move {
            self.youtube
                .account_info()
                .await
                .map_err(|error| error.to_string())
        })
    }
}

/// What a completed login harvested — the exact values
/// `finishLogin` saves (YouTubeLoginScreen.kt:172-191).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginCredentials {
    /// The full `Cookie:` header for music.youtube.com, HttpOnly
    /// cookies included; MUST carry SAPISID (validated on
    /// construction via [`LoginCredentials::validate`]).
    pub cookie: String,
    /// `VISITOR_DATA` from ytcfg, when the scrape found one.
    pub visitor_data: Option<String>,
    /// `DATASYNC_ID` from ytcfg, raw (`"||"` form preserved — the
    /// session loader normalizes).
    pub data_sync_id: Option<String>,
}

impl LoginCredentials {
    /// `finishLogin`'s guard (YouTubeLoginScreen.kt:165-166): a cookie
    /// string without SAPISID is not a signed-in session.
    pub fn validate(cookie: &str) -> Result<Self, LoginError> {
        let cookie = cookie.trim().to_owned();
        if cookie.is_empty() {
            return Err(LoginError::EmptyCookie);
        }
        if !has_sapisid(&cookie) {
            return Err(LoginError::MissingSapisid);
        }
        Ok(LoginCredentials {
            cookie,
            visitor_data: None,
            data_sync_id: None,
        })
    }
}

/// The account snapshot the UI shows (the settings triple plus the
/// photo the account menu returns).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AccountSnapshot {
    pub name: String,
    pub email: String,
    pub channel_handle: String,
    pub photo_url: Option<String>,
}

impl AccountSnapshot {
    /// The three persisted fields (YouTubeLoginScreen.kt:184-191) —
    /// empty strings when the fetch failed, exactly upstream's
    /// `info?.name.orEmpty()`.
    pub fn from_info(info: Option<&AccountInfo>) -> Self {
        match info {
            Some(info) => AccountSnapshot {
                name: info.name.clone(),
                email: info.email.clone().unwrap_or_default(),
                channel_handle: info.channel_handle.clone().unwrap_or_default(),
                photo_url: info.thumbnail_url.clone(),
            },
            None => AccountSnapshot::default(),
        }
    }
}

/// Apply a completed login: push the session, fetch the account, and
/// persist everything — `finishLogin` (YouTubeLoginScreen.kt:172-191).
///
/// The account fetch failing is NOT a login failure (upstream
/// `getOrNull()`); the snapshot comes back empty then.
pub async fn complete_login(
    credentials: &LoginCredentials,
    session: &SharedSession,
    vault: &Vault,
    store: &SettingsStore,
    account_source: &dyn AccountInfoSource,
) -> Result<AccountSnapshot, LoginError> {
    // 1. the live session gets the values first, so the account fetch
    //    rides them (YouTubeLoginScreen.kt:174-182: YouTube.cookie =
    //    cookie; visitorData?.let { YouTube.visitorData = it }; the
    //    dataSyncId "||" handling)
    if let Ok(mut guard) = session.write() {
        guard.set_cookie(Some(credentials.cookie.clone()));
        if let Some(visitor) = &credentials.visitor_data {
            guard.visitor_data = Some(visitor.clone());
        }
        if let Some(sync) = &credentials.data_sync_id {
            guard.data_sync_id = Some(normalize_data_sync_id(sync));
        }
    }

    // 2. account details need the new session
    let info = account_source.account_info().await.ok();

    // 3. persist: the three secrets into the vault
    vault
        .set(VaultKey::InnerTubeCookie, &credentials.cookie)
        .map_err(LoginError::Vault)?;
    if let Some(visitor) = &credentials.visitor_data {
        vault
            .set(VaultKey::VisitorData, visitor)
            .map_err(LoginError::Vault)?;
    }
    if let Some(sync) = &credentials.data_sync_id {
        vault
            .set(VaultKey::DataSyncId, sync)
            .map_err(LoginError::Vault)?;
    }

    // 4. ...and the account triple into settings (empty on fetch
    //    failure — orEmpty()); flushed immediately — a sign-in is
    //    rare and the triple must survive a crash right after (the
    //    upstream DataStore writes through on every edit)
    let snapshot = AccountSnapshot::from_info(info.as_ref());
    let _ = store.set_by_id(
        SettingId::AccountName,
        SettingValue::Str(snapshot.name.clone()),
    );
    let _ = store.set_by_id(
        SettingId::AccountEmail,
        SettingValue::Str(snapshot.email.clone()),
    );
    let _ = store.set_by_id(
        SettingId::AccountChannelHandle,
        SettingValue::Str(snapshot.channel_handle.clone()),
    );
    let _ = store.save();

    Ok(snapshot)
}

/// Sign out — `App.forgetAccount` (App.kt:372-405): remove the three
/// secrets and the account triple, null the session's cookie /
/// visitorData / dataSyncId. Everything already synced STAYS in the
/// library (ImportSyncViewModel.signOut's contract). Upstream also
/// wipes the WebView CookieManager; on desktop the login window's jar
/// dies with the window and the vault was the only persistent copy, so
/// forgetting the keys is the whole job.
pub fn sign_out(
    session: &SharedSession,
    vault: &Vault,
    store: &SettingsStore,
) -> Result<(), LoginError> {
    vault
        .delete(VaultKey::InnerTubeCookie)
        .map_err(LoginError::Vault)?;
    vault
        .delete(VaultKey::VisitorData)
        .map_err(LoginError::Vault)?;
    vault
        .delete(VaultKey::DataSyncId)
        .map_err(LoginError::Vault)?;

    let _ = store.set_by_id(SettingId::AccountName, SettingValue::Str(String::new()));
    let _ = store.set_by_id(SettingId::AccountEmail, SettingValue::Str(String::new()));
    let _ = store.set_by_id(
        SettingId::AccountChannelHandle,
        SettingValue::Str(String::new()),
    );
    let _ = store.save();

    if let Ok(mut guard) = session.write() {
        guard.set_cookie(None);
        guard.visitor_data = None;
        guard.data_sync_id = None;
    }
    Ok(())
}

/// The account snapshot the settings screen shows from the persisted
/// fields (the photo is not persisted upstream; it re-renders from a
/// fresh accountInfo when signed in).
pub fn stored_account(store: &SettingsStore) -> AccountSnapshot {
    let settings = store.snapshot();
    AccountSnapshot {
        name: settings.AccountName(),
        email: settings.AccountEmail(),
        channel_handle: settings.AccountChannelHandle(),
        photo_url: None,
    }
}

/// Login-flow failures.
#[derive(Debug, thiserror::Error)]
pub enum LoginError {
    /// The pasted/harvested cookie carries no SAPISID — sign-in has
    /// not finished (the upstream guard).
    #[error("the cookie has no SAPISID; sign-in is not complete yet")]
    MissingSapisid,
    /// An empty paste.
    #[error("the cookie is empty")]
    EmptyCookie,
    /// The keychain refused a write.
    #[error("vault: {0}")]
    Vault(#[from] mochy_vault::VaultError),
    /// The webview tier failed to even open.
    #[error("login window: {0}")]
    Window(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, RwLock};

    fn temp_store() -> (SettingsStore, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::mem::forget(dir);
        (SettingsStore::load(path.clone()), path)
    }

    struct FakeAccounts(Option<AccountInfo>);

    impl AccountInfoSource for FakeAccounts {
        fn account_info(&self) -> BoxFut<'_, Result<AccountInfo, String>> {
            let answer = self.0.clone().ok_or_else(|| "no account".to_string());
            Box::pin(async move { answer })
        }
    }

    fn credentials() -> LoginCredentials {
        LoginCredentials {
            cookie: "SAPISID=abc; SID=xyz; __Secure-3PAPISID=def".into(),
            visitor_data: Some("Cgt2aXNpdG9y".into()),
            data_sync_id: Some("prefix||suffix".into()),
        }
    }

    /// YouTubeLoginScreen.kt:165-166 + 172-191 — the SAPISID guard and
    /// the save order.
    #[tokio::test]
    async fn complete_login_persists_and_pushes_the_session() {
        let (store, _path) = temp_store();
        let vault = Vault::open_in_memory();
        let session: SharedSession = Arc::new(RwLock::new(Default::default()));

        let info = AccountInfo {
            name: "Mochy User".into(),
            email: Some("user@example.com".into()),
            channel_handle: Some("@mochyuser".into()),
            thumbnail_url: Some("https://yt3.ggpht.example/photo".into()),
        };
        let snapshot = complete_login(
            &credentials(),
            &session,
            &vault,
            &store,
            &FakeAccounts(Some(info)),
        )
        .await
        .unwrap();

        assert_eq!(snapshot.name, "Mochy User");
        assert_eq!(snapshot.email, "user@example.com");
        assert_eq!(snapshot.channel_handle, "@mochyuser");
        assert_eq!(
            snapshot.photo_url.as_deref(),
            Some("https://yt3.ggpht.example/photo")
        );

        // the session carries the new values, dataSyncId normalized
        let guard = session.read().unwrap();
        assert_eq!(
            guard.cookie.as_deref(),
            Some("SAPISID=abc; SID=xyz; __Secure-3PAPISID=def")
        );
        assert_eq!(guard.visitor_data.as_deref(), Some("Cgt2aXNpdG9y"));
        assert_eq!(guard.data_sync_id.as_deref(), Some("suffix"));

        // the vault keeps the raw values
        assert_eq!(
            vault.get(VaultKey::InnerTubeCookie).as_deref(),
            Some("SAPISID=abc; SID=xyz; __Secure-3PAPISID=def")
        );
        assert_eq!(
            vault.get(VaultKey::DataSyncId).as_deref(),
            Some("prefix||suffix")
        );

        // the settings triple matches the account
        assert_eq!(stored_account(&store).name, "Mochy User");
        assert_eq!(stored_account(&store).email, "user@example.com");
        assert_eq!(stored_account(&store).channel_handle, "@mochyuser");
    }

    /// YouTubeLoginScreen.kt:183 + 188-190 — an accountInfo failure is
    /// tolerated: the login still saves, the triple is empty.
    #[tokio::test]
    async fn account_fetch_failure_still_saves_the_login() {
        let (store, _path) = temp_store();
        let vault = Vault::open_in_memory();
        let session: SharedSession = Arc::new(RwLock::new(Default::default()));

        let snapshot = complete_login(
            &credentials(),
            &session,
            &vault,
            &store,
            &FakeAccounts(None),
        )
        .await
        .unwrap();
        assert_eq!(snapshot, AccountSnapshot::default());
        assert!(vault.get(VaultKey::InnerTubeCookie).is_some());
        assert_eq!(stored_account(&store).name, "");
    }

    /// App.kt:372-405 — sign-out forgets every key and the session.
    #[tokio::test]
    async fn sign_out_forgets_everything() {
        let (store, _path) = temp_store();
        let vault = Vault::open_in_memory();
        let session: SharedSession = Arc::new(RwLock::new(Default::default()));
        complete_login(
            &credentials(),
            &session,
            &vault,
            &store,
            &FakeAccounts(None),
        )
        .await
        .unwrap();
        sign_out(&session, &vault, &store).unwrap();

        assert!(vault.get(VaultKey::InnerTubeCookie).is_none());
        assert!(vault.get(VaultKey::VisitorData).is_none());
        assert!(vault.get(VaultKey::DataSyncId).is_none());
        let guard = session.read().unwrap();
        assert!(
            guard.cookie.is_none() && guard.visitor_data.is_none() && guard.data_sync_id.is_none()
        );
        drop(guard);
        assert_eq!(stored_account(&store).name, "");
        // idempotent: a second sign-out of nothing is fine
        sign_out(&session, &vault, &store).unwrap();
    }

    /// The SAPISID guard on manual construction.
    #[test]
    fn validate_enforces_the_sapisid_guard() {
        assert!(LoginCredentials::validate("SID=x; SAPISID=y").is_ok());
        assert!(matches!(
            LoginCredentials::validate("SID=x"),
            Err(LoginError::MissingSapisid)
        ));
        assert!(matches!(
            LoginCredentials::validate("   "),
            Err(LoginError::EmptyCookie)
        ));
    }
}
