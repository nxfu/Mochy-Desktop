//! Registry-driven settings UI, AudioLab pages, setup wizard, diagnostics
//!
//! Upstream absorption: ui/screens/settings (72 files) + audiolab/ + equalizer/ + setup/ (17 files) + devspace/ diagnostics screens.
//!
//! Phase 01 bootstrap stub — no public API yet. The implementing phase (see
//! docs/PROGRESS.md) fills this crate in against docs/PORT_SPEC.md; until
//! then this stub only proves the workspace seam compiles and tests.

#[cfg(test)]
mod tests {
    #[test]
    fn crate_skeleton_presents() {
        assert_eq!(env!("CARGO_PKG_NAME"), "mochy-ui-settings");
    }
}
