//! Home/search/library/album/artist/playlist/stats screens and their state entities
//!
//! Upstream absorption: ui/screens minus settings (HomeScreen, SearchScreen, library/, artist/, playlist/, recap/, genre + browse screens, SectionDetailScreen, HistoryScreen).
//!
//! Phase 01 bootstrap stub — no public API yet. The implementing phase (see
//! docs/PROGRESS.md) fills this crate in against docs/PORT_SPEC.md; until
//! then this stub only proves the workspace seam compiles and tests.

#[cfg(test)]
mod tests {
    #[test]
    fn crate_skeleton_presents() {
        assert_eq!(env!("CARGO_PKG_NAME"), "mochy-ui-browse");
    }
}
