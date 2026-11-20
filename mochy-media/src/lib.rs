//! MPRIS/SMTC/Now Playing bridges, media keys, tray, notifications
//!
//! Upstream absorption: replaces MediaSession/notification code: MusicLibrarySession custom commands + browse tree surface, Media3 notification provider, tray + non-media notification channels, hardware media-key routing.
//!
//! Phase 01 bootstrap stub — no public API yet. The implementing phase (see
//! docs/PROGRESS.md) fills this crate in against docs/PORT_SPEC.md; until
//! then this stub only proves the workspace seam compiles and tests.

#[cfg(test)]
mod tests {
    #[test]
    fn crate_skeleton_presents() {
        assert_eq!(env!("CARGO_PKG_NAME"), "mochy-media");
    }
}
