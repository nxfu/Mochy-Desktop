//! The data-saver gate (`utils/DataSaver.kt`).
//!
//! Upstream: `DataSaver` is a process-wide object the stream picker, the
//! pages and the canvas providers consult "at the moment they would spend
//! bandwidth". It is on when the `dataSaver` switch is, and — unless the
//! switch says always — only while the active network is metered:
//!
//! ```kotlin
//! // utils/DataSaver.kt:39-43
//! private fun isActive(context: Context, metered: Boolean): Boolean {
//!     val store = context.dataStore
//!     if (!store.get(DataSaverKey, false)) return false
//!     return !store.get(DataSaverMobileOnlyKey, true) || metered
//! }
//! ```
//!
//! Desktop port: the two preference values are mirrored into atomics the
//! same way `OfflineMode` is (Phase 02, `App.kt:187-189`); the metered
//! answer comes from [`detect_metered`] (a per-OS stub for now — desktop
//! OSes do not hand every app the metered flag the way Android's
//! `ConnectivityManager.isActiveNetworkMetered` does) or from a manual
//! override the settings page or tray always has
//! (`DataSaverSettings::manual_metered_override`). The manual data-saver
//! switch itself stays always available, exactly as upstream: turning
//! `dataSaverMobileOnly` off makes the gate metered-independent.
//!
//! Layering: infrastructure leaf crate — atomics only, no IO, no async.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

/// Mirror of `DataSaverKey` (`constants/PreferenceKeys.kt:93`).
static DATA_SAVER: AtomicBool = AtomicBool::new(false);
/// Mirror of `DataSaverMobileOnlyKey` (`constants/PreferenceKeys.kt:94`).
static DATA_SAVER_MOBILE_ONLY: AtomicBool = AtomicBool::new(true);

/// The metered answer in use: 0 = unknown (stub default), 1 = metered,
/// 2 = not metered. Set by [`set_metered`] from the OS probe or a manual
/// override.
static METERED: AtomicU8 = AtomicU8::new(0);

const METERED_UNKNOWN: u8 = 0;
const METERED_YES: u8 = 1;
const METERED_NO: u8 = 2;

/// Whether the network is known to be metered (`None` = unknown — the
/// per-OS probe has not answered yet).
pub fn metered() -> Option<bool> {
    match METERED.load(Ordering::Acquire) {
        METERED_YES => Some(true),
        METERED_NO => Some(false),
        _ => None,
    }
}

/// Record the metered answer — from [`detect_metered`] or the manual
/// override in settings (the "manual toggle always available" path).
pub fn set_metered(metered: Option<bool>) {
    METERED.store(
        match metered {
            Some(true) => METERED_YES,
            Some(false) => METERED_NO,
            None => METERED_UNKNOWN,
        },
        Ordering::Release,
    );
}

/// The data-saver switch itself (`DataSaverKey`).
pub fn set_data_saver(on: bool) {
    DATA_SAVER.store(on, Ordering::Release);
}

/// The "mobile data only" qualifier (`DataSaverMobileOnlyKey`).
pub fn set_data_saver_mobile_only(on: bool) {
    DATA_SAVER_MOBILE_ONLY.store(on, Ordering::Release);
}

/// Push both preference values in one call — what the settings mirror
/// does at startup and on every change.
pub fn apply_data_saver_settings(data_saver: bool, mobile_only: bool) {
    set_data_saver(data_saver);
    set_data_saver_mobile_only(mobile_only);
}

/// `DataSaver.isActive(metered)` (`utils/DataSaver.kt:39-43`) — the pure
/// gate, for callers that already know whether the network is metered.
pub fn is_active_with(metered: bool) -> bool {
    if !DATA_SAVER.load(Ordering::Acquire) {
        return false;
    }
    !DATA_SAVER_MOBILE_ONLY.load(Ordering::Acquire) || metered
}

/// The same gate against the last metered answer ([`metered`]). With the
/// metered state unknown the mobile-only qualifier is satisfied (upstream
/// reads `isActiveNetworkMetered == true`, which is false when the
/// system has no answer — an unknown desktop network is not billed
/// mobile data).
pub fn is_active() -> bool {
    is_active_with(metered().unwrap_or(false))
}

