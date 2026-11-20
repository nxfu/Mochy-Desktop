//! PlayerCore mixer + voices, QueueMachine, Transport fades, ErrorPolicy, sleep timer, SongTuning
//!
//! Upstream absorption: MusicService.kt logic (5,667 lines decomposed) + playback/queues (7 queue types) + SleepTimer + SongTuning + PlayerConnection facade.
//!
//! Phase 01 bootstrap stub — no public API yet. The implementing phase (see
//! docs/PROGRESS.md) fills this crate in against docs/PORT_SPEC.md; until
//! then this stub only proves the workspace seam compiles and tests.

#[cfg(test)]
mod tests {
    #[test]
    fn crate_skeleton_presents() {
        assert_eq!(env!("CARGO_PKG_NAME"), "mochy-player");
    }
}
