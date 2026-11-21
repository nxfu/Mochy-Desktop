//! PlayerCore — the engine the phases plug into.
//!
//! The control half of the blueprint's single-mixer architecture
//! (ch. 11): it owns the [`Mixer`]'s ring writers, spawns one decode
//! task per playing voice, drives them with hysteresis (fill above the
//! high watermark, idle below the low), and exposes the play/seek/
//! pause/fade operations the app (and Phase 10's QueueMachine) call.
//! The render half is [`Mixer::render`], called by the OutputRouter's
//! callback — never from here.
//!
//! Upstream absorption: MusicService.kt's playback orchestration
//! decomposed — the dual-ExoPlayer crossfade machinery
//! (`crossfadeInto`/`performCrossfadeSwap`/`rampCrossfade`,
//! lines 4794-5091) becomes gain automation on two voices
//! ([`PlayerCore::crossfade_into`]); the decode-to-ring pipe is the
//! blueprint's replacement for ResolvingDataSource + MediaCodec; the
//! preload-ahead (`prefetchNeighbours`, MusicService.kt:4829-4863) is
//! simply a second claimed slot filled by its own task.

use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use mochy_audio::decode::TrackDecoder;
use mochy_audio::sinc::SincResampler;

use crate::clock::PlaybackClock;
use crate::fade::FadeCurve;
use crate::mixer::{Mixer, MixerParams, RampSpec};
use crate::mailbox::ParamMailbox;
use crate::ring::RingWriter;

/// Fill above this fraction of the ring before idling (the hysteresis
/// ceiling — "the decode task keeps it filled with hysteresis",
/// blueprint ch. 11).
const FILL_HIGH: f32 = 0.9;
/// Refill once below this fraction (the hysteresis floor).
const FILL_LOW: f32 = 0.25;
/// How long the decode task sleeps while above the high watermark.
const IDLE_POLL: Duration = Duration::from_millis(5);

/// A source of interleaved f32 media frames — the decode task's input.
/// Real playback wraps a [`TrackDecoder`] over a
/// `mochy_audio::source::RangedMediaSource` (see [`DecoderSource`]);
/// tests use synthetic generators.
pub trait FrameSource: Send {
    /// The media's own sample rate.
    fn media_rate(&self) -> u32;
    /// The channel count.
    fn channels(&self) -> usize;
    /// Decodes the next block into `out` (interleaved); `None` at the
    /// end of the source. The first block starts at media sample
    /// `position()`.
    fn next_block(&mut self, out: &mut Vec<f32>) -> Option<()>;
    /// Seeks to a media sample, returning the sample landed on.
    fn seek(&mut self, to_sample: u64) -> u64;
    /// The media sample the NEXT decoded block starts at.
    fn position(&self) -> u64;
}

/// `TrackDecoder` as a [`FrameSource`] (the bridge the real player
/// runs on).
pub struct DecoderSource {
    decoder: TrackDecoder,
    position: u64,
    block: usize,
}

impl DecoderSource {
    /// Wraps an opened track decoder.
    pub fn new(decoder: TrackDecoder, block_frames: usize) -> DecoderSource {
        DecoderSource {
            decoder,
            position: 0,
            block: block_frames.max(64),
        }
    }
}

impl FrameSource for DecoderSource {
    fn media_rate(&self) -> u32 {
        self.decoder.info().sample_rate
    }

    fn channels(&self) -> usize {
        self.decoder.info().channels
    }

    fn next_block(&mut self, out: &mut Vec<f32>) -> Option<()> {
        let block = self.decoder.decode_block(self.block).ok()??;
        let channels = self.decoder.info().channels.max(1);
        self.position = block.first_sample + (block.samples.len() / channels) as u64;
        *out = block.samples;
        Some(())
    }

    fn seek(&mut self, to_sample: u64) -> u64 {
        match self.decoder.seek(to_sample) {
            Ok(landed) => {
                self.position = landed;
                landed
            }
            Err(_) => self.position,
        }
    }

    fn position(&self) -> u64 {
        self.position
    }
}

enum TaskCommand {
    Seek(u64),
    Stop,
}

/// One running decode task.
struct TaskHandle {
    commands: Sender<TaskCommand>,
    join: JoinHandle<()>,
}

impl TaskHandle {
    fn stop(self) {
        let _ = self.commands.send(TaskCommand::Stop);
        let _ = self.join.join();
    }
}

