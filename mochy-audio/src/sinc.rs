//! Polyphase Kaiser-windowed sinc resampler — the 120 dB hi-fi resampler.
//!
//! Port of `playback/audio/SincResampler.kt` (upstream pin `bde01cb4`,
//! whole file, 163 lines). Behavior is ported 1:1, including the shared
//! per-rate-pair filter table, the phase-accumulating polyphase loop and
//! the history-compaction rule; the transplanted upstream unit tests live
//! in `tests/sinc_resampler.rs`.
//!
//! Where it sits in the port (blueprint ch. 11, Table 23 "Mixer/voices"):
//! each decode task resamples its song to the output stream's rate with
//! one of these before filling the voice ring — the desktop replacement
//! for PrecisionSink's resample arm and Android's own mixer resampler
//! (`PrecisionSink.kt:65-106` decided when; this file is the "how").
//!
//! Upstream notes carried over verbatim in spirit:
//! - flat to ~20 kHz, 120 dB down past the lower Nyquist, float in and out;
//! - the filter for one pair of rates is shared by every player that needs
//!   it (`tables` ConcurrentHashMap → a process-wide table cache here);
//! - a strange rate pair would stutter the design pass, so tables beyond
//!   [`MAX_COEFFICIENTS`] are refused and the caller falls back (upstream
//!   let Android's mixer take those; the port's caller falls back to a
//!   linear resampler in `crate::precision`).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

/// Kaiser stopband attenuation in dB (`SincResampler.kt:99`).
pub const STOPBAND_DB: f64 = 120.0;
/// Taps for a straight 1:1 band; going down in rate scales it up so the
/// slope stays as steep (`SincResampler.kt:101`).
const BASE_TAPS: f64 = 192.0;
/// Initial per-channel history capacity (`SincResampler.kt:102`).
const INITIAL_CAPACITY: usize = 8192;
/// Designed on the playback thread as a song opens: a strange rate pair
/// would stutter, so beyond this the design is refused
/// (`SincResampler.kt:104`, `MaxCoefficients = 200_000`).
const MAX_COEFFICIENTS: u64 = 200_000;

/// The filter for one pair of rates, shared by every resampler that needs
/// it (`SincResampler.kt:96`, private `Table`).
#[derive(Debug)]
struct Table {
    up: u32,
    down: u32,
    taps: usize,
    passband_hz: f64,
    coefficients: Arc<[f32]>,
}

impl Table {
    /// Designs the polyphase filter for one rate pair
    /// (`SincResampler.kt:115-144`).
    fn design(in_rate: u32, out_rate: u32) -> Option<Table> {
        let gcd = gcd(in_rate, out_rate);
        let up = out_rate / gcd;
        let down = in_rate / gcd;
        let taps = (BASE_TAPS * (f64::from(in_rate) / f64::from(out_rate)).max(1.0)).ceil() as usize;
        let length = u64::from(up) * taps as u64;
        if length > MAX_COEFFICIENTS {
            return None;
        }

        // Kaiser's estimate: how wide the transition is for this length and
        // attenuation, in Hz at the input rate (`SincResampler.kt:124`).
        let transition_hz = (STOPBAND_DB - 7.95) / (2.285 * 2.0 * std::f64::consts::PI * taps as f64)
            * f64::from(in_rate);
        let stop_hz = f64::from(in_rate.min(out_rate)) / 2.0;
        let passband_hz = stop_hz - transition_hz;
        let cutoff = (stop_hz - transition_hz / 2.0) / (f64::from(up) * f64::from(in_rate));
        let beta = 0.1102 * (STOPBAND_DB - 8.7);
        let i0_beta = bessel_i0(beta);
        let centre = (length - 1) as f64 / 2.0;

        let length = length as usize;
        let mut coefficients = vec![0f32; length];
        for k in 0..length {
            let t = k as f64 - centre;
            let sinc = if t == 0.0 {
                2.0 * cutoff
            } else {
                (2.0 * std::f64::consts::PI * cutoff * t).sin() / (std::f64::consts::PI * t)
            };
            let ratio = t / centre;
            let window = bessel_i0(beta * (1.0 - ratio * ratio).max(0.0).sqrt()) / i0_beta;
            // Stored phase by phase, so one output reads one run of memory;
            // times `up` for the zeros stuffed between inputs
            // (`SincResampler.kt:138-141`).
            let p = k % up as usize;
            let j = k / up as usize;
            coefficients[p * taps + j] = (sinc * window * f64::from(up)) as f32;
        }
        Some(Table {
            up,
            down,
            taps,
            passband_hz,
            coefficients: coefficients.into(),
        })
    }
}

