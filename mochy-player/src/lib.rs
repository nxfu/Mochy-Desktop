//! PlayerCore mixer + voices, QueueMachine, Transport fades, ErrorPolicy, sleep timer, SongTuning
//!
//! Upstream absorption: MusicService.kt logic (5,667 lines decomposed) + playback/queues (7 queue types) + SleepTimer + SongTuning + PlayerConnection facade.
//!
//! Phase 08 lands the engine (blueprint ch. 11, Table 23
//! "Mixer/voices" and "Clock"):
//!
//! - [`ring`] — the per-voice lock-free SPSC ring (~1 s, generation
//!   protocol for seeks): "the callback owns a one-second ring per
//!   voice";
//! - [`mailbox`] — the atomic parameter snapshots swapped between
//!   blocks ("params are atomic snapshots swapped between blocks",
//!   PORT_SPEC §3.1.4);
//! - [`fade`] — the crossfade gain curves by played position
//!   (`rampCrossfade`'s equal-power and s-curve ramps,
//!   MusicService.kt:5010-5091);
//! - [`clock`] — PlaybackClock: the callback's sample counter feeding
//!   the shared f64 position entity with snap/pull smoothing, read only
//!   inside paint (the 60-fps discipline, blueprint ch. 10.2);
//! - [`mixer`] — N voices summed in the callback, silence for underruns
//!   (the exclusive_pass.cpp onData policy), sum-then-limit headroom;
//! - [`core`] — PlayerCore: decode tasks with fill hysteresis,
//!   play/seek/pause/fade/crossfade-into, and the hotplug
//!   resume-at-position reinit.
//!
//! QueueMachine, ErrorPolicy, SleepTimer and SongTuning are Phase 10.

pub mod clock;
pub mod core;
pub mod fade;
pub mod mailbox;
pub mod mixer;
pub mod ring;

pub use clock::{playback_clock, PlaybackClock, SmoothedPosition};
pub use core::{DecoderSource, FrameSource, PlayerCore};
pub use fade::{FadeCurve, GainAutomation};
pub use mixer::{Mixer, MixerParams, RampSpec, MAX_VOICES};
pub use ring::{voice_ring, voice_ring_frames, RingReader, RingWriter};

#[cfg(test)]
mod tests {
    #[test]
    fn crate_skeleton_presents() {
        assert_eq!(env!("CARGO_PKG_NAME"), "mochy-player");
    }
}