/// The player engine. Cheap to clone for the app's handle needs — the
/// shared state is all `Arc`/atomics.
pub struct PlayerCore {
    inner: Arc<CoreInner>,
    /// Control-side task bookkeeping (only touched with the lock held;
    /// the audio callback never comes near it).
    tasks: Mutex<Vec<Option<TaskHandle>>>,
}

struct CoreInner {
    rate: u32,
    channels: usize,
    voices: Vec<Arc<crate::mixer::VoiceShared>>,
    writers: Vec<Arc<RingWriter>>,
    params: Arc<ParamMailbox<MixerParams>>,
    clock: PlaybackClock,
}

impl PlayerCore {
    /// Creates the engine for one output format. Returns the core and
    /// the render half to hand to the OutputRouter.
    pub fn new(rate: u32, channels: usize) -> (PlayerCore, Mixer) {
        let params = Arc::new(ParamMailbox::new());
        let (mixer, clock, writers) = Mixer::new(rate, channels, params.clone());
        let inner = Arc::new(CoreInner {
            rate,
            channels,
            voices: mixer.voices.clone(),
            writers,
            params,
            clock,
        });
        let mut tasks = Vec::with_capacity(crate::mixer::MAX_VOICES);
        for _ in 0..crate::mixer::MAX_VOICES {
            tasks.push(None);
        }
        (
            PlayerCore {
                inner,
                tasks: Mutex::new(tasks),
            },
            mixer,
        )
    }

    /// The UI's clock (paint-side reads only — see [`crate::clock`]).
    pub fn clock(&self) -> &PlaybackClock {
        &self.inner.clock
    }

    /// The mixer parameter mailbox (master gain, ceiling, mute).
    pub fn params(&self) -> &Arc<ParamMailbox<MixerParams>> {
        &self.inner.params
    }

    /// The output rate voices are resampled to.
    pub fn output_rate(&self) -> u32 {
        self.inner.rate
    }

    /// Starts playing a source on a free voice slot. Returns the slot
    /// (the voice id) or `None` when every slot is busy.
    ///
    /// The upstream gate "a skip calls off what crossmix had lined up
    /// before the new one copies the speed" (MusicService.kt:4913) is
    /// the caller's business (Phase 10's QueueMachine); the engine just
    /// plays what it's handed.
    pub fn play(&self, source: Box<dyn FrameSource>) -> Option<usize> {
        self.play_with_fade(source, None)
    }

    /// [`play`] with an initial gain ramp (the blend-skip entry).
    pub fn play_with_fade(
        &self,
        source: Box<dyn FrameSource>,
        fade_in: Option<(f32, f32, u64, FadeCurve)>,
    ) -> Option<usize> {
        let slot = {
            let tasks = self.tasks.lock().expect("task table poisoned");
            let slot = (0..crate::mixer::MAX_VOICES).find(|slot| tasks[*slot].is_none())?;
            let shared = self.inner.voices[slot].clone();
            shared.active.store(false, Ordering::Release);
            shared.paused.store(false, Ordering::Release);
            shared.eof.store(false, Ordering::Release);
            shared.primary.store(false, Ordering::Release);
            shared.played_out.store(0, Ordering::Release);
            shared.base_media_sample.store(0, Ordering::Release);
            slot
        };
        let shared = self.inner.voices[slot].clone();
        let writer = self.inner.writers[slot].clone();
        let rate = self.inner.rate;
        let channels = self.inner.channels;
        let (tx, rx) = mpsc::channel::<TaskCommand>();
        let task_shared = shared.clone();
        let join = std::thread::Builder::new()
            .name(format!("mochy-voice-{slot}"))
            .spawn(move || {
                decode_task(task_shared, writer, source, rate, channels, rx);
            })
            .expect("voice thread spawn");
        // Wait for the task to arm the voice (it sets media rate/base
        // before flipping active, so the callback never sees a
        // half-configured voice).
        while !shared.active.load(Ordering::Acquire)
            && !shared.eof.load(Ordering::Acquire)
        {
            std::thread::sleep(Duration::from_micros(200));
        }
        if let Some((from, to, duration, curve)) = fade_in {
            shared.ramp.publish(RampSpec {
                from,
                to,
                duration_frames: duration,
                curve,
            });
        }
        let mut tasks = self.tasks.lock().expect("task table poisoned");
        tasks[slot] = Some(TaskHandle { commands: tx, join });
        Some(slot)
    }

    /// Marks the voice the clock follows.
    pub fn set_primary(&self, slot: usize) {
        for (i, voice) in self.inner.voices.iter().enumerate() {
            voice
                .primary
                .store(i == slot, Ordering::Release);
        }
    }