/// Per-OS metered detection (the `ConnectivityManager` half of
/// `DataSaver.isActive(context)`).
///
/// Phase 07 ships the seam only — the desktop probes need OS APIs that
/// arrive with the media/desktop-integration phases:
///
/// - **Windows** (`cfg(target_os = "windows")`): the Network List Manager
///   (`INetworkConnectionCost` / `NL_COST_*` in `windows::Win32::Networking`)
///   answers "metered" for tethered and capped connections.
/// - **Linux** (`cfg(target_os = "linux")`): NetworkManager exposes the
///   metered flag per connection (`org.freedesktop.NetworkManager.Connection`
///   `metered` property over D-Bus); no answer for wicd/iwd setups.
/// - **macOS** (`cfg(target_os = "macos")`): CoreWiFi reports
///   `kCWNetworkIsHotspot`, the closest analog; Ethernet is unmetered.
///
/// Every stub returns `None` (unknown) so the manual override in settings
/// stays the operative answer — "the manual toggle is always available".
pub fn detect_metered() -> Option<bool> {
    // cfg arms per OS (all stubs in Phase 07; see the doc comment above)
    #[cfg(target_os = "windows")]
    {
        None // TODO(phase-14): NLM connection-cost probe.
    }
    #[cfg(target_os = "linux")]
    {
        None // TODO(phase-14): NetworkManager `metered` property probe.
    }
    #[cfg(target_os = "macos")]
    {
        None // TODO(phase-14): CoreWiFi hotspot probe.
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        None
    }
}

/// Refresh the metered state from the OS probe (the app shell calls this
/// on startup and on network-change events; safe to call anytime).
pub fn refresh_metered_from_os() -> Option<bool> {
    let answer = detect_metered();
    set_metered(answer);
    answer
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every test here flips the process-global data-saver atomics, so
    /// they serialize on the same lock as the offline-flag tests
    /// (see `test_support` in lib.rs).
    fn lock() -> std::sync::MutexGuard<'static, ()> {
        crate::test_support::OFFLINE_FLAG_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// DataSaver.kt:39-43 — the whole gate, table-driven.
    #[test]
    fn gate_matches_data_saver_kt() {
        let _guard = lock();
        // (data_saver, mobile_only, metered) -> active
        let table: &[(bool, bool, bool, bool)] = &[
            // switch off: never active
            (false, false, false, false),
            (false, false, true, false),
            (false, true, false, false),
            (false, true, true, false),
            // switch on, "always": active regardless of metering
            (true, false, false, true),
            (true, false, true, true),
            // switch on, mobile-only: active only while metered
            (true, true, false, false),
            (true, true, true, true),
        ];
        for &(enabled, mobile_only, metered, expected) in table {
            apply_data_saver_settings(enabled, mobile_only);
            assert_eq!(
                is_active_with(metered),
                expected,
                "dataSaver={enabled} mobileOnly={mobile_only} metered={metered}"
            );
        }
        // restore defaults for the other tests
        apply_data_saver_settings(false, true);
    }

    #[test]
    fn unknown_metered_reads_as_unmetered() {
        let _guard = lock();
        apply_data_saver_settings(true, true);
        set_metered(None);
        assert!(
            !is_active(),
            "unknown network must not trip the mobile-only gate"
        );
        set_metered(Some(true));
        assert!(is_active());
        set_metered(Some(false));
        assert!(!is_active());
        // restore defaults
        apply_data_saver_settings(false, true);
        set_metered(None);
    }

    #[test]
    fn defaults_match_upstream_preferences() {
        let _guard = lock();
        // DataSaverKey default false, DataSaverMobileOnlyKey default true
        // (constants/PreferenceKeys.kt:93-94 + DataSaver.kt reads).
        // Fresh-process view is asserted through the gate: with the
        // switch off nothing is active, on any network.
        assert!(!is_active_with(false));
        assert!(!is_active_with(true));
    }

    #[test]
    fn os_probe_stub_answers_unknown_until_overridden() {
        let _guard = lock();
        assert_eq!(detect_metered(), None);
        assert_eq!(refresh_metered_from_os(), None);
        assert_eq!(metered(), None);
        // the manual override remains fully available
        set_metered(Some(true));
        assert_eq!(metered(), Some(true));
        set_metered(None);
    }
}
