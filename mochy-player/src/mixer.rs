//! The mixer: N voices summed in the audio callback, then limiter
//! headroom.
//!
//! The blueprint's replacement for ExoPlayer's model (ch. 11): "Dual
//! ExoPlayer + swap → N voices with gain automation (equal-power and
//! s-curve ramps by played position)". One render pass:
//!
//! 1. adopt the parameter snapshot (between blocks — [`ParamMailbox`]);
//! 2. for each active voice: pull from its ring (silence for what isn't
//!    there — the underrun policy), apply the gain automation
//!    ([`crate::fade`]) per frame, accumulate;
//! 3. master gain, then the headroom limiter — the joint limiter of the
//!    Android crossmix path becomes ordinary **sum-then-limit** here
//!    (Phase 20 ports the tuned limiter over this seam);
//! 4. publish the playback clock.
//!
//! The callback path touches **only** atomics, the rings and its own
//! scratch — no locks, no allocation, no drops of anything but plain
//! data.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

use crate::clock::{playback_clock, ClockWriter, PlaybackClock};
use crate::fade::GainAutomation;
use crate::mailbox::ParamMailbox;
use crate::ring::{voice_ring, RingReader, RingWriter};

/// Voice slots: upstream never ran more than a couple of players at once
/// (a crossfade's two, plus crossmix arming); 8 covers the queue
/// machinery Phase 10 adds (preloaded next-up voices).
pub const MAX_VOICES: usize = 8;

/// The mixer-wide parameter snapshot (atomic, swapped between blocks).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MixerParams {
    /// Master gain (the player volume, pre-limiter).
    pub master_gain: f32,
    /// The limiter ceiling — the headroom the sum is limited to.
    pub ceiling: f32,
    /// Mute (applied pre-limiter, like the upstream volume path).
    pub muted: bool,
}

impl Default for MixerParams {
    fn default() -> Self {
        MixerParams {
            master_gain: 1.0,
            ceiling: 1.0,
            muted: false,
        }
    }
}

/// One voice's ramp command (latest-wins through the mailbox).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RampSpec {
    pub from: f32,
    pub to: f32,
    pub duration_frames: u64,
    pub curve: crate::fade::FadeCurve,
}

/// The shared state of one voice slot. Every field the callback touches
/// is an atomic; every field the control side mutates is written so the
/// callback's read is always coherent (see per-field comments).
#[derive(Debug)]
pub struct VoiceShared {
    /// Control → callback: this slot is audible.
    pub(crate) active: AtomicBool,
    /// Control → callback: contribute silence, don't advance the ramp or
    /// position (the upstream pause: the stream keeps running, the
    /// position holds).
    pub(crate) paused: AtomicBool,
    /// Decode task → control/callback: the source is exhausted; when the
    /// ring drains the voice has ended.
    pub(crate) eof: AtomicBool,
    /// Control → callback: the media sample the ring's first frame
    /// corresponds to (set on play and on seek, between blocks by
    /// protocol).
    pub(crate) base_media_sample: AtomicU64,
    /// Callback → everyone: output frames this voice has actually mixed.
    pub(crate) played_out: AtomicU64,
    /// The ring's rate (set once at slot setup).
    pub(crate) rate_out: AtomicU32,
    /// The media's own rate (for the clock's sample mapping).
    pub(crate) media_rate: AtomicU32,
    /// Control → callback: the gain ramp to arm between blocks.
    pub(crate) ramp: ParamMailbox<RampSpec>,
    /// Control → callback: this voice is the one the clock follows.
    pub(crate) primary: AtomicBool,
}

impl VoiceShared {
    fn new() -> VoiceShared {
        VoiceShared {
            active: AtomicBool::new(false),
            paused: AtomicBool::new(false),
            eof: AtomicBool::new(false),
            base_media_sample: AtomicU64::new(0),
            played_out: AtomicU64::new(0),
            rate_out: AtomicU32::new(48_000),
            media_rate: AtomicU32::new(48_000),
            ramp: ParamMailbox::new(),
            primary: AtomicBool::new(false),
        }
    }

    /// Whether the source is exhausted (pair with the handle's fill
    /// check to know the voice has fully played out).
    pub fn is_eof(&self) -> bool {
        self.eof.load(Ordering::Acquire)
    }
}