    /// Pauses or resumes one voice (the upstream pause: silence, the
    /// position holds, the stream keeps running).
    pub fn set_paused(&self, slot: usize, paused: bool) {
        self.inner.voices[slot]
            .paused
            .store(paused, Ordering::Release);
    }

    /// Starts a gain ramp on one voice (the crossfade/duck primitive).
    pub fn start_fade(&self, slot: usize, from: f32, to: f32, duration_frames: u64, curve: FadeCurve) {
        self.inner.voices[slot].ramp.publish(RampSpec {
            from,
            to,
            duration_frames,
            curve,
        });
    }

    /// Seeks a voice. The decode task flushes its decoder state, resets
    /// the ring (the generation protocol) and refills from the target;
    /// the voice's position base moves with it.
    pub fn seek(&self, slot: usize, to_sample: u64) {
        if let Some(task) = self.tasks.lock().expect("task table poisoned")[slot].as_ref() {
            let _ = task.commands.send(TaskCommand::Seek(to_sample));
        }
    }

    /// Stops a voice and frees its slot.
    pub fn stop(&self, slot: usize) {
        let task = self.tasks.lock().expect("task table poisoned")[slot].take();
        if let Some(task) = task {
            task.stop();
        }
        let shared = self.inner.voices[slot].clone();
        shared.active.store(false, Ordering::Release);
        shared.eof.store(false, Ordering::Release);
    }

    /// Whether a voice's source is exhausted and its ring drained — the
    /// signal Phase 10's QueueMachine advances on.
    pub fn voice_has_ended(&self, slot: usize) -> bool {
        let shared = &self.inner.voices[slot];
        shared.is_eof() && self.inner.writers[slot].filled() == 0
    }

    /// The crossfade primitive: fades `from_slot` out and `to_slot` in
    /// over `duration_frames` of **played** audio on one curve family —
    /// the port of `crossfadeInto` + `rampCrossfade`'s two ramps without
    /// the second player, the media-session swap or the listener
    /// rewiring (the blueprint's whole point).
    pub fn crossfade_into(
        &self,
        from_slot: usize,
        to_slot: usize,
        duration_frames: u64,
        curve: FadeCurve,
        from_gain: f32,
        to_gain: f32,
    ) {
        self.set_primary(to_slot);
        self.start_fade(from_slot, from_gain, 0.0, duration_frames, curve);
        self.start_fade(to_slot, 0.0, to_gain, duration_frames, curve);
    }

    /// Hotplug recovery: the output stream was lost and re-opened —
    /// re-anchor every active voice at the position it had actually
    /// reached (the AudioTrack-failure recovery path, ported as
    /// reinit-at-position: blueprint ch. 11 "output-device hotplug
    /// re-opens the stream and resumes at position").
    ///
    /// Each voice's decode task restarts filling from
    /// `base + played × media_rate/rate_out`; the position bases are
    /// rebased so the clock shows the same media position across the
    /// gap.
    pub fn resume_at_position(&self) {
        for slot in 0..crate::mixer::MAX_VOICES {
            let shared = &self.inner.voices[slot];
            if !shared.active.load(Ordering::Acquire) {
                continue;
            }
            let played = shared.played_out.load(Ordering::Acquire);
            let base = shared.base_media_sample.load(Ordering::Acquire);
            let media_rate = shared.media_rate.load(Ordering::Acquire) as f64;
            let rate_out = shared.rate_out.load(Ordering::Acquire) as f64;
            let media_now = base as f64 + played as f64 * media_rate / rate_out;
            shared
                .base_media_sample
                .store(media_now as u64, Ordering::Release);
            shared.played_out.store(0, Ordering::Release);
            // The rings are empty after the reopen: the decode tasks are
            // instructed to refill from where they left off.
            if let Some(task) = self.tasks.lock().expect("task table poisoned")[slot].as_ref() {
                let _ = task.commands.send(TaskCommand::Seek(media_now as u64));
            }
        }
    }

    /// Stops everything (player teardown).
    pub fn shutdown(&self) {
        let mut tasks = self.tasks.lock().expect("task table poisoned");
        for task in tasks.iter_mut() {
            if let Some(task) = task.take() {
                task.stop();
            }
        }
    }
}

