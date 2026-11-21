//! Bit-exact decoder-PCM packing into a DAC's integer container.
//!
//! Port of `playback/audio/pass/PassPcm.kt` (upstream pin `bde01cb4`,
//! whole file, 87 lines): "only the packing changes, never a sample's
//! value, no gain, no dither". This is the conversion the desktop's
//! exclusive and USB outputs use — the mixer's f32 output is packed into
//! the 16/24/32-bit container the DAC was opened at, with the exactness
//! flag telling the caller whether the floats were whole steps of the
//! song's depth to begin with (`PassSink.convert` flips the pass's
//! "exact" status when they were not — the port's exclusive path reports
//! the same through [`crate::output::StreamReport`]).
//!
//! Every sample is lined up at the top of 32 bits, so one writer serves
//! every container (`PassPcm.kt:66-84`).

use crate::precision::PcmEncoding;

/// Packs interleaved decoder PCM into the output container
/// (`PassPcm.convert`).
///
/// * `input` — interleaved decoder PCM bytes, advanced per sample read;
/// * `source_bits` — how many bits the song really has
///   ([`source_bits_of`], `PassPcm.kt:30-40`);
/// * `out_bits` — 16, 24 or 32: the integer container being written;
/// * `muted` — only muting reaches an exclusive output
///   (`PassSink.setVolume`: `volume <= 0.0001`);
///
/// Returns the number of **bytes** written and whether every float sample
/// was a whole step of the song's depth (`exact`).
///
/// Because the port's mixer always produces interleaved f32 (blueprint
/// ch. 11: "f32 throughout"), the input side needs only the float arm of
/// the upstream switch; the integer input arms exist for the offline
/// harness and parity captures and are kept through
/// [`PcmEncoding`].
pub fn convert(
    input: &[u8],
    encoding: PcmEncoding,
    channels: usize,
    source_bits: u32,
    out_bits: u32,
    output: &mut [u8],
    muted: bool,
) -> ConvertOutcome {
    let in_bytes = encoding.bytes_per_sample();
    let out_bytes = (out_bits / 8) as usize;
    let samples = (input.len() / (in_bytes * channels.max(1)))
        .min(output.len() / (out_bytes * channels.max(1)))
        * channels.max(1);
    // Float holds a whole step of the song's depth exactly: 24 bits fit a
    // float's mantissa (PassPcm.kt:69-70).
    let bits = source_bits.min(24);
    let float_scale = (1u64 << (bits - 1)) as f64;
    let float_shift = 32 - bits;
    let mut exact = true;
    let mut written = 0usize;
    let mut at = 0usize;
    for _ in 0..samples {
        // Every sample is lined up at the top of 32 bits, so one writer
        // serves every container (PassPcm.kt:66-84).
        let sample: i32 = match encoding {
            PcmEncoding::Pcm16 => (i16::from_le_bytes([input[at], input[at + 1]]) as i32) << 16,
            PcmEncoding::Pcm16Be => (i16::from_be_bytes([input[at], input[at + 1]]) as i32) << 16,
            PcmEncoding::Pcm24 => read_i24le(input, at) << 8,
            PcmEncoding::Pcm24Be => read_i24be(input, at) << 8,
            PcmEncoding::Pcm32 => i32::from_le_bytes([
                input[at],
                input[at + 1],
                input[at + 2],
                input[at + 3],
            ]),
            PcmEncoding::Pcm32Be => i32::from_be_bytes([
                input[at],
                input[at + 1],
                input[at + 2],
                input[at + 3],
            ]),
            PcmEncoding::Float => {
                let value = f64::from(f32::from_le_bytes([
                    input[at],
                    input[at + 1],
                    input[at + 2],
                    input[at + 3],
                ]));
                let scaled = value * float_scale;
                let step = scaled.round().clamp(-float_scale, float_scale - 1.0) as i64;
                if exact && (scaled - step as f64).abs() > 1e-3 {
                    exact = false;
                }
                (step as i32) << float_shift
            }
        };
        at += in_bytes;
        put_sample(
            output,
            written,
            if muted { 0 } else { sample },
            out_bits,
        );
        written += out_bytes;
    }
    ConvertOutcome { bytes: written, exact }
}

