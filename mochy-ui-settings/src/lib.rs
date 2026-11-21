//! Registry-driven settings UI, AudioLab pages, setup wizard, diagnostics
//!
//! Upstream absorption: ui/screens/settings (72 files) + audiolab/ + equalizer/ + setup/ (17 files) + devspace/ diagnostics screens.
//!
//! Phase 07 fills in `login` — the port of
//! `ui/screens/settings/importsync/YouTubeLoginScreen.kt`: the wry
//! sign-in window with cookie extraction (feature `login-window`), the
//! loopback-redirect fallback, the manual cookie-paste last resort, the
//! pure extraction/validation core, account persistence into the vault
//! and the settings registry, and sign-out (App.kt forgetAccount). The
//! settings screens themselves arrive with the UI phases (M3+).

pub mod login;

#[cfg(test)]
mod tests {
    #[test]
    fn crate_skeleton_presents() {
        assert_eq!(env!("CARGO_PKG_NAME"), "mochy-ui-settings");
    }
}