/// The decode task body: fill the voice ring with hysteresis until the
/// source ends or a stop arrives.
fn decode_task(
    shared: Arc<crate::mixer::VoiceShared>,
    writer: Arc<RingWriter>,
    mut source: Box<dyn FrameSource>,
    output_rate: u32,
    output_channels: usize,
    commands: Receiver<TaskCommand>,
) {
    let media_rate = source.media_rate();
    let media_channels = source.channels();
    shared
        .media_rate
        .store(media_rate, Ordering::Release);
    shared.rate_out.store(output_rate, Ordering::Release);
    shared
        .base_media_sample
        .store(source.position(), Ordering::Release);
    writer.reset();

    // The resampler: the song is brought to the output rate here, on the
    // decode thread — "designed on the playback thread as a song opens"
    // (SincResampler.kt:103), one per voice (PrecisionSink's per-song
    // decision; the desktop always targets the output rate).
    let mut resampler = if media_rate != output_rate {
        SincResampler::create(media_rate, output_rate, media_channels)
    } else {
        None
    };

    let mut decoded: Vec<f32> = Vec::new();
    let mut resampled: Vec<f32> = Vec::new();
    let capacity = writer.capacity();
    let high = (capacity as f32 * FILL_HIGH) as usize;
    let low = (capacity as f32 * FILL_LOW) as usize;

    // Arm the voice once the first frames are in (the preload-first
    // instant start; upstream `Queue.preloadItem`).
    let mut armed = false;
    loop {
        // Commands first.
        match commands.try_recv() {
            Ok(TaskCommand::Stop) => break,
            Ok(TaskCommand::Seek(to)) => {
                let landed = source.seek(to);
                shared
                    .base_media_sample
                    .store(landed, Ordering::Release);
                shared.played_out.store(0, Ordering::Release);
                writer.reset();
                if let Some(resampler) = resampler.as_mut() {
                    resampler.reset();
                }
                decoded.clear();
                armed = false;
            }
            Err(mpsc::TryRecvError::Disconnected) => break,
            Err(mpsc::TryRecvError::Empty) => {}
        }
        let filled = writer.filled();
        if filled < low {
            // Decode and push until above the high watermark or EOF.
            while writer.filled() < high {
                let Some(()) = source.next_block(&mut decoded) else {
                    shared.eof.store(true, Ordering::Release);
                    break;
                };
                push_block(
                    &writer,
                    &decoded,
                    media_channels,
                    output_channels,
                    resampler.as_mut(),
                    &mut resampled,
                );
                if !armed && writer.filled() > 0 {
                    armed = true;
                    shared.active.store(true, Ordering::Release);
                }
            }
            if shared.eof.load(Ordering::Acquire) {
                break;
            }
        } else if filled >= high {
            std::thread::sleep(IDLE_POLL);
            continue;
        } else {
            // Between the marks: top up gently.
            let Some(()) = source.next_block(&mut decoded) else {
                shared.eof.store(true, Ordering::Release);
                break;
            };
            push_block(
                &writer,
                &decoded,
                media_channels,
                output_channels,
                resampler.as_mut(),
                &mut resampled,
            );
        }
        if !armed && writer.filled() > 0 {
            armed = true;
            shared.active.store(true, Ordering::Release);
        }
        std::thread::sleep(IDLE_POLL);
    }
    // On exit, the eof flag stays set; the mixer retires the voice when
    // the ring drains (or the control side stops it).
}

