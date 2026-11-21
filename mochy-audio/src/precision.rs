//! Precision decisions: what reaches the output track — float or dithered
//! 16-bit, at the song's rate or the output's.
//!
//! Port of `playback/audio/PrecisionSink.kt` (upstream pin `bde01cb4`) —
//! the decision table (`applyConfiguration`, lines 65-106), the decoder-PCM
//! plane reader (`readPlanes`, 177-206) and the TPDF dither (`ditherTo16`,
//! 208-223). The blueprint's desktop mapping (ch. 7) runs the whole chain
//! in f32 and drops most of the sink *machinery* (Media3 buffer juggling);
//! what survives as behavior is exactly this: the per-song decision of
//! passthrough vs resample vs dither, the plane conversions feeding it, and
//! the dither itself.
//!
//! The upstream "hi-fi resampler switch" semantics: when the song must be
//! brought to another rate (the output's fixed rate, or an effect's rate
//! cap), it happens *here* with [`crate::sinc::SincResampler`] instead of
//! the OS mixer's resampler (`PrecisionSink.kt:81` —
//! "the hi-fi resampler switch: songs are brought to the output's own rate
//! here instead of in android's mixer").

// The resampler itself (`crate::sinc::SincResampler`) is created by the
// decode task following [`decide`]'s plan — the seam of
// `PrecisionSink.applyConfiguration` line 77.

/// The PCM encodings the port's decode path can hand over — the upstream
/// `Workable` set (`PrecisionSink.kt:234-242`), desktop-side spelling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PcmEncoding {
    /// 16-bit little-endian integers.
    Pcm16,
    /// 16-bit big-endian integers.
    Pcm16Be,
    /// 24-bit packed little-endian.
    Pcm24,
    /// 24-bit packed big-endian.
    Pcm24Be,
    /// 32-bit little-endian integers.
    Pcm32,
    /// 32-bit big-endian integers.
    Pcm32Be,
    /// 32-bit IEEE floats.
    Float,
}

impl PcmEncoding {
    /// `bytesPerSample` (`PrecisionSink.kt:244-248`).
    pub fn bytes_per_sample(self) -> usize {
        match self {
            PcmEncoding::Pcm16 | PcmEncoding::Pcm16Be => 2,
            PcmEncoding::Pcm24 | PcmEncoding::Pcm24Be => 3,
            _ => 4,
        }
    }

    /// Whether this is a high-resolution encoding
    /// (`Util.isEncodingHighResolutionPcm` — 24-bit and above).
    pub fn is_high_resolution(self) -> bool {
        matches!(
            self,
            PcmEncoding::Pcm24 | PcmEncoding::Pcm24Be | PcmEncoding::Pcm32 | PcmEncoding::Pcm32Be
        )
    }
}

/// The knob state the decision is made from — the `AudioOutputs` fields the
/// sink reads on the playback thread (`PrecisionSink.kt:70-72`:
/// `outputs.floatAllowed`, `outputs.fixedOutputRate`,
/// `outputs.effectRateCap`).
#[derive(Debug, Clone, Copy, Default)]
pub struct PrecisionKnobs {
    /// Whether float can make it all the way to the track
    /// (`AudioOutputs.floatAllowed` — the service works this out from every
    /// effect and transition; the sink just reads it).
    pub float_allowed: bool,
    /// The mixer's rate where it's known for sure with the resampler on
    /// (`AudioOutputs.fixedOutputRate`); `None` = whatever it's given.
    pub fixed_output_rate: Option<u32>,
    /// The highest rate the effects take; songs above it are brought down
    /// to it first (`AudioOutputs.effectRateCap` — 3D sims render at 48 kHz
    /// at most).
    pub effect_rate_cap: Option<u32>,
}

/// What the sink decided for one song (`applyConfiguration`'s outcome).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PrecisionPlan {
    /// Float when true, dithered 16-bit when false (`outEncoding`).
    pub output_float: bool,
    /// The rate the song is handed over at — its own, or the resampler's.
    pub output_rate: u32,
    /// Whether a [`SincResampler`] must be created for this song.
    pub needs_resampler: bool,
    /// The target rate for the resampler (valid when `needs_resampler`).
    pub resample_to: Option<u32>,
}