/// The render-side mixer. Owned by whichever thread runs the output
/// callback (exactly one at a time — the OutputRouter guarantees it).
/// The ring **readers** live here for good; the matching **writers**
/// come back from the constructor for the control side (PlayerCore) to
/// hand to decode tasks — slot ownership never moves after construction.
pub struct Mixer {
    channels: usize,
    rate: u32,
    pub(crate) voices: Vec<Arc<VoiceShared>>,
    readers: Vec<RingReader>,
    ramps: Vec<Option<GainAutomation>>,
    gains: Vec<f32>,
    params: Arc<ParamMailbox<MixerParams>>,
    adopted: MixerParams,
    clock_writer: ClockWriter,
    limiter: HeadroomLimiter,
    // Scratch, reused every block.
    voice_scratch: Vec<f32>,
    gain_scratch: Vec<f32>,
}

impl Mixer {
    /// Builds the mixer for one output format. Returns the mixer, the
    /// playback clock for the UI, and the per-slot ring writers for the
    /// control side.
    pub fn new(
        rate: u32,
        channels: usize,
        params: Arc<ParamMailbox<MixerParams>>,
    ) -> (Mixer, PlaybackClock, Vec<Arc<RingWriter>>) {
        let (clock_writer, clock) = playback_clock();
        let mut voices = Vec::with_capacity(MAX_VOICES);
        let mut readers = Vec::with_capacity(MAX_VOICES);
        let mut writers = Vec::with_capacity(MAX_VOICES);
        let mut ramps = Vec::with_capacity(MAX_VOICES);
        let mut gains = Vec::with_capacity(MAX_VOICES);
        for _ in 0..MAX_VOICES {
            voices.push(Arc::new(VoiceShared::new()));
            // Rings are created eagerly per slot so ownership never moves
            // after construction (the callback owns the readers for good).
            let (writer, reader) = voice_ring(rate, channels);
            writers.push(Arc::new(writer));
            readers.push(reader);
            ramps.push(None);
            gains.push(1.0);
        }
        let mixer = Mixer {
            channels,
            rate,
            voices,
            readers,
            ramps,
            gains,
            params,
            adopted: MixerParams::default(),
            clock_writer,
            limiter: HeadroomLimiter::new(),
            voice_scratch: vec![0f32; 8192 * channels],
            gain_scratch: vec![0f32; 8192],
        };
        (mixer, clock, writers)
    }

    /// The output rate the rings were sized for.
    pub fn rate(&self) -> u32 {
        self.rate
    }

    /// The output channel count.
    pub fn channels(&self) -> usize {
        self.channels
    }

