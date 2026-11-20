//! Settings registry (~400 keys with defaults + search metadata), audio-lab store, atomic persistence, live watchers
//!
//! Upstream absorption: constants/PreferenceKeys.kt (1,399 lines; 398 static keys + per-style key factory) + utils/DataStore + 22 SharedPreferences stores + eq/AudioLabStore.
//!
//! Phase 01 bootstrap stub — no public API yet. The implementing phase (see
//! docs/PROGRESS.md) fills this crate in against docs/PORT_SPEC.md; until
//! then this stub only proves the workspace seam compiles and tests.

#[cfg(test)]
mod tests {
    #[test]
    fn crate_skeleton_presents() {
        assert_eq!(env!("CARGO_PKG_NAME"), "mochy-config");
    }
}