/// The outcome of one packing pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConvertOutcome {
    /// Bytes written to the output container.
    pub bytes: usize,
    /// Whether every float sample was a whole step of the song's depth —
    /// `false` means the song wasn't lossless to begin with
    /// (`PassPcm.convert`'s return value).
    pub exact: bool,
}

/// How many bits the song really has: the file's own depth when the
/// extractor knew it, else what the decoder handed over
/// (`PassPcm.sourceBits`, lines 30-40).
pub fn source_bits_of(decoder_encoding: PcmEncoding, file_bits: Option<u32>) -> u32 {
    match file_bits {
        Some(16) | Some(24) | Some(32) => file_bits.unwrap(),
        _ => match decoder_encoding {
            PcmEncoding::Pcm16 | PcmEncoding::Pcm16Be => 16,
            PcmEncoding::Pcm32 | PcmEncoding::Pcm32Be => 32,
            _ => 24,
        },
    }
}

/// Writes one top-of-32-bits sample into the container
/// (`PassPcm.put`, lines 86-96).
fn put_sample(output: &mut [u8], at: usize, sample: i32, bits: u32) {
    match bits {
        16 => output[at..at + 2].copy_from_slice(&((sample >> 16) as i16).to_le_bytes()),
        24 => {
            output[at] = (sample >> 8) as u8;
            output[at + 1] = (sample >> 16) as u8;
            output[at + 2] = (sample >> 24) as u8;
        }
        _ => output[at..at + 4].copy_from_slice(&sample.to_le_bytes()),
    }
}

fn read_i24le(b: &[u8], at: usize) -> i32 {
    (u32::from(b[at])
        | (u32::from(b[at + 1]) << 8)
        | ((b[at + 2] as i8 as u32) << 16)) as i32
}

fn read_i24be(b: &[u8], at: usize) -> i32 {
    ((b[at] as i8 as u32) << 16 | (u32::from(b[at + 1]) << 8) | u32::from(b[at + 2])) as i32
}

