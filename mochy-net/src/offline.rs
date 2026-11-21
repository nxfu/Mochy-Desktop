//! The global offline-mode flag (utils/OfflineMode.kt).
//!
//! Upstream: a `@Volatile` singleton set at startup from the
//! `offlineMode` preference and on every change of it (App.kt:187-189 +
//! 229-235); the player, the lyrics lookup and the YouTube client all
//! check it "at the moment they would go online".
//!
//! Desktop port: an `AtomicBool` behind a handle type so the flag is a
//! field of the typed services object (PORT_SPEC §3.1 rule 3 — no global
//! mutable state beyond this process-wide atomic, which mirrors the
//! upstream singleton exactly).

use std::sync::atomic::{AtomicBool, Ordering};

static OFFLINE: AtomicBool = AtomicBool::new(false);

/// Set the global offline mode (idempotent).
pub fn set_offline(on: bool) {
    OFFLINE.store(on, Ordering::Release);
}

/// Read the global offline mode.
pub fn is_on() -> bool {
    OFFLINE.load(Ordering::Acquire)
}

/// A handle to the offline flag (stored in the services object).
#[derive(Debug, Clone, Copy, Default)]
pub struct OfflineFlag;

impl OfflineFlag {
    pub fn set(self, on: bool) {
        set_offline(on);
    }

    pub fn is_on(self) -> bool {
        is_on()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flag_round_trips_and_defaults_off() {
        // holds the test-support lock: the flag is process-global, and a
        // sibling test inside retry_on_io_errors must not observe it
        let _guard = crate::test_support::OFFLINE_FLAG_LOCK.lock().unwrap();
        assert!(!is_on(), "offline defaults to off");
        set_offline(true);
        assert!(is_on());
        assert!(OfflineFlag.is_on());
        set_offline(false);
        assert!(!is_on());
    }
}
