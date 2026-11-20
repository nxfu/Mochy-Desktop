//! Theme tokens + Inter, list/grid items, sheets/popovers/context menus, toasts, shimmer, glass layer, seek bars, chips, index rail
//!
//! Upstream absorption: ui/component (105 files) + ui/menu (26 files) + ui/theme (8 files).
//!
//! Phase 01 bootstrap stub — no public API yet. The implementing phase (see
//! docs/PROGRESS.md) fills this crate in against docs/PORT_SPEC.md; until
//! then this stub only proves the workspace seam compiles and tests.

#[cfg(test)]
mod tests {
    #[test]
    fn crate_skeleton_presents() {
        assert_eq!(env!("CARGO_PKG_NAME"), "mochy-ui-kit");
    }
}