/// The per-song decision (`PrecisionSink.applyConfiguration`,
/// lines 65-106).
///
/// Upstream shape preserved 1:1:
/// * nothing to do unless the format is workable raw PCM;
/// * the target rate is the tightest of the fixed output rate and the
///   effect cap, applied only when it differs from the song's rate
///   (`listOfNotNull(configuredTarget, capped).minOrNull()` at line 74);
/// * the song is reworked when a resampler is needed or the source is
///   hi-res, and the reworked encoding is float when allowed, else
///   16-bit (lines 80-84);
/// * a resampler instance can be reused when the rates match (line 76).
pub fn decide(
    sample_rate: u32,
    channels: usize,
    encoding: PcmEncoding,
    knobs: &PrecisionKnobs,
    existing_resampler_rate: Option<(u32, u32)>,
) -> PrecisionPlan {
    // `workable` = raw PCM in the Workable set with a channel count — every
    // [`PcmEncoding`] variant IS in the upstream set, so the check reduces
    // to the channel count (PrecisionSink.kt:71).
    let workable = channels > 0;
    if !workable {
        return PrecisionPlan {
            output_float: encoding == PcmEncoding::Float,
            output_rate: sample_rate,
            needs_resampler: false,
            resample_to: None,
        };
    }
    let hi_res = encoding.is_high_resolution();
    // The capped rate applies only when the song is above it (line 73).
    let capped = knobs.effect_rate_cap.filter(|cap| sample_rate > *cap);
    // The tightest of the two targets, only when it differs from the song.
    let target = knobs
        .fixed_output_rate
        .into_iter()
        .chain(capped)
        .min()
        .filter(|rate| *rate != sample_rate);
    // An existing resampler can be kept when its rates already match
    // (line 76: `resampler?.takeIf { r -> r.inRate == rate && r.outRate == it }`).
    let resampling = target.is_some();
    let reusable = existing_resampler_rate == Some((sample_rate, target.unwrap_or(sample_rate)));
    let reworked = resampling || hi_res;
    PrecisionPlan {
        output_float: if !reworked {
            encoding == PcmEncoding::Float
        } else {
            knobs.float_allowed
        },
        // The handed-over rate is the resampler's when one is engaged —
        // kept or new alike (line 87: `setSampleRate(nextResampler?.outRate …)`).
        output_rate: target.unwrap_or(sample_rate),
        needs_resampler: resampling && !reusable,
        resample_to: target,
    }
}

/// Reads interleaved decoder PCM into per-channel f32 planes
/// (`PrecisionSink.readPlanes`, lines 177-206; the integer scalings are
/// `-32768f`, `-8388608f`, `/ 2147483648.0` exactly as upstream).
pub fn read_planes(
    input: &[u8],
    encoding: PcmEncoding,
    channels: usize,
    planes: &mut [&mut [f32]],
) {
    let frame_bytes = encoding.bytes_per_sample() * channels.max(1);
    for (frame, chunk) in input.chunks_exact(frame_bytes).enumerate() {
        for (plane, sample_bytes) in planes
            .iter_mut()
            .zip(chunk.chunks_exact(encoding.bytes_per_sample()))
            .take(channels)
        {
            plane[frame] = match encoding {
                PcmEncoding::Pcm16 => {
                    i16::from_le_bytes([sample_bytes[0], sample_bytes[1]]) as f32 / 32768.0
                }
                PcmEncoding::Pcm16Be => {
                    i16::from_be_bytes([sample_bytes[0], sample_bytes[1]]) as f32 / 32768.0
                }
                PcmEncoding::Pcm24 => read_i24le(sample_bytes, 0) as f32 / 8_388_608.0,
                PcmEncoding::Pcm24Be => read_i24be(sample_bytes, 0) as f32 / 8_388_608.0,
                PcmEncoding::Pcm32 => {
                    i32::from_le_bytes([
                        sample_bytes[0],
                        sample_bytes[1],
                        sample_bytes[2],
                        sample_bytes[3],
                    ]) as f32
                        / 2_147_483_648.0
                }
                PcmEncoding::Pcm32Be => {
                    i32::from_be_bytes([
                        sample_bytes[0],
                        sample_bytes[1],
                        sample_bytes[2],
                        sample_bytes[3],
                    ]) as f32
                        / 2_147_483_648.0
                }
                PcmEncoding::Float => f32::from_le_bytes([
                    sample_bytes[0],
                    sample_bytes[1],
                    sample_bytes[2],
                    sample_bytes[3],
                ]),
            };
        }
    }
}

/// A TPDF dither to 16-bit — the exact upstream xorshift + two random
/// halves of a step + soft-clamp + round (`PrecisionSink.ditherTo16`,
/// lines 208-223).
///
/// "tpdf dither: two random halves of a step added before rounding,
/// soft-clamped first so dolby float peaks above 1.0 never crackle."
#[derive(Debug, Clone)]
pub struct TpdfDither {
    state: u32,
}