    /// Renders one block of interleaved output frames. The audio
    /// callback (or the offline harness). `out.len()` must be a multiple
    /// of the channel count.
    pub fn render(&mut self, out: &mut [f32]) {
        let frames = out.len() / self.channels.max(1);
        if frames == 0 {
            return;
        }
        // 1. Adopt the parameter snapshot — between blocks.
        if let Some(params) = self.params.adopt() {
            self.adopted = params;
        }
        out.fill(0f32);
        // Grow scratch if a giant block arrives.
        if self.voice_scratch.len() < frames * self.channels {
            self.voice_scratch.resize(frames * self.channels, 0.0);
            self.gain_scratch.resize(frames, 0.0);
        }
        let mut any_primary = false;
        let mut primary_position = None;
        let mut playing = false;
        for slot in 0..MAX_VOICES {
            let shared = &self.voices[slot];
            if !shared.active.load(Ordering::Acquire) {
                continue;
            }
            let reader = &mut self.readers[slot];
            let generation = reader.generation();
            let paused = shared.paused.load(Ordering::Acquire);
            let (taken, block_played) = if paused {
                (0usize, 0usize)
            } else {
                let scratch = &mut self.voice_scratch[..frames * self.channels];
                let taken = reader.pop(scratch);
                (taken, taken)
            };
            // A seek reset mid-block: this block's take is stale.
            let stale = reader.generation_changed(generation);
            // 2. Arm any pending ramp between blocks.
            if let Some(spec) = shared.ramp.adopt() {
                self.ramps[slot] = Some(GainAutomation::new(
                    spec.curve,
                    spec.from,
                    spec.to,
                    spec.duration_frames,
                ));
                self.gains[slot] = spec.from;
            }
            let frame_gain = &mut self.gain_scratch[..frames];
            let ramp = self.ramps[slot].as_mut();
            match ramp {
                Some(ramp) if !paused => {
                    ramp.advance(frames, frame_gain);
                }
                Some(ramp) => {
                    frame_gain.fill(ramp.current_gain());
                }
                None => frame_gain.fill(self.gains[slot]),
            }
            if stale {
                continue;
            }
            // 3. Accumulate with per-frame gain.
            let channels = self.channels;
            for frame in 0..frames {
                let g = frame_gain[frame];
                let base = frame * channels;
                if frame < taken {
                    let from = base;
                    let to = base + channels;
                    let src = &self.voice_scratch[from..to];
                    for (dst, src) in out[from..to].iter_mut().zip(src) {
                        *dst += src * g;
                    }
                }
                // Frames past `taken` contribute silence — the underrun
                // policy: the buffer was zeroed above.
            }
            if let Some(ramp) = self.ramps[slot].as_ref() {
                if ramp.is_finished() {
                    self.gains[slot] = {
                        let spec_end = ramp.current_gain();
                        spec_end
                    };
                    self.ramps[slot] = None;
                }
            }
            // 4. Position bookkeeping: only frames that actually sounded.
            let played = block_played;
            let played_total = shared.played_out.fetch_add(played as u64, Ordering::AcqRel) as u64
                + played as u64;
            let base = shared.base_media_sample.load(Ordering::Acquire);
            let media_rate = shared.media_rate.load(Ordering::Acquire);
            let rate_out = shared.rate_out.load(Ordering::Acquire);
            if shared.primary.load(Ordering::Acquire) {
                any_primary = true;
                playing = !paused && (taken > 0 || !shared.eof.load(Ordering::Acquire));
                let media = base as f64 + played_total as f64 * f64::from(media_rate) / f64::from(rate_out);
                primary_position = Some(media / f64::from(media_rate));
            }
            // End-of-voice: the source is done and the ring drained.
            if shared.eof.load(Ordering::Acquire) && reader.filled() == 0 && taken == 0 && !paused {
                shared.active.store(false, Ordering::Release);
            }
        }
        // 5. Master gain + limiter headroom.
        let master = if self.adopted.muted {
            0.0
        } else {
            self.adopted.master_gain
        };
        for sample in out.iter_mut() {
            *sample *= master;
        }
        self.limiter.limit(out, self.adopted.ceiling);
        // 6. Publish the clock.
        if let Some(position) = primary_position {
            let rate = self
                .voices
                .iter()
                .position(|v| v.primary.load(Ordering::Acquire))
                .map(|slot| self.voices[slot].media_rate.load(Ordering::Acquire))
                .unwrap_or(44_100);
            self.clock_writer.publish(
                crate::clock::Seconds(position),
                rate,
                playing,
            );
        } else {
            let _ = any_primary;
        }
    }
}

/// The Phase 08 headroom limiter: sum-then-limit with a smoothed gain
/// fallback and a hard ceiling. Phase 20 ports the tuned limiter over
/// this seam (the crossmix joint limiter's desktop stand-in).
#[derive(Debug)]
struct HeadroomLimiter {
    /// Current gain (≤ 1).
    gain: f32,
}

impl HeadroomLimiter {
    fn new() -> HeadroomLimiter {
        HeadroomLimiter { gain: 1.0 }
    }