/// Polyphase kaiser-windowed sinc resampler: flat to ~20 kHz, 120 dB down
/// past the lower Nyquist, float in and out (`SincResampler.kt:11-17`).
#[derive(Debug)]
pub struct SincResampler {
    /// Input rate in Hz.
    pub in_rate: u32,
    /// Output rate in Hz.
    pub out_rate: u32,
    channels: usize,
    table: Arc<Table>,
    /// Each channel's recent input, oldest first; the filter reads back
    /// `taps - 1` samples from the newest (`SincResampler.kt:26`).
    history: Vec<Vec<f32>>,
    stored: usize,
    /// Absolute index of `history[0]`, and of the input sample the next
    /// output centres on (`SincResampler.kt:28-29`).
    base: i64,
    next_input: i64,
    phase: u32,
}

impl SincResampler {
    /// Creates a resampler for one channel layout, or `None` when the rates
    /// match (nothing to do), are invalid, or the filter would be too large
    /// (`SincResampler.kt:108-113`).
    pub fn create(in_rate: u32, out_rate: u32, channels: usize) -> Option<SincResampler> {
        if in_rate == 0 || out_rate == 0 || in_rate == out_rate || channels == 0 {
            return None;
        }
        let key = (u64::from(in_rate) << 32) | u64::from(out_rate);
        let tables = table_cache();
        let mut guard = tables.lock().expect("sinc table cache poisoned");
        let table = match guard.get(&key) {
            Some(table) => table.clone(),
            None => {
                let designed = Arc::new(Table::design(in_rate, out_rate)?);
                guard.insert(key, designed.clone());
                designed
            }
        };
        Some(SincResampler {
            in_rate,
            out_rate,
            channels,
            table,
            history: vec![vec![0f32; INITIAL_CAPACITY]; channels],
            stored: 0,
            base: 0,
            next_input: 0,
            phase: 0,
        })
        .map(|mut r| {
            r.reset();
            r
        })
    }

    /// The filter's taps per output sample, which is what decides its
    /// steepness (`SincResampler.kt:19`).
    pub fn taps(&self) -> usize {
        self.table.taps
    }

    /// Where the passband ends, in Hz (`SincResampler.kt:20`).
    pub fn passband_hz(&self) -> f64 {
        self.table.passband_hz
    }

    /// The lower rate's Nyquist, where the stopband starts
    /// (`SincResampler.kt:21`).
    pub fn stopband_hz(&self) -> f64 {
        f64::from(self.in_rate.min(self.out_rate)) / 2.0
    }

    /// Forgets everything heard, for a seek or a flush: the next sample
    /// starts on a quiet history (`SincResampler.kt:37-44`).
    pub fn reset(&mut self) {
        self.stored = self.table.taps - 1;
        for channel in &mut self.history {
            channel[..self.stored].fill(0.0);
        }
        self.base = -(self.stored as i64);
        self.next_input = 0;
        self.phase = 0;
    }

    /// How many output frames at most `frames` more input can give, for
    /// sizing buffers (`SincResampler.kt:47`).
    pub fn max_output_frames(&self, frames: usize) -> usize {
        ((frames as u64 + 2) * u64::from(self.table.up) / u64::from(self.table.down) + 2) as usize
    }

    /// Resamples `frames` of `input` (one slice per channel) into `output`
    /// and says how many frames it wrote (`SincResampler.kt:50-84`).
    ///
    /// `input`/`output` channel slices are indexed like the upstream
    /// `Array<FloatArray>` planes; extra channels in either are ignored,
    /// missing ones panic exactly like the Kotlin indexing would.
    pub fn process(&mut self, input: &[&[f32]], frames: usize, output: &mut [&mut [f32]]) -> usize {
        self.append(input, frames);
        let taps = self.table.taps;
        let up = self.table.up;
        let down = self.table.down;
        let coefficients = &self.table.coefficients;
        let last = self.base + self.stored as i64 - 1;
        let mut written = 0usize;
        while self.next_input <= last {
            let newest = (self.next_input - self.base) as usize;
            let offset = self.phase as usize * taps;
            for c in 0..self.channels {
                let x = &self.history[c];
                let mut acc = 0f32;
                let mut j = 0usize;
                while j < taps {
                    acc += coefficients[offset + j] * x[newest - j];
                    j += 1;
                }
                output[c][written] = acc;
            }
            written += 1;
            if written == output[0].len() {
                // Port guard: upstream callers always size the target with
                // `maxOutputFrames`; refuse to overrun a short buffer rather
                // than panic (behavior difference recorded in HANDOFF).
                break;
            }
            self.phase += down;
            self.next_input += i64::from(self.phase / up);
            self.phase %= up;
        }
        // Only the last `taps - 1` samples before the next output are still
        // needed (`SincResampler.kt:76-82`).
        let keep_from = (self.next_input - (taps as i64 - 1) - self.base) as usize;
        let keep_from = keep_from.min(self.stored);
        if keep_from > 0 {
            for channel in &mut self.history {
                channel.copy_within(keep_from..keep_from + (self.stored - keep_from), 0);
            }
            self.stored -= keep_from;
            self.base += keep_from as i64;
        }
        written
    }

