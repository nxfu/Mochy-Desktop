//! PlaybackClock — the 60-fps discipline's one clock.
//!
//! Port of the position plumbing that on Android ran through
//! `CrossmixClockSink` (every buffer's timestamp + the stream offset →
//! "where in its song each buffer sits") and `Player.getCurrentPosition`
//! (start time + frames actually played, `PassSink.getCurrentPositionUs`:
//! `startUs + framesToUs(min(played, written))`). The blueprint's
//! desktop shape (ch. 10.2 / ch. 11 Table 23 "Clock"): the audio
//! callback's sample counter feeds a **shared f64 position entity** that
//! draw code reads **inside paint callbacks — never in update logic,
//! never triggering re-layout**; lyrics, seek-bar fill, the mini ticker
//! and visualizers all consume this one clock.
//!
//! The smoothing model is the snap/pull interpolator the blueprint
//! names: between callbacks the reader extrapolates with the wall clock;
//! a discontinuity beyond [`SNAP_THRESHOLD`] jumps (a seek); smaller
//! drift is pulled in at a bounded rate so the bar never stutters
//! backwards. The paint side owns the smoothed state (one per consumer);
//! the audio side owns two atomics and nothing else — **no locks, no
//! allocation** in the callback.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Position discontinuities larger than this snap (seek, track change).
/// Below it the reader pulls smoothly (resync drift).
pub const SNAP_THRESHOLD: Duration = Duration::from_millis(250);
/// The pull rate: how fast the smoothed position chases the audio
/// position, as a fraction per second. 4%/s is imperceptible as motion
/// but recovers any drift within moments.
pub const PULL_RATE_PER_SECOND: f64 = 0.04;

/// The audio-thread half: written by the render callback, read by
/// paint code. Cheap enough to update once per block.
#[derive(Debug)]
pub struct ClockWriter {
    inner: Arc<ClockInner>,
}

/// The paint-side handle: clone one per consumer (lyrics, seek bar,
/// ticker); each holds its own smoothing state.
#[derive(Debug, Clone)]
pub struct PlaybackClock {
    inner: Arc<ClockInner>,
}

#[derive(Debug)]
struct ClockInner {
    /// The active voice's media position in **nanoseconds at the media
    /// rate** — f64 seconds packed into bits; written by the callback
    /// after each block, monotonic per voice generation.
    position_ns: AtomicU64,
    /// The wall-clock instant (ns since the process epoch) the position
    /// was current at — the extrapolation anchor.
    updated_at_ns: AtomicU64,
    /// The media rate of the position (for rate-aware consumers).
    media_rate: AtomicU32,
    /// Whether the transport is playing (a paused clock freezes).
    playing: AtomicU64,
}

/// The process-wide monotonic epoch the clock's timestamps are measured
/// against (a `OnceLock<Instant>` — read-only after init, so the
/// callback never allocates).
fn epoch() -> Instant {
    static EPOCH: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    *EPOCH.get_or_init(Instant::now)
}

fn now_ns() -> u64 {
    epoch().elapsed().as_nanos() as u64
}

/// Creates the clock pair.
pub fn playback_clock() -> (ClockWriter, PlaybackClock) {
    let inner = Arc::new(ClockInner {
        position_ns: AtomicU64::new(0),
        updated_at_ns: AtomicU64::new(0),
        media_rate: AtomicU32::new(44_100),
        playing: AtomicU64::new(0),
    });
    (
        ClockWriter {
            inner: inner.clone(),
        },
        PlaybackClock { inner },
    )
}

impl ClockWriter {
    /// Publishes the media position for the block the callback just
    /// rendered (call once per block, after mixing). `position` is
    /// seconds into the media; `media_rate` is the song's own rate.
    pub fn publish(&self, position: Seconds, media_rate: u32, playing: bool) {
        self.inner
            .position_ns
            .store(position.0.to_bits(), Ordering::Release);
        self.inner
            .updated_at_ns
            .store(now_ns(), Ordering::Release);
        self.inner.media_rate.store(media_rate, Ordering::Relaxed);
        self.inner
            .playing
            .store(u64::from(playing), Ordering::Relaxed);
    }
}

/// A media position in seconds (the shared f64 entity).
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Seconds(pub f64);

impl Default for Seconds {
    fn default() -> Self {
        Seconds(0.0)
    }
}

impl PlaybackClock {
    /// The raw audio position right now (no smoothing) — seconds into
    /// the media. Exposed for tests and the parity harness; paint code
    /// wants [`smoothed_position`].
    pub fn audio_position(&self) -> Seconds {
        Seconds(f64::from_bits(self.inner.position_ns.load(Ordering::Acquire)))
    }

    /// The media rate the position ticks at.
    pub fn media_rate(&self) -> u32 {
        self.inner.media_rate.load(Ordering::Relaxed)
    }

    /// Whether the transport is playing.
    pub fn is_playing(&self) -> bool {
        self.inner.playing.load(Ordering::Relaxed) != 0
    }

    /// The extrapolated position at `now` — what the audio callback has
    /// consumed plus the wall time since it last reported, frozen when
    /// paused. This is the paint-side reader of the shared entity.
    pub fn extrapolated_position(&self, now: Instant) -> Seconds {
        let _ = now; // the caller's frame time; the anchor is the callback's
        let position = self.audio_position();
        let updated = self.inner.updated_at_ns.load(Ordering::Acquire);
        let at = now_ns().max(updated);
        if !self.is_playing() {
            return position;
        }
        let elapsed = Duration::from_nanos(at.saturating_sub(updated));
        // Between callbacks the song advances by wall time.
        Seconds(position.0 + elapsed.as_secs_f64())
    }

