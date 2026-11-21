//! Voice gain automation — the crossfade curves, transplanted into the
//! mixer.
//!
//! Port of the ramp math in `MusicService.rampCrossfade`
//! (upstream pin `bde01cb4`, lines 5010-5091): the Android app stepped
//! two ExoPlayers' volumes every frame by "the time actually played"
//! (heard time — "the wall clock started as soon as isPlaying flipped,
//! while the audio track was still filling"), on two curve families:
//!
//! - **equal-power** for the end-of-song crossfade: `in = sin(θ)`,
//!   `out = cos(θ)` — the classic constant-power pair for uncorrelated
//!   sources;
//! - **s-curve** for blended skips: `in = sin²(θ)`, `out = cos²(θ)`
//!   ("without one a skip eases on equal-gain s-curves, the
//!   end-of-song crossfade on equal-power ramps", MusicService.kt:5073).
//!
//! The mixer's version drives the same curves by the voice's **played
//! frame count** — the desktop equivalent of "timed by the new song's
//! own position, which is what's really coming out of the speaker" —
//! computed per-sample inside the render loop instead of stepped by a
//! coroutine. `progress = played_frames / duration_frames`,
//! `θ = progress · π/2`.
//!
//! The fade-out tail for the crossmix blend's layered effect
//! (`tailFade`, the held-then-quick release) belongs to the Phase 20
//! crossmix renderer and is deliberately not ported here.

/// Which curve family a fade runs on (MusicService.kt:5073-5081).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FadeCurve {
    /// Equal-power: `sin(θ)` fading in over `cos(θ)` fading out — the
    /// end-of-song crossfade.
    EqualPower,
    /// Equal-gain s-curves: `sin²(θ)` / `cos²(θ)` — the blended skip.
    SCurve,
}

impl FadeCurve {
    /// The fading-in gain at `progress` ∈ [0, 1].
    pub fn gain_in(&self, progress: f32) -> f32 {
        let progress = progress.clamp(0.0, 1.0);
        match self {
            FadeCurve::EqualPower => (progress * std::f32::consts::FRAC_PI_2).sin(),
            FadeCurve::SCurve => {
                let s = (progress * std::f32::consts::FRAC_PI_2).sin();
                s * s
            }
        }
    }

    /// The fading-out gain at `progress` ∈ [0, 1] (the same progress
    /// clock as the fading-in voice — both sides move on one clock for
    /// the whole length, MusicService.kt:5063).
    pub fn gain_out(&self, progress: f32) -> f32 {
        let progress = progress.clamp(0.0, 1.0);
        match self {
            FadeCurve::EqualPower => (progress * std::f32::consts::FRAC_PI_2).cos(),
            FadeCurve::SCurve => {
                let c = (progress * std::f32::consts::FRAC_PI_2).cos();
                c * c
            }
        }
    }
}

/// A gain ramp on one voice: from `from` to `to` over
/// `duration_frames` of **played** audio, on one curve family.
///
/// This is the engine the QueueMachine (Phase 10) drives crossfades and
/// the ErrorPolicy drives ducking with; PlayerCore exposes
/// `start_fade`/`fade_to` over it.
#[derive(Debug, Clone)]
pub struct GainAutomation {
    curve: FadeCurve,
    from: f32,
    to: f32,
    duration_frames: u64,
    elapsed_frames: u64,
}

impl GainAutomation {
    /// A ramp from `from` to `to` over `duration_frames` played frames.
    pub fn new(curve: FadeCurve, from: f32, to: f32, duration_frames: u64) -> GainAutomation {
        GainAutomation {
            curve,
            from,
            to,
            duration_frames: duration_frames.max(1),
            elapsed_frames: 0,
        }
    }

    /// The curve family in use.
    pub fn curve(&self) -> FadeCurve {
        self.curve
    }

    /// Whether the ramp has run its course.
    pub fn is_finished(&self) -> bool {
        self.elapsed_frames >= self.duration_frames
    }

    /// The current static gain (before this block advances the ramp).
    pub fn current_gain(&self) -> f32 {
        self.gain_at(self.elapsed_frames)
    }

