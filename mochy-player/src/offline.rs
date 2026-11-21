//! The offline render harness — the test-side counterpart of the audio
//! graph (blueprint ch. 11 "Offline render", Phase 20's parity rig).
//!
//! Renders any [`RenderSource`] (the [`Mixer`](crate::Mixer) through
//! the output link today; the Phase 19-20 DSP graph through the same
//! seam later) straight into PCM buffers, with no output device: the
//! same `render` pull the cpal callback, the WASAPI event loop, the
//! ALSA poll loop and the USB worker all make, on the caller's thread,
//! in fixed blocks. What the parity captures of Phase 20 need then is
//! a file: [`wav`] packs the buffer into a plain PCM WAV (16/24/32-bit
//! or float) over the same [`convert`](mochy_audio::pcm::convert) the
//! output edge uses, so a desktop render and an upstream Android
//! capture of the same graph settings are byte-comparable.
//!
//! The harness is a test utility by design — nothing in the app's
//! runtime path depends on it — but it ships compiled (not under
//! `cfg(test)`) so integration tests and Phase 20's capture scripts
//! can drive it from outside the crate.

use mochy_audio::output::RenderSource;
use mochy_audio::pcm;
use mochy_audio::precision::PcmEncoding;

/// Renders `frames` interleaved frames from `source` in fixed blocks.
///
/// `block_frames` is the pull size — the callback granularity the graph
/// is exercised at (256 frames is a typical desktop callback; the
/// upstream `onData` pass rendered 4,096-frame chunks).
pub fn render_frames(
    source: &mut dyn RenderSource,
    channels: usize,
    frames: usize,
    block_frames: usize,
) -> Vec<f32> {
    let block_frames = block_frames.max(1);
    let mut out = Vec::with_capacity(frames * channels);
    let mut block = vec![0f32; block_frames * channels];
    let mut left = frames;
    while left > 0 {
        let take = left.min(block_frames);
        let slice = &mut block[..take * channels];
        source.render(slice, channels);
        out.extend_from_slice(slice);
        left -= take;
    }
    out
}

/// Renders a whole number of seconds (rounded down to whole frames).
pub fn render_seconds(
    source: &mut dyn RenderSource,
    rate: u32,
    channels: usize,
    seconds: f64,
    block_frames: usize,
) -> Vec<f32> {
    let frames = (f64::from(rate) * seconds) as usize;
    render_frames(source, channels, frames, block_frames)
}

/// The WAV flavors a parity capture comes in — the depths the output
/// edge itself produces (`PrecisionSink`'s integer depths, plus float
/// for the passthrough path).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WavDepth {
    Pcm16,
    Pcm24,
    Pcm32,
    Float,
}

impl WavDepth {
    fn bits(self) -> u32 {
        match self {
            WavDepth::Pcm16 => 16,
            WavDepth::Pcm24 => 24,
            WavDepth::Pcm32 => 32,
            WavDepth::Float => 32,
        }
    }

    fn format_tag(self) -> u16 {
        match self {
            WavDepth::Float => 3, // IEEE float
            _ => 1,               // PCM
        }
    }
}