    /// The snap/pull-smoothed position for one paint consumer.
    ///
    /// Read this **inside Element paint** (the 60-fps discipline): it
    /// never mutates shared state, so any number of consumers can call
    /// it per frame. The smoothing state lives in the returned value's
    /// caller — use [`SmoothedPosition`] to hold it across frames.
    pub fn smoothed_position(&self, smoother: &mut SmoothedPosition, now: Instant) -> Seconds {
        let target = self.extrapolated_position(now);
        let elapsed = smoother
            .last_update
            .map(|then| now.duration_since(then))
            .unwrap_or(Duration::ZERO);
        if smoother.last_update.is_none() {
            // First read: adopt.
            smoother.current = target;
            smoother.last_update = Some(now);
            return target;
        }
        smoother.last_update = Some(now);
        let delta = target.0 - smoother.current.0;
        if delta.abs() > SNAP_THRESHOLD.as_secs_f64() {
            // A seek or track change: snap.
            smoother.current = target;
            return target;
        }
        // Pull: bounded catch-up, never moving backwards perceptibly.
        let pull = PULL_RATE_PER_SECOND * elapsed.as_secs_f64() * delta.abs();
        let step = delta.abs().min(pull.max(delta.abs() * 0.05));
        smoother.current = Seconds(smoother.current.0 + delta.signum() * step);
        // Never overshoot past the target.
        if (target.0 - smoother.current.0).signum() != delta.signum() {
            smoother.current = target;
        }
        smoother.current
    }
}

/// One paint consumer's smoothing state (seek-bar fill, lyrics, ticker
/// each hold one).
#[derive(Debug, Clone, Copy, Default)]
pub struct SmoothedPosition {
    current: Seconds,
    last_update: Option<Instant>,
}

impl SmoothedPosition {
    /// A fresh smoother with no adopted position yet.
    pub fn new() -> SmoothedPosition {
        SmoothedPosition::default()
    }

    /// The last smoothed position (0 before the first read).
    pub fn current(&self) -> Seconds {
        self.current
    }

    /// Forces the next read to snap (call from update logic on a seek,
    /// NOT from paint).
    pub fn snap_next(&mut self) {
        self.last_update = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publish_and_read_raw_position() {
        let (writer, clock) = playback_clock();
        assert_eq!(clock.audio_position(), Seconds(0.0));
        writer.publish(Seconds(12.5), 44_100, true);
        assert_eq!(clock.audio_position(), Seconds(12.5));
        assert_eq!(clock.media_rate(), 44_100);
        assert!(clock.is_playing());
        writer.publish(Seconds(12.5), 44_100, false);
        assert!(!clock.is_playing());
    }

    #[test]
    fn extrapolation_freezes_when_paused() {
        let (writer, clock) = playback_clock();
        writer.publish(Seconds(5.0), 48_000, false);
        let now = Instant::now();
        let at = clock.extrapolated_position(now);
        assert_eq!(at, Seconds(5.0));
    }

    #[test]
    fn extrapolation_advances_by_wall_time_when_playing() {
        let (writer, clock) = playback_clock();
        writer.publish(Seconds(10.0), 48_000, true);
        let now = Instant::now();
        // Immediately after: essentially the same position.
        let at = clock.extrapolated_position(now);
        assert!((at.0 - 10.0).abs() < 0.05, "{at:?}");
        // After a real 30 ms sleep: advanced by roughly that.
        std::thread::sleep(Duration::from_millis(30));
        let later = clock.extrapolated_position(Instant::now());
        assert!(
            later.0 > at.0 && later.0 - at.0 < 0.2,
            "advanced by {}",
            later.0 - at.0
        );
    }

    #[test]
    fn smoother_snaps_on_seeks() {
        let (writer, clock) = playback_clock();
        writer.publish(Seconds(1.0), 48_000, true);
        let mut smoother = SmoothedPosition::new();
        let now = Instant::now();
        let first = clock.smoothed_position(&mut smoother, now);
        assert!((first.0 - 1.0).abs() < 0.05, "first read: {first:?}");
        // A seek far forward: snap, not pull.
        writer.publish(Seconds(60.0), 48_000, true);
        let snapped = clock.smoothed_position(&mut smoother, Instant::now());
        assert!(
            (snapped.0 - 60.0).abs() < 0.05,
            "snapped to {snapped:?}"
        );
    }

    #[test]
    fn smoother_pulls_small_drift_gradually() {
        let (writer, clock) = playback_clock();
        writer.publish(Seconds(10.0), 48_000, true);
        let mut smoother = SmoothedPosition::new();
        let _ = clock.smoothed_position(&mut smoother, Instant::now());
        // A small discontinuity (50 ms — say a decode hiccup resync):
        // the smoother moves part-way toward it, never overshooting.
        writer.publish(Seconds(10.05), 48_000, true);
        let smoothed = clock.smoothed_position(&mut smoother, Instant::now());
        assert!(smoothed.0 >= 10.0 && smoothed.0 <= 10.05, "{smoothed:?}");
        // Repeated reads converge to the target.
        for _ in 0..200 {
            let _ = clock.smoothed_position(&mut smoother, Instant::now());
        }
        let converged = smoother.current();
        assert!(
            (converged.0 - 10.05).abs() < 0.01,
            "converged to {converged:?}"
        );
    }
}
