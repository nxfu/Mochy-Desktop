//! The sync gates — play reporting, history pull, the auto-sync
//! cooldown — over the settings registry.
//!
//! Upstream sources:
//! - `MusicService.kt:1193-1201` — the play reporter enable:
//!   `(prefs[YtReportPlaysKey] ?: true) && prefs[OfflineModeKey] != true`
//!   ("offline mode keeps every play on the phone too"), with
//!   `countAfterMs = HistoryDuration * 1000`;
//! - `constants/PreferenceKeys.kt:137-142` — `ytReportPlays` (default
//!   true upstream) and `ytPullHistory` (default false; "YouTube Music
//!   history is copied into Mochy's own listening history"), plus the
//!   `ytHistoryLastSeen` marker;
//! - `utils/SyncUtils.kt:215-238` (`tryAutoSync`) +
//!   `extensions/ContextExt.kt` (`isSyncEnabled = YtmSyncKey && user
//!   logged in`) + `constants/PreferenceKeys.kt:435`
//!   (`SYNC_COOLDOWN = 30 * 60`) — the auto-sync gate;
//! - `utils/SyncUtils.kt:354-357` — `YtSyncHoldKey` holds YouTube pulls
//!   after an undo (checked by the caller; see [`SyncHoldGate`]).
//!
//! Both toggles are user-controlled privacy switches the gates read
//! live; upstream defaults them `ytReportPlays = true` /
//! `ytPullHistory = false` (the settings screens' `rememberPreference`
//! fallbacks, YouTubeSyncScreen.kt:56-57) and the port's registry
//! carries the same defaults.

use mochy_config::Settings;

/// `SYNC_COOLDOWN` (constants/PreferenceKeys.kt:435) — 30 minutes.
pub const SYNC_COOLDOWN_SECS: i64 = 30 * 60;

/// The play-report gate (MusicService.kt:1196): plays count on the
/// signed-in account's YouTube history only when the user allows it
/// AND the app is not in offline mode (offline keeps every play local).
pub fn play_reporting_enabled(settings: &Settings) -> bool {
    settings.YtReportPlays() && !settings.OfflineMode()
}

/// The history-pull gate: YouTube Music's history is copied into
/// Mochy's own listening history only when the user asked for it
/// (`ytPullHistory`, opt-in upstream and here).
pub fn history_sync_enabled(settings: &Settings) -> bool {
    settings.YtPullHistory()
}

/// `tryAutoSync` (SyncUtils.kt:215-238) — an automatic full sync may
/// run when syncing is on (`YtmSyncKey`), the user is signed in, the
/// internet is connected, and the last full sync is older than
/// [`SYNC_COOLDOWN_SECS`] (`lastSync > 0` guards a never-synced store —
/// the first sync is always due).
pub fn auto_sync_allowed(
    settings: &Settings,
    logged_in: bool,
    internet_connected: bool,
    last_full_sync_secs: i64,
    now_epoch_secs: i64,
) -> bool {
    // isSyncEnabled (ContextExt.kt) = YtmSyncKey && user logged in
    if !settings.YtmSync() || !logged_in {
        return false;
    }
    if !internet_connected {
        return false;
    }
    if last_full_sync_secs > 0 && (now_epoch_secs - last_full_sync_secs) < SYNC_COOLDOWN_SECS {
        return false;
    }
    true
}

/// The undo hold (`YtSyncHoldKey`, SyncUtils.kt:354-357): an undo
/// leaves it on so automatic YouTube pulls stay off until the user
/// syncs manually; any manual sync or a fresh login clears it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyncHoldGate {
    pub held: bool,
}

impl SyncHoldGate {
    pub fn from_settings(settings: &Settings) -> Self {
        SyncHoldGate {
            held: settings.YtSyncHold(),
        }
    }

    /// `executeFullSync`'s check: automatic syncs respect the hold,
    /// manual ones do not.
    pub fn blocks(self, manual: bool) -> bool {
        self.held && !manual
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mochy_config::{SettingId, SettingValue, SettingsStore};

    fn store_with(id: SettingId, value: SettingValue) -> SettingsStore {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::mem::forget(dir);
        let store = SettingsStore::load(path);
        store.set_by_id(id, value).unwrap();
        store
    }

    /// MusicService.kt:1196 — the play-report truth table.
    #[test]
    fn play_reporting_gate_matches_music_service() {
        // default: allowed (ytReportPlays defaults true upstream)
        let default_store = {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("settings.json");
            std::mem::forget(dir);
            SettingsStore::load(path)
        };
        assert!(play_reporting_enabled(&default_store.snapshot()));

        // offline mode keeps every play on the machine
        let store = store_with(SettingId::OfflineMode, SettingValue::Bool(true));
        assert!(!play_reporting_enabled(&store.snapshot()));

        // the user may switch reporting off entirely
        let store = store_with(SettingId::YtReportPlays, SettingValue::Bool(false));
        assert!(!play_reporting_enabled(&store.snapshot()));
    }

    /// `ytPullHistory` is opt-in (default false).
    #[test]
    fn history_pull_is_opt_in() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::mem::forget(dir);
        let store = SettingsStore::load(path);
        assert!(!history_sync_enabled(&store.snapshot()));
        store
            .set_by_id(SettingId::YtPullHistory, SettingValue::Bool(true))
            .unwrap();
        assert!(history_sync_enabled(&store.snapshot()));
    }

    /// SyncUtils.kt:215-238 — the auto-sync gate, table-driven.
    #[test]
    fn auto_sync_gate_matches_try_auto_sync() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::mem::forget(dir);
        let store = SettingsStore::load(path);
        let settings = store.snapshot();
        let now = 1_000_000;

        // never synced + everything on -> due
        assert!(auto_sync_allowed(&settings, true, true, 0, now));
        // signed out / offline -> not due
        assert!(!auto_sync_allowed(&settings, false, true, 0, now));
        assert!(!auto_sync_allowed(&settings, true, false, 0, now));
        // within the cooldown -> not due
        assert!(!auto_sync_allowed(&settings, true, true, now - 1_000, now));
        assert!(!auto_sync_allowed(
            &settings,
            true,
            true,
            now - SYNC_COOLDOWN_SECS + 1,
            now
        ));
        // exactly at the cooldown boundary -> due (upstream: `< COOLDOWN`)
        assert!(auto_sync_allowed(
            &settings,
            true,
            true,
            now - SYNC_COOLDOWN_SECS,
            now
        ));
        // far past it -> due
        assert!(auto_sync_allowed(
            &settings,
            true,
            true,
            now - 10 * SYNC_COOLDOWN_SECS,
            now
        ));

        // YtmSyncKey off -> never (isSyncEnabled)
        let store = store_with(SettingId::YtmSync, SettingValue::Bool(false));
        assert!(!auto_sync_allowed(&store.snapshot(), true, true, 0, now));
    }

    /// SyncUtils.kt:354-357 — the undo hold blocks automatic pulls only.
    #[test]
    fn sync_hold_blocks_auto_but_not_manual() {
        let store = store_with(SettingId::YtSyncHold, SettingValue::Bool(true));
        let gate = SyncHoldGate::from_settings(&store.snapshot());
        assert!(gate.blocks(false));
        assert!(!gate.blocks(true), "a manual sync ignores the hold");
    }
}