    /// Appends input to each channel's history, growing as needed
    /// (`SincResampler.kt:86-93`).
    fn append(&mut self, input: &[&[f32]], frames: usize) {
        if self.stored + frames > self.history[0].len() {
            let size = (self.stored + frames).max(self.history[0].len() * 2);
            for channel in &mut self.history {
                channel.resize(size, 0.0);
            }
        }
        for c in 0..self.channels {
            self.history[c][self.stored..self.stored + frames].copy_from_slice(&input[c][..frames]);
        }
        self.stored += frames;
    }
}

/// Process-wide table cache — the port of the upstream
/// `ConcurrentHashMap<Long, Table>` (`SincResampler.kt:106`).
fn table_cache() -> &'static Mutex<HashMap<u64, Arc<Table>>> {
    static CACHE: OnceLock<Mutex<HashMap<u64, Arc<Table>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// `besselI0` series (`SincResampler.kt:146-158`).
fn bessel_i0(x: f64) -> f64 {
    let mut sum = 1.0;
    let mut term = 1.0;
    let half = x / 2.0;
    let mut k = 1.0;
    while k < 64.0 {
        term *= (half / k) * (half / k);
        sum += term;
        if term < sum * 1e-12 {
            break;
        }
        k += 1.0;
    }
    sum
}

/// `gcd` (`SincResampler.kt:160`).
fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 {
        a
    } else {
        gcd(b, a % b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_same_or_invalid_rates() {
        // SincResamplerTest.create contract: same rate → null (nothing to do).
        assert!(SincResampler::create(44_100, 44_100, 2).is_none());
        assert!(SincResampler::create(0, 48_000, 2).is_none());
        assert!(SincResampler::create(44_100, 0, 2).is_none());
        assert!(SincResampler::create(44_100, 48_000, 0).is_none());
        assert!(SincResampler::create(44_100, 48_000, 2).is_some());
    }

    #[test]
    fn table_is_shared_between_instances() {
        // The upstream ConcurrentHashMap: one Table per rate pair.
        let a = SincResampler::create(44_100, 48_000, 1).unwrap();
        let b = SincResampler::create(44_100, 48_000, 1).unwrap();
        assert_eq!(a.taps(), b.taps());
        assert_eq!(a.passband_hz(), b.passband_hz());
        // 44.1→48 kHz: gcd 300 → up 160, down 147; taps 192 (no downscale).
        assert_eq!(a.taps(), 192);
    }

    #[test]
    fn refuses_absurd_rate_pairs() {
        // MaxCoefficients guard (200_000): coprime neighbours stuff the
        // whole up-factor into the table length.
        assert!(SincResampler::create(44_100, 44_101, 2).is_none());
        // A long-but-sane ladder pair fits: 44.1 -> 192 kHz (up 640 × 192 taps).
        assert!(SincResampler::create(44_100, 192_000, 2).is_some());
        // Downscales are where the tap count grows (taps 192 × in/out):
        // 192 -> 44.1 kHz: gcd 600 → up 147, taps ceil(192×4.35)=836 → 122k, fits.
        assert!(SincResampler::create(192_000, 44_100, 2).is_some());
    }

    #[test]
    fn reset_restores_quiet_history() {
        let mut r = SincResampler::create(44_100, 48_000, 1).unwrap();
        let input = [vec![0.5f32; 1000]];
        let planes: &[&[f32]] = &[&input[0]];
        let mut out = [vec![0f32; 2000]];
        let mut out_planes: [&mut [f32]; 1] = [&mut out[0]];
        let written = r.process(planes, 1000, &mut out_planes);
        assert!(written > 500);
        r.reset();
        // After reset the next output must be zero (quiet history).
        let mut out2 = [vec![0f32; 64]];
        let mut out2_planes: [&mut [f32]; 1] = [&mut out2[0]];
        let w2 = r.process(planes, 0, &mut out2_planes);
        assert_eq!(w2, 0);
    }

    #[test]
    fn max_output_frames_sizes_generously() {
        let r = SincResampler::create(44_100, 48_000, 1).unwrap();
        // (frames + 2) * up / down + 2 — 44.1→48 kHz: gcd 300, up 160, down 147.
        let expect = ((1000u64 + 2) * 160 / 147 + 2) as usize;
        assert_eq!(r.max_output_frames(1000), expect);
    }

    #[test]
    fn stopband_and_passband_math() {
        let r = SincResampler::create(44_100, 48_000, 1).unwrap();
        assert_eq!(r.stopband_hz(), 22_050.0);
        // passband = stop - transition; transition > 0 → passband < stop.
        assert!(r.passband_hz() > 18_000.0 && r.passband_hz() < 22_050.0);
        let r2 = SincResampler::create(96_000, 48_000, 1).unwrap();
        assert_eq!(r2.stopband_hz(), 24_000.0);
    }
}
