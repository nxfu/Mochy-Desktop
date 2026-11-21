//! Transplanted upstream unit tests for [`mochy_audio::sinc::SincResampler`].
//!
//! Ported 1:1 from `app/src/test/kotlin/com/mochyy/mochymusic/playback/audio/SincResamplerTest.kt`
//! (upstream pin `bde01cb4`, 72 lines) — the four tests, their fixtures,
//! tolerances and comment lines, per the port's transplant discipline
//! (PORT_SPEC §12: "transplanted Android unit tests (DSP math, parsers,
//! matchers, graders — ported 1:1 with original fixtures)").
//!
//! Kotlin → Rust notes (behavior-preserving only):
//! - `run(...)` mirrors the chunked `run` helper, including the
//!   `chunk = input.size` default;
//! - `rms` mirrors the sum-of-squares RMS over an exclusive range;
//! - assertions use the same tolerances (`2.0` samples, `0.001` level,
//!   `0.01` dB, `-110` dB alias floor, `1e-6` sample delta).

use mochy_audio::sinc::SincResampler;

fn tone(rate: u32, hz: f64, seconds: f64, amplitude: f64) -> Vec<f32> {
    let count = (rate as f64 * seconds) as usize;
    (0..count)
        .map(|i| {
            (amplitude * (2.0 * std::f64::consts::PI * hz * i as f64 / f64::from(rate)).sin())
                as f32
        })
        .collect()
}

fn run(resampler: &mut SincResampler, input: &[f32], chunk: usize) -> Vec<f32> {
    let chunk = if chunk == 0 { input.len() } else { chunk };
    let mut out = Vec::new();
    let mut at = 0usize;
    while at < input.len() {
        let n = chunk.min(input.len() - at);
        let plane: &[&[f32]] = &[&input[at..at + n]];
        let mut target = vec![0f32; resampler.max_output_frames(n)];
        let mut target_planes: [&mut [f32]; 1] = [&mut target];
        let written = resampler.process(plane, n, &mut target_planes);
        out.extend_from_slice(&target[..written]);
        at += n;
    }
    out
}

fn rms(samples: &[f32], from: usize, to: usize) -> f64 {
    (samples[from..to]
        .iter()
        .map(|s| f64::from(*s) * f64::from(*s))
        .sum::<f64>()
        / (to - from) as f64)
        .sqrt()
}

// a 1 khz tone comes out of 44.1 -> 48 at the same level, at the right length
#[test]
fn keeps_audible_tones_whole() {
    let mut resampler = SincResampler::create(44_100, 48_000, 1).unwrap();
    let input = tone(44_100, 1_000.0, 1.0, 0.5);
    let out = run(&mut resampler, &input, 0);
    assert!((out.len() as f64 - 48_000.0).abs() <= 2.0);
    let level = rms(&out, 2_000, out.len() - 2_000) / rms(&input, 2_000, input.len() - 2_000);
    assert!((level - 1.0).abs() <= 0.001, "level {level}");
}

// the top of what anyone hears, 20 khz, comes through at full level
#[test]
fn flat_to_20k() {
    let input = tone(44_100, 20_000.0, 1.0, 0.5);
    let mut resampler = SincResampler::create(44_100, 48_000, 1).unwrap();
    let out = run(&mut resampler, &input, 0);
    let db = 20.0 * (rms(&out, 4_000, out.len() - 4_000) / rms(&input, 4_000, input.len() - 4_000))
        .log10();
    assert!(db.abs() < 0.01, "20 khz is {db} dB");
}

// a 30 khz tone in a 96 khz file can't exist at 48 khz: it must vanish, not fold down to 18 khz
#[test]
fn rejects_what_would_alias() {
    let mut resampler = SincResampler::create(96_000, 48_000, 1).unwrap();
    let out = run(&mut resampler, &tone(96_000, 30_000.0, 0.5, 0.5), 0);
    let db = 20.0 * (rms(&out, 2_000, out.len() - 2_000) / (0.5 / 2f64.sqrt())).log10();
    assert!(db < -110.0, "aliasing only {db:.1} dB down");
}

// the player hands buffers of any size: cutting the song up must not change a single sample
#[test]
fn chunks_match_one_block() {
    let input = tone(44_100, 3_000.0, 0.3, 0.5);
    let mut whole_r = SincResampler::create(44_100, 48_000, 1).unwrap();
    let whole = run(&mut whole_r, &input, 0);
    let mut pieces_r = SincResampler::create(44_100, 48_000, 1).unwrap();
    let pieces = run(&mut pieces_r, &input, 333);
    assert_eq!(whole.len(), pieces.len());
    for (i, (w, p)) in whole.iter().zip(&pieces).enumerate() {
        assert!(
            (w - p).abs() < 1e-6f32,
            "sample {i} differs: {w} vs {p}"
        );
    }
}