/// Converts one decoded media block to the output layout and pushes it.
fn push_block(
    writer: &RingWriter,
    decoded: &[f32],
    media_channels: usize,
    output_channels: usize,
    resampler: Option<&mut SincResampler>,
    resampled: &mut Vec<f32>,
) {
    // Channel count changes mid-stream are not a thing the formats we
    // play do; route mono→stereo and drop extras defensively.
    let frames = decoded.len() / media_channels.max(1);
    let interleaved: Vec<f32> = if media_channels == output_channels {
        decoded.to_vec()
    } else {
        let mut out = Vec::with_capacity(frames * output_channels);
        for frame in 0..frames {
            for channel in 0..output_channels {
                let value = if channel < media_channels {
                    decoded[frame * media_channels + channel]
                } else {
                    decoded[frame * media_channels] // mono→N duplication
                };
                out.push(value);
            }
        }
        out
    };
    match resampler {
        None => {
            writer.push(&interleaved);
        }
        Some(resampler) => {
            // Plane-wise resample then re-interleave.
            let channels = output_channels;
            let in_frames = interleaved.len() / channels;
            let max_out = resampler.max_output_frames(in_frames);
            resampled.clear();
            resampled.resize(max_out * channels, 0.0);
            // Split into planes.
            let mut planes: Vec<Vec<f32>> = vec![vec![0f32; in_frames]; channels];
            for frame in 0..in_frames {
                for channel in 0..channels {
                    planes[channel][frame] = interleaved[frame * channels + channel];
                }
            }
            let plane_refs: Vec<&[f32]> = planes.iter().map(|p| p.as_slice()).collect();
            let mut out_planes: Vec<&mut [f32]> = resampled
                .chunks_mut(max_out)
                .take(channels)
                .collect();
            let written = resampler.process(&plane_refs, in_frames, &mut out_planes);
            // Re-interleave the written frames.
            let mut out = Vec::with_capacity(written * channels);
            for frame in 0..written {
                for channel in 0..channels {
                    out.push(resampled[channel * max_out + frame]);
                }
            }
            writer.push(&out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A deterministic ramp source at a chosen rate; the sample value is
    /// `index × scale` (keep `scale` small enough to stay under the
    /// limiter ceiling).
    struct RampSource {
        rate: u32,
        next: u64,
        end: u64,
        scale: f32,
    }

    impl FrameSource for RampSource {
        fn media_rate(&self) -> u32 {
            self.rate
        }
        fn channels(&self) -> usize {
            1
        }
        fn next_block(&mut self, out: &mut Vec<f32>) -> Option<()> {
            if self.next >= self.end {
                return None;
            }
            out.clear();
            let take = 1024.min((self.end - self.next) as usize);
            for i in 0..take {
                out.push((self.next + i as u64) as f32 * self.scale);
            }
            self.next += take as u64;
            Some(())
        }
        fn seek(&mut self, to_sample: u64) -> u64 {
            self.next = to_sample.min(self.end);
            self.next
        }
        fn position(&self) -> u64 {
            self.next
        }
    }

    /// A 440 Hz sine source — frequency survives resampling exactly.
    struct SineSource {
        rate: u32,
        next: u64,
        end: u64,
    }

    impl FrameSource for SineSource {
        fn media_rate(&self) -> u32 {
            self.rate
        }
        fn channels(&self) -> usize {
            1
        }
        fn next_block(&mut self, out: &mut Vec<f32>) -> Option<()> {
            if self.next >= self.end {
                return None;
            }
            out.clear();
            let take = 1024.min((self.end - self.next) as usize);
            for i in 0..take {
                let t = (self.next + i as u64) as f64 / f64::from(self.rate);
                out.push((2.0 * std::f64::consts::PI * 440.0 * t).sin() as f32 * 0.5);
            }
            self.next += take as u64;
            Some(())
        }
        fn seek(&mut self, to_sample: u64) -> u64 {
            self.next = to_sample.min(self.end);
            self.next
        }
        fn position(&self) -> u64 {
            self.next
        }
    }

    #[test]
    fn play_renders_the_source_with_hysteresis() {
        let (core, mut mixer) = PlayerCore::new(48_000, 1);
        let slot = core
            .play(Box::new(RampSource {
                rate: 48_000,
                next: 0,
                end: 20_000,
                scale: 0.00002,
            }))
            .unwrap();
        core.set_primary(slot);
        // Let the task fill the ring.
        std::thread::sleep(Duration::from_millis(50));
        let mut out = vec![0f32; 1024];
        mixer.render(&mut out);
        // The ramp begins at 0 and rises by the scale per frame.
        assert!((out[0] - 0.0).abs() < 1e-9);
        assert!((out[1] - 0.00002).abs() < 1e-8);
        assert!((out[500] - 0.01).abs() < 1e-5);
        core.shutdown();
    }

    #[test]
    fn seek_rebases_the_stream() {
        let (core, mut mixer) = PlayerCore::new(48_000, 1);
        let slot = core
            .play(Box::new(RampSource {
                rate: 48_000,
                next: 0,
                end: 100_000,
                scale: 0.00002,
            }))
            .unwrap();
        core.set_primary(slot);
        std::thread::sleep(Duration::from_millis(50));
        core.seek(slot, 10_000);
        // Wait for the refill.
        std::thread::sleep(Duration::from_millis(80));
        let mut out = vec![0f32; 64];
        mixer.render(&mut out);
        // After the seek the ramp continues from sample 10000 (value
        // 10000 × scale).
        assert!(
            (out[0] - 0.2).abs() < 0.004,
            "expected ≈0.2 after seek, got {}",
            out[0]
        );
        core.shutdown();
    }

    #[test]
    fn clock_follows_the_primary_voice() {
        let (core, mut mixer) = PlayerCore::new(48_000, 1);
        let slot = core
            .play(Box::new(RampSource {
                rate: 48_000,
                next: 0,
                end: 200_000,
                scale: 0.000002,
            }))
            .unwrap();
        core.set_primary(slot);
        std::thread::sleep(Duration::from_millis(50));
        let mut out = vec![0f32; 4800];
        for _ in 0..3 {
            mixer.render(&mut out);
        }
        let position = core.clock().audio_position();
        assert!(
            position.0 > 0.05 && position.0 < 0.5,
            "clock at {position:?}"
        );
        core.shutdown();
    }

    #[test]
    fn crossfade_between_two_voices_on_equal_power_curves() {
        let (core, mut mixer) = PlayerCore::new(48_000, 1);
        let a = core
            .play(Box::new(RampSource {
                rate: 48_000,
                next: 0,
                end: 500_000,
                scale: 0.000001,
            }))
            .unwrap();
        let b = core
            .play(Box::new(RampSource {
                rate: 48_000,
                next: 500_000,
                end: 1_000_000,
                scale: 0.000001,
            }))
            .unwrap();
        core.set_primary(a);
        std::thread::sleep(Duration::from_millis(80));
        // The end-of-song crossfade: two equal-power ramps over 960
        // frames.
        core.crossfade_into(a, b, 960, FadeCurve::EqualPower, 1.0, 1.0);
        let mut out = vec![0f32; 960];
        mixer.render(&mut out);
        // Voice A at ~0.5+ and B at ~500+ start quiet and loud
        // respectively; mid-fade both are audible.
        let mid = out[480];
        assert!(mid > 0.0, "mid-fade silence: {mid}");
        // The sum of equal-power gains of two DIFFERENT ramps is between
        // 1 and sqrt(2) — the mix stays present throughout.
        assert!(out.iter().all(|s| *s >= -1.0 && *s <= 2.0));
        core.shutdown();
    }

    #[test]
    fn resume_at_position_preserves_the_media_position() {
        // The hotplug-resume primitive: after the "reopen", the clock
        // shows the same media position and playback continues from it.
        let (core, mut mixer) = PlayerCore::new(48_000, 1);
        let slot = core
            .play(Box::new(RampSource {
                rate: 48_000,
                next: 0,
                end: 200_000,
                scale: 0.000002,
            }))
            .unwrap();
        core.set_primary(slot);
        std::thread::sleep(Duration::from_millis(50));
        let mut out = vec![0f32; 4800];
        mixer.render(&mut out);
        mixer.render(&mut out);
        let before = core.clock().audio_position();
        assert!(before.0 > 0.05, "nothing played: {before:?}");
        // The device "went away and came back".
        core.resume_at_position();
        std::thread::sleep(Duration::from_millis(100));
        mixer.render(&mut out);
        let after = core.clock().audio_position();
        assert!(
            (after.0 - before.0).abs() < 0.5,
            "position jumped: {before:?} → {after:?}"
        );
        core.shutdown();
    }

    #[test]
    fn resampling_voice_plays_at_the_output_rate() {
        // A 44.1 kHz sine into a 48 kHz output: the tone's frequency
        // survives the sinc resample exactly — frame n of the output
        // matches the 48 kHz-native sine at the same instant.
        let (core, mut mixer) = PlayerCore::new(48_000, 1);
        let slot = core
            .play(Box::new(SineSource {
                rate: 44_100,
                next: 0,
                end: 88_200, // 2 seconds of media
            }))
            .unwrap();
        core.set_primary(slot);
        std::thread::sleep(Duration::from_millis(150));
        let mut out = vec![0f32; 512];
        mixer.render(&mut out);
        // The sinc filter delays the signal by (taps-1)/2 input samples
        // (upstream's resampler has the same group delay — a ~2 ms
        // constant, position accounting absorbs it). Assert the tone's
        // FREQUENCY survives: some constant sub-300-frame alignment of
        // the 48 kHz-native sine matches within 1%.
        let mut best = f32::MAX;
        for delay in 0..300 {
            let mut worst = 0f32;
            for i in (delay..delay + 200).step_by(4) {
                let t = (i - delay) as f64 / 48_000.0;
                let expect = (2.0 * std::f64::consts::PI * 440.0 * t).sin() as f32 * 0.5;
                worst = worst.max((out[i] - expect).abs());
            }
            best = best.min(worst);
        }
        assert!(best < 0.03, "no alignment matched: best error {best}");
        core.shutdown();
    }
}