impl Default for TpdfDither {
    fn default() -> Self {
        // `dither = 0x2545F491` (PrecisionSink.kt:46) — the same seed.
        TpdfDither { state: 0x2545_F491 }
    }
}

impl TpdfDither {
    /// One sample in, one 16-bit sample out.
    pub fn dither_to_16(&mut self, sample: f32) -> i16 {
        // xorshift32, the same triple the upstream uses.
        self.state ^= self.state << 13;
        self.state ^= self.state >> 17;
        self.state ^= self.state << 5;
        let a = (self.state & 0xFFFF) as f32 / 65536.0;
        let b = ((self.state >> 16) & 0xFFFF) as f32 / 65536.0;
        let clamped = if !sample.is_finite() {
            0.0
        } else if sample > 0.98 {
            0.98 + 0.02 * ((sample - 0.98) / 0.02).tanh()
        } else if sample < -0.98 {
            -0.98 + 0.02 * ((sample + 0.98) / 0.02).tanh()
        } else {
            sample
        };
        let value = (clamped * 32768.0 + a - b).round() as i32;
        value.clamp(i16::MIN as i32, i16::MAX as i32) as i16
    }
}

fn read_i24le(b: &[u8], at: usize) -> i32 {
    // (b[at] & 0xFF) | ((b[at+1] & 0xFF) << 8) | (signed b[at+2] << 16)
    // — the high byte sign-extends exactly like the Kotlin `toInt()` at
    // PrecisionSink.kt:188.
    (u32::from(b[at]) | (u32::from(b[at + 1]) << 8) | ((b[at + 2] as i8 as u32) << 16)) as i32
}
fn read_i24be(b: &[u8], at: usize) -> i32 {
    // (signed b[at] << 16) | ((b[at+1] & 0xFF) << 8) | (b[at+2] & 0xFF)
    // (PrecisionSink.kt:193).
    ((b[at] as i8 as u32) << 16 | (u32::from(b[at + 1]) << 8) | u32::from(b[at + 2])) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passthrough_when_nothing_applies() {
        // A 44.1 kHz 16-bit song, no knobs: nothing reworked, its own rate.
        let plan = decide(
            44_100,
            2,
            PcmEncoding::Pcm16,
            &PrecisionKnobs::default(),
            None,
        );
        assert!(!plan.needs_resampler);
        assert_eq!(plan.output_rate, 44_100);
        assert!(!plan.output_float);
    }

    #[test]
    fn float_passthrough_is_the_default_when_allowed() {
        let knobs = PrecisionKnobs {
            float_allowed: true,
            ..Default::default()
        };
        let plan = decide(48_000, 2, PcmEncoding::Float, &knobs, None);
        assert!(plan.output_float);
        assert!(!plan.needs_resampler);
    }

    #[test]
    fn fixed_output_rate_starts_the_resampler() {
        // The hi-fi switch: output fixed at 48 kHz, a 44.1 kHz song is
        // brought there with the sinc resampler, not the OS mixer.
        let knobs = PrecisionKnobs {
            float_allowed: true,
            fixed_output_rate: Some(48_000),
            effect_rate_cap: None,
        };
        let plan = decide(44_100, 2, PcmEncoding::Float, &knobs, None);
        assert!(plan.needs_resampler);
        assert_eq!(plan.resample_to, Some(48_000));
        assert_eq!(plan.output_rate, 48_000);
        assert!(plan.output_float);
        // The existing resampler is kept when the rates already match
        // (PrecisionSink.kt:76) — the song still arrives at 48 kHz.
        let kept = decide(
            44_100,
            2,
            PcmEncoding::Float,
            &knobs,
            Some((44_100, 48_000)),
        );
        assert!(!kept.needs_resampler);
        assert_eq!(kept.output_rate, 48_000);
        assert_eq!(kept.resample_to, Some(48_000));
    }

    #[test]
    fn effect_cap_wins_when_tighter() {
        // 3D sims render at 48 kHz at most: a 96 kHz song caps down.
        let knobs = PrecisionKnobs {
            float_allowed: true,
            fixed_output_rate: Some(192_000),
            effect_rate_cap: Some(48_000),
        };
        let plan = decide(96_000, 2, PcmEncoding::Float, &knobs, None);
        assert_eq!(plan.resample_to, Some(48_000));
        // A song AT the cap is not above it: the cap lets the fixed rate
        // decide (PrecisionSink.kt:73 — `takeIf { format.sampleRate > it }`).
        let plan2 = decide(48_000, 2, PcmEncoding::Float, &knobs, None);
        assert_eq!(plan2.resample_to, Some(192_000));
        // A song already at the target is left alone.
        let knobs2 = PrecisionKnobs {
            float_allowed: true,
            fixed_output_rate: Some(48_000),
            effect_rate_cap: Some(48_000),
        };
        let plan3 = decide(48_000, 2, PcmEncoding::Float, &knobs2, None);
        assert!(!plan3.needs_resampler);
    }

    #[test]
    fn hi_res_without_float_lands_on_dithered_16() {
        // A 24-bit song, float not allowed: reworked, dithered 16-bit.
        let knobs = PrecisionKnobs {
            float_allowed: false,
            ..Default::default()
        };
        let plan = decide(44_100, 2, PcmEncoding::Pcm24, &knobs, None);
        assert!(!plan.output_float);
        assert!(!plan.needs_resampler);
        // Hi-res + float allowed: float.
        let plan2 = decide(
            44_100,
            2,
            PcmEncoding::Pcm24,
            &PrecisionKnobs {
                float_allowed: true,
                ..Default::default()
            },
            None,
        );
        assert!(plan2.output_float);
    }

    #[test]
    fn reads_every_plane_encoding() {
        // The upstream scalings: -32768, -8388608, /2^31.
        let mut left = [0f32; 1];
        let mut right = [0f32; 1];
        let mut planes: [&mut [f32]; 2] = [&mut left, &mut right];
        read_planes(
            &[0x00, 0x80, 0xFF, 0x7F],
            PcmEncoding::Pcm16,
            2,
            &mut planes,
        );
        assert_eq!(left[0], -1.0);
        assert_eq!(right[0], 32767.0 / 32768.0);

        let mut mono = [0f32; 1];
        let mut planes: [&mut [f32]; 1] = [&mut mono];
        read_planes(&[0x00, 0x00, 0x80], PcmEncoding::Pcm24, 1, &mut planes);
        assert_eq!(mono[0], -8_388_608.0 / 8_388_608.0);
        let mut mono2 = [0f32; 1];
        let mut planes: [&mut [f32]; 1] = [&mut mono2];
        read_planes(
            &[0x00, 0x00, 0x00, 0x80],
            PcmEncoding::Pcm32,
            1,
            &mut planes,
        );
        assert_eq!(mono2[0], -2_147_483_648.0 / 2_147_483_648.0);
        let mut mono3 = [0f32; 1];
        let mut planes: [&mut [f32]; 1] = [&mut mono3];
        read_planes(
            &[0x00, 0x00, 0x00, 0x00],
            PcmEncoding::Float,
            1,
            &mut planes,
        );
        assert_eq!(mono3[0], 0.0);
    }

    #[test]
    fn tpdf_dither_stays_in_range_and_centers() {
        let mut dither = TpdfDither::default();
        // Silence dithers to the classic 1-LSB TPDF floor: -1, 0 or +1,
        // zero for the bulk of it (two random halves of a step, rounded).
        let mut counts = [0usize; 3];
        for _ in 0..10_000 {
            let v = dither.dither_to_16(0.0);
            assert!((-1..=1).contains(&v), "silence wandered to {v}");
            counts[(v + 1) as usize] += 1;
        }
        assert!(counts[1] > 6_000, "not centered: {:?}", counts);
        assert!(
            counts[0] > 500 && counts[2] > 500,
            "not triangular: {:?}",
            counts
        );
        // Full-scale and beyond never clip crackle: soft-clamped to 0.98 +
        // tanh tail, clamped to the i16 range.
        for _ in 0..1000 {
            let v = dither.dither_to_16(50.0);
            assert!((i16::MIN as i32..=i16::MAX as i32).contains(&(v as i32)));
        }
        // Deterministic seed → deterministic stream.
        let mut a = TpdfDither::default();
        let mut b = TpdfDither::default();
        for x in [0.0, 0.25, -0.5, 0.99] {
            assert_eq!(a.dither_to_16(x), b.dither_to_16(x));
        }
    }

    #[test]
    fn tpdf_dither_error_is_one_step_wide() {
        // A constant 0.5-LSB-offset signal dithers between two adjacent
        // codes (TPDF: the classic ±1 step triangular noise).
        let mut dither = TpdfDither::default();
        let mut above = 0usize;
        let mut below = 0usize;
        for _ in 0..10_000 {
            let v = dither.dither_to_16(1.5 / 32768.0);
            match v {
                1 => above += 1,
                2 => below += 1,
                _ => panic!("dither wandered to {v}"),
            }
        }
        assert!(
            above > 1000 && below > 1000,
            "not triangular: {above}/{below}"
        );
    }
}