/// Packs interleaved f32 samples into a PCM WAV file (44-byte RIFF
/// header + data), through the same sample conversion the output edge
/// uses — so a capture is exactly what the stream would have carried.
///
/// Returns the file bytes and whether every sample packed losslessly
/// (the `convert` outcome; a float capture of integer-exact material
/// stays exact, a 16-bit capture of full-range float does not).
pub fn wav(samples: &[f32], rate: u32, channels: usize, depth: WavDepth) -> (Vec<u8>, bool) {
    let channels = channels.max(1);
    let bits = depth.bits();
    let bytes_per_sample = (bits / 8) as usize;
    // Interleaved f32 → the requested container, one "channel" of
    // packed words (channels ride inside the frame layout already).
    let mut input = Vec::with_capacity(samples.len() * 4);
    for sample in samples {
        input.extend_from_slice(&sample.to_le_bytes());
    }
    let mut data = vec![0u8; samples.len() * bytes_per_sample];
    let outcome = match depth {
        WavDepth::Float => {
            data.copy_from_slice(&input);
            pcm::ConvertOutcome {
                bytes: data.len(),
                exact: true,
            }
        }
        WavDepth::Pcm16 | WavDepth::Pcm24 | WavDepth::Pcm32 => {
            pcm::convert(&input, PcmEncoding::Float, 1, bits, bits, &mut data, false)
        }
    };
    let data_len = outcome.bytes;

    let mut out = Vec::with_capacity(44 + data_len);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&((36 + data_len) as u32).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&depth.format_tag().to_le_bytes());
    out.extend_from_slice(&(channels as u16).to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    // byte rate = rate × channels × bytes per sample, block-aligned.
    out.extend_from_slice(&(rate * channels as u32 * bytes_per_sample as u32).to_le_bytes());
    out.extend_from_slice(&((channels * bytes_per_sample) as u16).to_le_bytes());
    out.extend_from_slice(&(bits as u16).to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data_len as u32).to_le_bytes());
    out.extend_from_slice(&data[..data_len]);
    (out, outcome.exact)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    /// A source worth capturing: a 440 Hz sine at half-scale, stereo.
    struct Sine {
        rate: u32,
        at: usize,
    }

    impl RenderSource for Sine {
        fn render(&mut self, out: &mut [f32], channels: usize) {
            for frame in 0..out.len() / channels.max(1) {
                let t = (self.at + frame) as f64 / f64::from(self.rate);
                let value = (2.0 * PI * 440.0 * t).sin() as f32 * 0.5;
                for channel in 0..channels {
                    out[frame * channels + channel] = value;
                }
            }
            self.at += out.len() / channels.max(1);
        }
    }

    #[test]
    fn renders_whole_frames_across_block_boundaries() {
        let mut sine = Sine {
            rate: 48_000,
            at: 0,
        };
        let samples = render_frames(&mut sine, 2, 1_000, 256);
        assert_eq!(samples.len(), 2_000);
        // The phase advances continuously across block edges: sample
        // 256 and 257 (same frame, channels 0/1) agree, and the value
        // at the block boundary matches the closed form.
        let at = |frame: usize| -> f32 {
            (2.0 * PI * 440.0 * (frame as f64 / 48_000.0)).sin() as f32 * 0.5
        };
        for frame in [0usize, 255, 256, 999] {
            assert!(
                (samples[frame * 2] - at(frame)).abs() < 1e-6,
                "frame {frame}: {} vs {}",
                samples[frame * 2],
                at(frame)
            );
            assert_eq!(samples[frame * 2], samples[frame * 2 + 1]);
        }
    }

    #[test]
    fn seconds_round_to_whole_frames() {
        let mut sine = Sine {
            rate: 44_100,
            at: 0,
        };
        let samples = render_seconds(&mut sine, 44_100, 1, 0.5, 512);
        assert_eq!(samples.len(), 22_050);
    }

    #[test]
    fn wav_header_is_a_plain_pcm_file() {
        let mut sine = Sine {
            rate: 48_000,
            at: 0,
        };
        let samples = render_frames(&mut sine, 2, 16, 256);
        let (file, _) = wav(&samples, 48_000, 2, WavDepth::Pcm16);
        assert_eq!(&file[0..4], b"RIFF");
        assert_eq!(&file[8..12], b"WAVE");
        assert_eq!(&file[12..16], b"fmt ");
        assert_eq!(&file[36..40], b"data");
        // PCM, stereo, 48 kHz, 16-bit, block-align 4.
        assert_eq!(u16::from_le_bytes([file[20], file[21]]), 1);
        assert_eq!(u16::from_le_bytes([file[22], file[23]]), 2);
        assert_eq!(
            u32::from_le_bytes([file[24], file[25], file[26], file[27]]),
            48_000
        );
        assert_eq!(u16::from_le_bytes([file[34], file[35]]), 16);
        assert_eq!(u16::from_le_bytes([file[32], file[33]]), 4);
        // 16 frames stereo 16-bit = 64 data bytes; RIFF size counts
        // from byte 8.
        assert_eq!(
            u32::from_le_bytes([file[40], file[41], file[42], file[43]]),
            64
        );
        assert_eq!(
            u32::from_le_bytes([file[4], file[5], file[6], file[7]]),
            36 + 64
        );
        assert_eq!(file.len(), 44 + 64);
        // Whole steps pack exactly.
        let (_, exact) = wav(&[0.0, 0.5, -0.5, 0.25], 48_000, 2, WavDepth::Pcm16);
        assert!(exact);
    }

    #[test]
    fn float_and_32_bit_captures_round_trip() {
        let samples = vec![0.25f32, -0.75, 0.5, -0.125];
        let (file, exact) = wav(&samples, 44_100, 2, WavDepth::Float);
        assert!(exact);
        let body = &file[44..];
        let back: Vec<f32> = body
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect();
        assert_eq!(back, samples);
        // 32-bit integer packing keeps the whole steps whole.
        let (_, exact32) = wav(&samples, 44_100, 2, WavDepth::Pcm32);
        assert!(exact32);
    }

    #[test]
    fn full_range_float_is_inexact_at_16_bits() {
        // The parity rig's sanity: a float capture that isn't on the
        // 16-bit grid reports inexact, so captures compare like with
        // like.
        let (_, exact) = wav(&[0.123_456], 48_000, 1, WavDepth::Pcm16);
        assert!(!exact);
    }
}
