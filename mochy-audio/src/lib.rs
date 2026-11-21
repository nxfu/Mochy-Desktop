//! Decode pipeline, DSP graph (EQ, spatial, reverb, loudness, 8D, Stage port, crossmix renderer), exclusive output router, spectrum tap
//!
//! Upstream absorption: eq/ (25 files) + playback/audio/ (incl. pass/ exclusive + USB) + cpp/stage (libmochystage, 979 .cpp lines) + crossmix/ renderer + AudioSpectrum.
//!
//! Phase 08 lands the engine half of this crate (blueprint ch. 11), module
//! by module per its commit series: [`sinc`] — the 120 dB polyphase Kaiser
//! sinc resampler (`playback/audio/SincResampler.kt`, ported first per the
//! phase prompt). The decode pipeline, ranged byte cache, OutputRouter and
//! USB driver join in the following commits of this phase; the DSP
//! processors themselves (EQ, Stage, spatial, …) land in Phases 19–20 on
//! top of the graph seams defined here.

pub mod pcm;
pub mod precision;
pub mod sinc;

#[cfg(test)]
mod tests {
    #[test]
    fn crate_skeleton_presents() {
        assert_eq!(env!("CARGO_PKG_NAME"), "mochy-audio");
    }
}