/// Packs one block of the mixer's interleaved f32 output into an integer
/// container — the desktop shortcut over [`convert`] with a float input.
///
/// Exclusive outputs call this per device block; `source_bits` drives the
/// whole-step exactness check exactly like `PassPcm.convert`'s float arm.
pub fn pack_float_block(
    input: &[f32],
    source_bits: u32,
    out_bits: u32,
    output: &mut [u8],
    muted: bool,
) -> ConvertOutcome {
    let bytes: Vec<u8> = input.iter().flat_map(|s| s.to_le_bytes()).collect();
    convert(
        &bytes,
        PcmEncoding::Float,
        1,
        source_bits,
        out_bits,
        output,
        muted,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sixteen_bit_round_trip_is_untouched() {
        // 16-bit in, 16-bit out: byte-for-byte (top-of-32 alignment aside).
        let input: Vec<u8> = [-1i16, 0, 1, i16::MAX, i16::MIN]
            .iter()
            .flat_map(|s| s.to_le_bytes())
            .collect();
        let mut output = vec![0u8; 16];
        let outcome = convert(&input, PcmEncoding::Pcm16, 1, 16, 16, &mut output, false);
        assert!(outcome.exact);
        assert_eq!(outcome.bytes, 10);
        let got: Vec<i16> = output[..10]
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]))
            .collect();
        assert_eq!(got, vec![-1, 0, 1, i16::MAX, i16::MIN]);
    }

    #[test]
    fn float_whole_steps_are_exact() {
        // 24-bit song as floats: every value is a whole 1/2^23 step.
        let steps: Vec<f32> = [-8_388_608i32, 0, 8_388_607, 4_194_304]
            .iter()
            .map(|s| *s as f32 / 8_388_608.0)
            .collect();
        let bytes: Vec<u8> = steps.iter().flat_map(|s| s.to_le_bytes()).collect();
        let mut output = vec![0u8; steps.len() * 3];
        let outcome = convert(&bytes, PcmEncoding::Float, 1, 24, 24, &mut output, false);
        assert!(outcome.exact, "whole steps must be exact");
        // Packed 24-bit little-endian at top of 32: -2^23 → 0x800000 →
        // bytes (sample>>8..) = 00 00 80.
        assert_eq!(&output[..3], &[0x00, 0x00, 0x80]);
        assert_eq!(&output[3..6], &[0x00, 0x00, 0x00]);
    }

    #[test]
    fn float_fractions_are_flagged_inexact() {
        // 16-bit song, a half-step float: not a whole multiple of 1/32768.
        let half_step = 0.5f32 + 0.5 / 32768.0;
        let bytes: Vec<u8> = half_step.to_le_bytes().to_vec();
        let mut output = vec![0u8; 2];
        let outcome = convert(&bytes, PcmEncoding::Float, 1, 16, 16, &mut output, false);
        assert!(!outcome.exact);
        // Still rounds to the nearest step: 0.5 × 32768 + 0.5 = 16384.5 → 16385 (round-half-away).
        assert!(i16::from_le_bytes([output[0], output[1]]) == 16_384
            || i16::from_le_bytes([output[0], output[1]]) == 16_385);
    }

    #[test]
    fn muted_goes_out_as_zero() {
        // A whole 16-bit step, muted: exact and silent.
        let bytes: Vec<u8> = (16384.0f32 / 32768.0).to_le_bytes().to_vec();
        let mut output = vec![0u8; 2];
        let outcome = convert(&bytes, PcmEncoding::Float, 1, 16, 16, &mut output, true);
        assert!(outcome.exact);
        assert_eq!(output[..2], [0, 0]);
    }

    #[test]
    fn source_bits_prefers_the_file() {
        // The file's own depth when known, else the decoder's container.
        assert_eq!(source_bits_of(PcmEncoding::Float, Some(24)), 24);
        assert_eq!(source_bits_of(PcmEncoding::Pcm16, Some(24)), 24);
        assert_eq!(source_bits_of(PcmEncoding::Float, None), 24);
        assert_eq!(source_bits_of(PcmEncoding::Pcm16, None), 16);
        assert_eq!(source_bits_of(PcmEncoding::Pcm32, None), 32);
    }

    #[test]
    fn thirty_two_bit_container_gets_whole_words() {
        // 24-bit song into a 32-bit slot: value sits at the top of 32 bits
        // (sample << 8), low byte zero.
        let value = 8_388_607f32 / 8_388_608.0; // 2^23-1 steps
        let bytes: Vec<u8> = value.to_le_bytes().to_vec();
        let mut output = vec![0u8; 4];
        let outcome = convert(&bytes, PcmEncoding::Float, 1, 24, 32, &mut output, false);
        assert!(outcome.exact);
        // 0x7FFFFF00 little-endian.
        assert_eq!(output, [0x00, 0xFF, 0xFF, 0x7F]);
    }

    #[test]
    fn pack_float_block_matches_convert() {
        let block = [0.25f32, -0.25, 0.5, -0.5];
        let mut a = vec![0u8; 16];
        let mut b = vec![0u8; 16];
        let oa = pack_float_block(&block, 16, 16, &mut a, false);
        let ob = convert(
            &block.iter().flat_map(|s| s.to_le_bytes()).collect::<Vec<u8>>(),
            PcmEncoding::Float,
            1,
            16,
            16,
            &mut b,
            false,
        );
        assert_eq!(oa, ob);
        assert_eq!(a, b);
    }
}
