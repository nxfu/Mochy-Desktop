//! Decode pipeline, DSP graph (EQ, spatial, reverb, loudness, 8D, Stage port, crossmix renderer), exclusive output router, spectrum tap
//!
//! Upstream absorption: eq/ (25 files) + playback/audio/ (incl. pass/ exclusive + USB) + cpp/stage (libmochystage, 979 .cpp lines) + crossmix/ renderer + AudioSpectrum.
//!
//! Phase 01 bootstrap stub — no public API yet. The implementing phase (see
//! docs/PROGRESS.md) fills this crate in against docs/PORT_SPEC.md; until
//! then this stub only proves the workspace seam compiles and tests.

#[cfg(test)]
mod tests {
    #[test]
    fn crate_skeleton_presents() {
        assert_eq!(env!("CARGO_PKG_NAME"), "mochy-audio");
    }
}