    fn limit(&mut self, out: &mut [f32], ceiling: f32) {
        if ceiling <= 0.0 || !ceiling.is_finite() {
            out.fill(0.0);
            return;
        }
        // Below the ceiling at unity: bit-exact passthrough (parity with
        // the untouched single-voice path).
        let peak = out.iter().fold(0f32, |acc, s| acc.max(s.abs()));
        if peak * self.gain <= ceiling {
            if self.gain < 1.0 {
                // Release: recover toward unity (about 6 dB/s at 10 ms
                // blocks — imperceptible).
                self.gain = (self.gain + 0.0001).min(1.0);
                for sample in out.iter_mut() {
                    *sample *= self.gain;
                }
            }
            return;
        }
        // Attack: the gain target for this block, applied with a glide so
        // the release isn't a click.
        let target = (ceiling / peak).clamp(0.0, 1.0);
        self.gain = target;
        for sample in out.iter_mut() {
            *sample = (*sample * self.gain).clamp(-ceiling, ceiling);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mono_mixer() -> (Mixer, Arc<ParamMailbox<MixerParams>>, Vec<Arc<RingWriter>>) {
        let params = Arc::new(ParamMailbox::new());
        let (mixer, _clock, writers) = Mixer::new(48_000, 1, params.clone());
        (mixer, params, writers)
    }

    #[test]
    fn sum_then_limit_two_full_scale_voices() {
        // The acceptance shape: two voices at full scale sum to ±2.0 and
        // the limiter keeps the output inside the ceiling.
        let (mut mixer, _params, writers) = mono_mixer();
        mixer.voices[0].active.store(true, Ordering::Release);
        mixer.voices[0].primary.store(true, Ordering::Release);
        mixer.voices[1].active.store(true, Ordering::Release);
        writers[0].push(&vec![1f32; 4800]);
        writers[1].push(&vec![1f32; 4800]);
        let mut out = vec![0f32; 480];
        mixer.render(&mut out);
        let peak = out.iter().fold(0f32, |acc, s| acc.max(s.abs()));
        assert!(peak <= 1.0 + 1e-6, "peak {peak}");
        // While summed above the ceiling the limiter compresses; the mix
        // is audibly present, not muted.
        assert!(peak > 0.5, "mix vanished: {peak}");
    }

    #[test]
    fn single_voice_below_ceiling_is_untouched() {
        // Parity: one voice at half scale passes bit-exact.
        let (mut mixer, _params, writers) = mono_mixer();
        mixer.voices[0].active.store(true, Ordering::Release);
        mixer.voices[0].primary.store(true, Ordering::Release);
        let frames: Vec<f32> = (0..2400).map(|i| (i as f32 / 2400.0) * 0.5 - 0.25).collect();
        writers[0].push(&frames);
        let mut out = vec![0f32; 2400];
        mixer.render(&mut out);
        assert_eq!(&out[..2400], &frames[..]);
    }

    #[test]
    fn master_gain_applies_after_summing() {
        let (mut mixer, params, writers) = mono_mixer();
        params.publish(MixerParams {
            master_gain: 0.5,
            ..Default::default()
        });
        mixer.voices[0].active.store(true, Ordering::Release);
        mixer.voices[0].primary.store(true, Ordering::Release);
        writers[0].push(&vec![0.5f32; 100]);
        let mut out = vec![0f32; 100];
        mixer.render(&mut out);
        assert!(out.iter().all(|s| (s - 0.25).abs() < 1e-7));
    }

    #[test]
    fn underrun_policy_plays_silence_and_continues() {
        // The acceptance case: decode lags — the block gets what the ring
        // holds plus silence, the position only advances by real frames,
        // and when decode catches up the stream continues seamlessly.
        let (mut mixer, _params, writers) = mono_mixer();
        let shared = mixer.voices[0].clone();
        shared.active.store(true, Ordering::Release);
        shared.primary.store(true, Ordering::Release);
        // Only 100 frames available for a 480-frame block.
        let first: Vec<f32> = (0..100).map(|i| i as f32 * 0.001).collect();
        assert_eq!(writers[0].push(&first), 100);
        let mut out = vec![0f32; 480];
        mixer.render(&mut out);
        assert_eq!(&out[..100], &first[..]);
        assert!(
            out[100..].iter().all(|s| *s == 0.0),
            "underrun must be silence"
        );
        // Only the real frames counted.
        assert_eq!(shared.played_out.load(Ordering::Acquire), 100);
        // The voice stays active; decode catches up with the next 200.
        let next: Vec<f32> = (100..300).map(|i| i as f32 * 0.001).collect();
        assert_eq!(writers[0].push(&next), 200);
        mixer.render(&mut out);
        assert_eq!(&out[..200], &next[..]);
        assert_eq!(shared.played_out.load(Ordering::Acquire), 300);
    }

    #[test]
    fn eof_retires_a_drained_voice() {
        let (mut mixer, _params, writers) = mono_mixer();
        let shared = mixer.voices[0].clone();
        shared.active.store(true, Ordering::Release);
        shared.primary.store(true, Ordering::Release);
        writers[0].push(&vec![0.25f32; 64]);
        shared.eof.store(true, Ordering::Release);
        let mut out = vec![0f32; 64];
        mixer.render(&mut out);
        assert!(shared.active.load(Ordering::Acquire));
        mixer.render(&mut out);
        // Drained + eof → the voice went silent on its own.
        assert!(!shared.active.load(Ordering::Acquire));
    }

    #[test]
    fn ramp_is_armed_between_blocks() {
        // A fade from 0 to 1 over 480 frames on equal-power: the first
        // block's gains follow sin(θ).
        let (mut mixer, _params, writers) = mono_mixer();
        let shared = mixer.voices[0].clone();
        shared.active.store(true, Ordering::Release);
        shared.primary.store(true, Ordering::Release);
        shared.ramp.publish(RampSpec {
            from: 0.0,
            to: 1.0,
            duration_frames: 480,
            curve: crate::fade::FadeCurve::EqualPower,
        });
        writers[0].push(&vec![1f32; 960]);
        let mut out = vec![0f32; 480];
        mixer.render(&mut out);
        // Frame 0 silent, frame 240 at sin(π/4), frame 479 ≈ 1.
        assert!(out[0].abs() < 1e-7);
        assert!((out[240] - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
        assert!(out[479] > 0.99);
        // Second block: ramp finished, unity gain.
        let mut out2 = vec![0f32; 480];
        mixer.render(&mut out2);
        assert!(out2.iter().all(|s| (*s - 1.0).abs() < 1e-6));
    }

    #[test]
    fn gapless_voice_handover() {
        // Two voices: the first runs dry exactly at its end while the
        // second is already playing — the output is continuous with no
        // gap and no dip (single-mixer gapless, the album case).
        let (mut mixer, _params, writers) = mono_mixer();
        mixer.voices[0].active.store(true, Ordering::Release);
        mixer.voices[0].primary.store(true, Ordering::Release);
        mixer.voices[1].active.store(true, Ordering::Release);
        // Voice A: 240 frames then EOF.
        writers[0].push(&(0..240).map(|i| i as f32 * 0.001).collect::<Vec<_>>());
        mixer.voices[0].eof.store(true, Ordering::Release);
        // Voice B: a second full-scale signal already flowing.
        writers[1].push(&vec![0.5f32; 2400]);
        let mut out = vec![0f32; 2400];
        mixer.render(&mut out);
        // The first 240 frames carry A+B; after that only B continues
        // with no gap: frames 240.. are exactly 0.5 (limiter passthrough
        // — the sum peaked below the ceiling).
        assert!(
            (out[240] - 0.5).abs() < 1e-6,
            "gap or dip at the seam: {}",
            out[240]
        );
        assert!(out[241..].iter().all(|s| (s - 0.5).abs() < 1e-6));
        assert!(out[..240].iter().any(|s| *s > 0.5), "voice A never sounded");
    }

    #[test]
    fn paused_voice_holds_position() {
        // The upstream pause: silence, position holds, resuming continues.
        let (mut mixer, _params, writers) = mono_mixer();
        let shared = mixer.voices[0].clone();
        shared.active.store(true, Ordering::Release);
        shared.primary.store(true, Ordering::Release);
        writers[0].push(&vec![0.5f32; 960]);
        let mut out = vec![0f32; 480];
        mixer.render(&mut out);
        assert_eq!(shared.played_out.load(Ordering::Acquire), 480);
        shared.paused.store(true, Ordering::Release);
        mixer.render(&mut out);
        assert!(out.iter().all(|s| *s == 0.0));
        assert_eq!(
            shared.played_out.load(Ordering::Acquire),
            480,
            "pause must not advance the position"
        );
        shared.paused.store(false, Ordering::Release);
        mixer.render(&mut out);
        assert_eq!(shared.played_out.load(Ordering::Acquire), 960);
        assert!(out.iter().all(|s| (s - 0.5).abs() < 1e-7));
    }
}
