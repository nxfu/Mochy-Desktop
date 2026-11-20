//! Player dock, lyrics page, queue page, style platform, share posters
//!
//! Upstream absorption: ui/player (186 files): BottomSheetPlayer, LyricsScreen + LyricsGlowLine, QueueScreen, styles/ (11 player styles), ipod/, bedside/, studio/, share/.
//!
//! Phase 01 bootstrap stub — no public API yet. The implementing phase (see
//! docs/PROGRESS.md) fills this crate in against docs/PORT_SPEC.md; until
//! then this stub only proves the workspace seam compiles and tests.

#[cfg(test)]
mod tests {
    #[test]
    fn crate_skeleton_presents() {
        assert_eq!(env!("CARGO_PKG_NAME"), "mochy-ui-player");
    }
}