    /// The gain `frames` played frames from the ramp's start.
    pub fn gain_at(&self, frames: u64) -> f32 {
        if frames >= self.duration_frames {
            return self.to;
        }
        let progress = frames as f32 / self.duration_frames as f32;
        // The curve shapes the interpolation between the two endpoints,
        // exactly like the upstream's `targetVolume() * inGain` /
        // `startVolume * outGain` composition.
        let shaped = self.curve.gain_in(progress);
        self.from + (self.to - self.from) * shaped
    }

    /// Advances the ramp over `frames` played frames, writing the
    /// per-frame gains into `out` (one per frame, interleaving applied by
    /// the caller).
    pub fn advance(&mut self, frames: usize, out: &mut [f32]) {
        let frames = frames.min(out.len());
        for (i, gain) in out[..frames].iter_mut().enumerate() {
            *gain = self.gain_at(self.elapsed_frames + i as u64);
        }
        self.elapsed_frames += frames as u64;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_power_pair_sums_to_constant_power() {
        // For uncorrelated sources, in² + out² = 1 at every instant.
        let curve = FadeCurve::EqualPower;
        for i in 0..=100 {
            let p = i as f32 / 100.0;
            let power = curve.gain_in(p).powi(2) + curve.gain_out(p).powi(2);
            assert!((power - 1.0).abs() < 1e-6, "power {power} at {p}");
        }
    }

    #[test]
    fn s_curve_pair_sums_to_one_gain() {
        // sin² + cos² = 1 exactly: the equal-gain property.
        let curve = FadeCurve::SCurve;
        for i in 0..=100 {
            let p = i as f32 / 100.0;
            let sum = curve.gain_in(p) + curve.gain_out(p);
            assert!((sum - 1.0).abs() < 1e-6, "sum {sum} at {p}");
        }
        // And the endpoints are the corners.
        assert_eq!(curve.gain_in(0.0), 0.0);
        assert_eq!(curve.gain_out(0.0), 1.0);
        assert!((curve.gain_in(1.0) - 1.0).abs() < 1e-7);
        assert!(curve.gain_out(1.0).abs() < 1e-7);
    }

    #[test]
    fn ramp_runs_endpoint_to_endpoint_over_played_frames() {
        let mut ramp = GainAutomation::new(FadeCurve::EqualPower, 0.0, 1.0, 1000);
        assert_eq!(ramp.current_gain(), 0.0);
        let mut gains = vec![0f32; 500];
        ramp.advance(500, &mut gains);
        // Halfway: sin(π/4) ≈ 0.7071.
        assert!((ramp.current_gain() - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
        ramp.advance(500, &mut gains);
        assert!(ramp.is_finished());
        assert_eq!(ramp.current_gain(), 1.0);
        // Past the end it holds.
        ramp.advance(100, &mut gains);
        assert_eq!(ramp.current_gain(), 1.0);
    }

    #[test]
    fn ramp_composes_endpoint_volumes() {
        // "started from the old one's [volume], a fade begun while muted
        // or ducked ended at 0 or a fifth" — MusicService.kt:5085.
        let mut ramp = GainAutomation::new(FadeCurve::SCurve, 0.2, 0.8, 100);
        assert_eq!(ramp.current_gain(), 0.2);
        let mut gains = vec![0f32; 100];
        ramp.advance(100, &mut gains);
        assert!((ramp.current_gain() - 0.8).abs() < 1e-6);
    }

    #[test]
    fn per_frame_gains_follow_the_curve() {
        let mut ramp = GainAutomation::new(FadeCurve::EqualPower, 0.0, 1.0, 4);
        let mut gains = vec![0f32; 4];
        ramp.advance(4, &mut gains);
        assert!((gains[0] - 0.0).abs() < 1e-7);
        // Frame 1 of 4: sin(π/8); frame 2: sin(π/4); frame 3: sin(3π/8).
        assert!((gains[1] - (std::f32::consts::FRAC_PI_8).sin()).abs() < 1e-6);
        assert!((gains[2] - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
        assert!((gains[3] - (3.0 * std::f32::consts::FRAC_PI_8).sin()).abs() < 1e-6);
    }
}
