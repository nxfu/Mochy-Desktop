//! The YouTube sign-in flow — the port of
//! `ui/screens/settings/importsync/YouTubeLoginScreen.kt`.
//!
//! Three tiers (blueprint ch. 14 R3 mitigation):
//!
//! 1. **The wry window** (feature `login-window`): a child webview on
//!    accounts.google.com with a desktop-Chrome UA, watching for the
//!    music.youtube.com landing, then harvesting the cookie jar
//!    (HttpOnly included) + the ytcfg session ids —
//!    `YouTubeLoginWebContent` (YouTubeLoginScreen.kt:141-280).
//! 2. **The loopback-redirect fallback** (same feature): the sign-in
//!    `continue` target becomes a one-shot `127.0.0.1` listener; the
//!    redirect is a deterministic completion signal, after which the
//!    window materializes the YouTube cookies on music.youtube.com.
//! 3. **Manual cookie paste** (always compiled, pure): the last
//!    resort for machines where the webview cannot finish.
//!
//! The completion (persist to vault + settings, fetch the account over
//! account/account_menu, push the session) and the sign-out
//! (App.kt `forgetAccount`) are pure and testable without any webview:
//! see [`account`]. Cookie matching lives in [`cookies`], the ytcfg
//! scrape in [`ytcfg`].
//!
//! Single account, exactly like upstream: one cookie, one visitor id,
//! one data-sync id, one account triple.

pub mod account;
pub mod cookies;
pub mod loopback;
pub mod manual;
#[cfg(feature = "login-window")]
pub mod webview;
pub mod ytcfg;

pub use account::{
    AccountInfoSource, AccountSnapshot, FacadeAccountSource, LoginCredentials, LoginError,
    complete_login, sign_out, stored_account,
};
pub use cookies::{HARVEST_URL, JarCookieRecord, cookie_header_for, has_sapisid};
pub use loopback::{LoopbackError, LoopbackFinishListener};
pub use manual::{normalize_paste, validate_paste};
pub use ytcfg::{READ_CONFIG_JS, YtcfgIds, parse_ytcfg_result};

#[cfg(feature = "login-window")]
pub use webview::{
    LOGIN_URL, LoginWindowOptions, WindowOutcome, desktop_chrome_user_agent,
    login_url_with_continue, run_login_window,
};

use std::sync::{Arc, RwLock};

use mochy_config::SettingsStore;
use mochy_innertube::session::SharedSession;
use mochy_vault::Vault;

/// One login flow over the app's session, vault and settings — the
/// state the upstream screen reached through singletons
/// (`YouTube.cookie`, `context.dataStore`).
pub struct LoginFlow {
    session: SharedSession,
    vault: Vault,
    store: SettingsStore,
}

impl LoginFlow {
    pub fn new(session: SharedSession, vault: Vault, store: SettingsStore) -> Self {
        LoginFlow {
            session,
            vault,
            store,
        }
    }

    /// Complete a harvested or pasted login —
    /// [`complete_login`] with this flow's handles.
    pub async fn complete(
        &self,
        credentials: &LoginCredentials,
        account_source: &dyn AccountInfoSource,
    ) -> Result<AccountSnapshot, LoginError> {
        complete_login(
            credentials,
            &self.session,
            &self.vault,
            &self.store,
            account_source,
        )
        .await
    }

    /// Sign out — [`sign_out`] with this flow's handles
    /// (App.kt forgetAccount: the keys are forgotten, everything
    /// already synced stays).
    pub fn sign_out(&self) -> Result<(), LoginError> {
        sign_out(&self.session, &self.vault, &self.store)
    }

    /// Whether a signed-in session is stored (the SAPISID rule).
    pub fn is_signed_in(&self) -> bool {
        mochy_innertube::parse_cookie_string(
            &self
                .vault
                .get(mochy_vault::VaultKey::InnerTubeCookie)
                .unwrap_or_default(),
        )
        .contains_key("SAPISID")
    }

    /// The interactive window flow — tiers 1 and 2 (feature
    /// `login-window`). Blocks the calling thread while the window is
    /// up; async callers wrap it in their runtime's blocking facility
    /// (`tokio::task::spawn_blocking`). Returns the harvested
    /// credentials on success — the caller completes them with
    /// [`LoginFlow::complete`] on its own runtime (the account fetch
    /// is the only await).
    #[cfg(feature = "login-window")]
    pub fn login_with_window(
        &self,
        options: LoginWindowOptions,
    ) -> Result<WindowOutcome, LoginError> {
        run_login_window(options).map_err(LoginError::Window)
    }

    /// The manual paste flow — tier 3, always available. Validates the
    /// paste (the same SAPISID guard as the harvest) and completes it.
    pub async fn login_with_paste(
        &self,
        paste: &str,
        account_source: &dyn AccountInfoSource,
    ) -> Result<AccountSnapshot, LoginError> {
        let credentials = validate_paste(paste)?;
        self.complete(&credentials, account_source).await
    }
}

/// Convenience: a fresh default shared session for callers that have
/// none yet (the app shell owns the real one).
pub fn fresh_session() -> SharedSession {
    Arc::new(RwLock::new(Default::default()))
}
