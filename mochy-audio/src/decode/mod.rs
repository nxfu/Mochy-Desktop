//! The symphonia decode pipeline.
//!
//! The desktop replacement for `MediaCodec` + `FFmpeg ext`
//! (blueprint ch. 11 Table 23 "Demux/decode"): symphonia for
//! WebM/MP4/Ogg/FLAC/MP3/ALAC/WAV/PCM, Opus through the
//! `symphonia-adapter-libopus` adapter (the workspace pin; there is no
//! pure-Rust Opus decoder), and — behind the `fdk-aac` feature — HE-AAC
//! through `symphonia-adapter-fdk-aac` (the vendored Fraunhofer library;
//! registering it after the built-ins makes it the AAC decoder while the
//! feature is on, mirroring the upstream distinction where a module's
//! codec name "knows its encode better than the container says: HE-AAC
//! reads as plain AAC there" — MusicService.kt
//! `recordDecodedFormat`).
//!
//! Gapless: [`gapless`] supplies the MP4/AAC encoder-delay accounting
//! symphonia lacks (verified against 0.6.1: the isomp4 demuxer parses
//! `elst` into its atom tree but never populates `Track::delay`/
//! `Track::padding`); [`TrackDecoder`] applies the trim
//! sample-accurately so track-to-track transitions on album playback
//! are seamless — the single-mixer architecture makes the transition
//! itself free, the next voice is simply there when this one ends.

pub mod gapless;

use std::io::{Read, Seek, SeekFrom};
use std::sync::OnceLock;

use symphonia::core::codecs::CodecParameters;
use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::codecs::registry::CodecRegistry;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::io::MediaSource;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::{Time, TimeBase};

/// The codec registry: symphonia's enabled set plus the Opus adapter (and
/// FDK AAC behind its feature). Registration order decides the winner for
/// a codec claimed twice — the later `register_audio_decoder` replaces
/// the earlier at the same tier, so FDK (registered last) owns AAC while
/// its feature is on.
pub fn codec_registry() -> &'static CodecRegistry {
    static REGISTRY: OnceLock<CodecRegistry> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        let mut registry = CodecRegistry::new();
        symphonia::default::register_enabled_codecs(&mut registry);
        registry.register_audio_decoder::<symphonia_adapter_libopus::OpusDecoder>();
        #[cfg(feature = "fdk-aac")]
        registry.register_audio_decoder::<symphonia_adapter_fdk_aac::AacDecoder>();
        registry
    })
}

#[derive(thiserror::Error, Debug)]
pub enum DecodeError {
    #[error("no audio track in the stream")]
    NoAudioTrack,
    #[error("the decoder wouldn't start: {0}")]
    Init(String),
    #[error("decode failed: {0}")]
    Decode(String),
    #[error("seek failed: {0}")]
    Seek(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// What one opened track says about itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrackInfo {
    pub sample_rate: u32,
    pub channels: usize,
    /// The codec's short name (e.g. "aac", "opus", "flac").
    pub codec: &'static str,
    /// Total playable samples per channel when the container says.
    pub total_samples: Option<u64>,
    /// The gapless trim the container carries, applied or not.
    pub gapless: Option<gapless::GaplessInfo>,
    /// Whether the gapless trim was applied (its timescale matched the
    /// sample rate).
    pub gapless_applied: bool,
}

/// One gapless-aware block of decoded audio.
#[derive(Debug, Clone)]
pub struct DecodedBlock {
    /// Interleaved f32 frames (any source layout is interleaved here).
    pub samples: Vec<f32>,
    /// The media-sample index this block's first frame sits at, **after**
    /// the gapless trim — the sample-accurate position the clock
    /// consumes.
    pub first_sample: u64,
}

/// One decodable track: demux + decode + gapless accounting.
pub struct TrackDecoder {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track_id: u32,
    time_base: Option<TimeBase>,
    info: TrackInfo,
    /// Encoder priming still to skip (post-seek bookkeeping included).
    skip_remaining: u64,
    /// Tail samples still allowed out; `None` = unbounded.
    emit_remaining: Option<u64>,
    /// Media samples emitted so far (post-trim).
    emitted: u64,
    /// A partially-consumed packet from a previous `decode_block` call.
    pending: Option<PendingPacket>,
}

struct PendingPacket {
    /// Interleaved samples of the packet.
    buffer: Vec<f32>,
    channels: usize,
    /// Frames of this packet already consumed.
    consumed: usize,
}

impl TrackDecoder {
    /// Opens the default audio track of a source. An MP4 source is walked
    /// for its gapless edit list first (see [`gapless`]), rewound, then
    /// handed to symphonia; other containers report no trim.
    pub fn open(
        mut source: Box<dyn MediaSource>,
        extension: Option<&str>,
    ) -> Result<TrackDecoder, DecodeError> {
        let gapless = parse_gapless_from_source(source.as_mut())?;
        Self::open_with_gapless(source, extension, gapless)
    }

    fn open_with_gapless(
        mut source: Box<dyn MediaSource>,
        extension: Option<&str>,
        gapless: Option<gapless::GaplessInfo>,
    ) -> Result<TrackDecoder, DecodeError> {
        source
            .seek(SeekFrom::Start(0))
            .map_err(|e| DecodeError::Init(format!("rewind: {e}")))?;
        let mss = symphonia::core::io::MediaSourceStream::new(source, Default::default());
        let mut hint = Hint::new();
        if let Some(extension) = extension {
            hint.with_extension(extension);
        }
        let format = symphonia::default::get_probe()
            .probe(
                &hint,
                mss,
                FormatOptions::default(),
                MetadataOptions::default(),
            )
            .map_err(|e| DecodeError::Init(e.to_string()))?;
        let track = format
            .default_track(TrackType::Audio)
            .ok_or(DecodeError::NoAudioTrack)?;
        let params: &symphonia::core::codecs::audio::AudioCodecParameters = track
            .codec_params
            .as_ref()
            .and_then(CodecParameters::audio)
            .ok_or_else(|| DecodeError::Init("codec parameters missing".into()))?;
        let decoder = codec_registry()
            .make_audio_decoder(params, &AudioDecoderOptions::default())
            .map_err(|e| DecodeError::Init(e.to_string()))?;
        let sample_rate = params.sample_rate.unwrap_or(0);
        let channels = params.channels.as_ref().map(|c| c.count()).unwrap_or(0);
        if sample_rate == 0 || channels == 0 {
            return Err(DecodeError::Init("no sample rate or channels".into()));
        }
        let codec = decoder.codec_info().short_name;
        // num_frames excludes delay/padding frames per its docs — but the
        // isomp4 demuxer fills it from the media duration, which INCLUDES
        // the encoder padding for edited tracks; the gapless trim below
        // cuts both ends back to the playable span.
        let total_samples = track.num_frames;
        let gapless_applied = matches!(
            &gapless,
            Some(info) if info.timescale == sample_rate && info.priming_samples > 0
        );
        let (skip_remaining, emit_remaining) = match &gapless {
            Some(info) if info.timescale == sample_rate => {
                let playable = total_samples.map(|total| {
                    total.saturating_sub(info.priming_samples + info.padding_samples.unwrap_or(0))
                });
                (info.priming_samples, playable)
            }
            _ => (0, total_samples),
        };
        let info = TrackInfo {
            sample_rate,
            channels,
            codec,
            total_samples,
            gapless,
            gapless_applied,
        };
        Ok(TrackDecoder {
            time_base: track.time_base,
            track_id: track.id,
            format,
            decoder,
            info,
            skip_remaining,
            emit_remaining,
            emitted: 0,
            pending: None,
        })
    }

    /// The opened track's facts.
    pub fn info(&self) -> &TrackInfo {
        &self.info
    }

    /// Decodes the next block of interleaved f32 frames (at most
    /// `max_frames`). Returns `None` at the end of the trimmed track.
    pub fn decode_block(&mut self, max_frames: usize) -> Result<Option<DecodedBlock>, DecodeError> {
        loop {
            // Drain a partially-consumed packet first.
            if let Some(pending) = self.pending.take() {
                let frames = pending.buffer.len() / pending.channels - pending.consumed;
                if frames > 0 {
                    if let Some(block) =
                        self.take_packet_frames(pending, max_frames.min(frames).max(1))
                    {
                        return Ok(Some(block));
                    }
                    // The rest of the packet fell inside a trim: read on.
                    continue;
                }
            }
            // Pull the next packet (0.6: Ok(None) is the end).
            let packet = match self.format.next_packet() {
                Ok(Some(packet)) => packet,
                Ok(None) => return Ok(None),
                Err(symphonia::core::errors::Error::ResetRequired) => {
                    return Err(DecodeError::Decode("decoder reset required".into()));
                }
                Err(e) => return Err(DecodeError::Decode(e.to_string())),
            };
            if packet.track_id != self.track_id {
                continue;
            }
            match self.decoder.decode(&packet) {
                Ok(decoded) => {
                    let channels = decoded.spec().channels().count();
                    let mut buffer = vec![0f32; decoded.samples_interleaved()];
                    decoded.copy_to_slice_interleaved(&mut buffer);
                    let pending = PendingPacket {
                        buffer,
                        channels,
                        consumed: 0,
                    };
                    if let Some(block) = self.take_packet_frames(pending, max_frames) {
                        return Ok(Some(block));
                    }
                    // The whole packet fell inside a trim: keep reading.
                }
                Err(symphonia::core::errors::Error::DecodeError(_))
                | Err(symphonia::core::errors::Error::IoError(_)) => {
                    // A corrupt or short packet is skipped — MediaCodec's
                    // error concealment equivalent upstream relied on.
                    continue;
                }
                Err(e) => return Err(DecodeError::Decode(e.to_string())),
            }
        }
    }

    /// Applies the priming/padding trims to one packet's frames, emitting
    /// at most `max_frames`; stashes the remainder as `pending`.
    fn take_packet_frames(
        &mut self,
        mut pending: PendingPacket,
        max_frames: usize,
    ) -> Option<DecodedBlock> {
        let channels = pending.channels;
        let total_frames = pending.buffer.len() / channels;
        // Leading trim.
        let mut start = 0usize;
        if self.skip_remaining > 0 {
            let skip = (self.skip_remaining as usize).min(total_frames);
            start = skip;
            self.skip_remaining -= skip as u64;
        }
        // Tail trim.
        let allowed = self.emit_remaining.unwrap_or(usize::MAX as u64) as usize;
        let available = total_frames - start;
        let take = available.min(allowed).min(max_frames);
        if take == 0 {
            if available == 0 {
                self.emit_remaining = Some(0);
            }
            return None;
        }
        if let Some(remaining) = self.emit_remaining.as_mut() {
            *remaining -= take as u64;
        }
        let first_sample = self.emitted;
        self.emitted += take as u64;
        pending.consumed = start + take;
        let samples = pending.buffer[start * channels..(start + take) * channels].to_vec();
        if pending.consumed < total_frames {
            self.pending = Some(pending);
        }
        Some(DecodedBlock {
            samples,
            first_sample,
        })
    }

    /// Seeks to a post-trim media sample (the position the clock shows).
    /// Returns the sample the seek actually landed on.
    pub fn seek(&mut self, to_sample: u64) -> Result<u64, DecodeError> {
        self.pending = None;
        // Seek target in pre-trim (container) samples, expressed in the
        // track's timebase.
        let rate = self.info.sample_rate;
        let container_sample = to_sample + self.trim_offset();
        // Time::try_new(seconds, nanos) — the sample index converted by
        // hand (no From<u64> for Time in 0.6).
        let secs = (container_sample / u64::from(rate)) as i64;
        let nanos = ((container_sample % u64::from(rate)) * 1_000_000_000 / u64::from(rate)) as u32;
        let time = Time::try_new(secs, nanos)
            .ok_or_else(|| DecodeError::Seek("seek time out of range".into()))?;
        let seeked = self
            .format
            .seek(
                SeekMode::Accurate,
                SeekTo::Time {
                    time,
                    track_id: Some(self.track_id),
                },
            )
            .map_err(|e| DecodeError::Seek(e.to_string()))?;
        // Where the seek landed, in samples: convert through the timebase
        // the demuxer actually used.
        let landed_container = match self.time_base {
            Some(tb) => {
                let seconds = tb.calc_time_saturating(seeked.actual_ts);
                (seconds.as_secs_f64() * f64::from(rate)).round() as u64
            }
            None => container_sample,
        };
        let landed_post_trim = landed_container.saturating_sub(self.trim_offset());
        // Re-arm the trims for the new position: the demuxer landed at or
        // before the target, packets between here and the target are
        // skipped by the leading-trim accounting.
        self.skip_remaining =
            landed_container.saturating_sub(self.trim_offset() + landed_post_trim);
        if let Some(playable) = self.emit_remaining_origin() {
            self.emit_remaining = Some(playable.saturating_sub(landed_post_trim));
        }
        self.emitted = landed_post_trim;
        Ok(landed_post_trim)
    }

    /// The pre-trim offset of the currently playing span (the priming).
    fn trim_offset(&self) -> u64 {
        match (&self.info.gapless, self.info.gapless_applied) {
            (Some(info), true) => info.priming_samples,
            _ => 0,
        }
    }

    /// The total playable samples after both trims, when known.
    fn emit_remaining_origin(&self) -> Option<u64> {
        let info = self
            .info
            .gapless
            .as_ref()
            .filter(|_| self.info.gapless_applied)?;
        Some(
            self.info
                .total_samples?
                .saturating_sub(info.priming_samples + info.padding_samples.unwrap_or(0)),
        )
    }
}

/// A Read+Seek view over a `dyn MediaSource` — supertrait methods are
/// callable on the trait object, but the object itself implements
/// neither trait, so the gapless walker drives it through this shim.
struct SourceReader<'a>(&'a mut dyn MediaSource);

impl Read for SourceReader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.0.read(buf)
    }
}

impl Seek for SourceReader<'_> {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        self.0.seek(pos)
    }
}

/// Runs the MP4 gapless walk over a `MediaSource` (Read+Seek). The
/// caller rewinds afterwards; the box parser itself returns `None` for
/// anything that isn't an MP4 with an audio edit list.
fn parse_gapless_from_source(
    source: &mut dyn MediaSource,
) -> std::io::Result<Option<gapless::GaplessInfo>> {
    gapless::parse_mp4_gapless(&mut SourceReader(source))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Read, Seek};

    /// A minimal valid WAV: a deterministic ramp, 16-bit.
    fn wav_bytes(rate: u32, frames: usize, channels: usize) -> Vec<u8> {
        let mut wav = Vec::new();
        let data_bytes = frames * channels * 2;
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&((36 + data_bytes) as u32).to_le_bytes());
        wav.extend_from_slice(b"WAVE");
        wav.extend_from_slice(b"fmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
        wav.extend_from_slice(&(channels as u16).to_le_bytes());
        wav.extend_from_slice(&rate.to_le_bytes());
        wav.extend_from_slice(&(rate * channels as u32 * 2).to_le_bytes());
        wav.extend_from_slice(&((channels * 2) as u16).to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(data_bytes as u32).to_le_bytes());
        for i in 0..frames * channels {
            let value = ((i as i32 * 97) % 8000 - 4000) as i16;
            wav.extend_from_slice(&value.to_le_bytes());
        }
        wav
    }

    struct CursorSource(Cursor<Vec<u8>>);

    impl Read for CursorSource {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            self.0.read(buf)
        }
    }
    impl Seek for CursorSource {
        fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
            self.0.seek(pos)
        }
    }
    impl MediaSource for CursorSource {
        fn is_seekable(&self) -> bool {
            true
        }
        fn byte_len(&self) -> Option<u64> {
            Some(self.0.get_ref().len() as u64)
        }
    }

    fn decode_all(bytes: Vec<u8>) -> (TrackInfo, Vec<f32>) {
        let mut decoder =
            TrackDecoder::open(Box::new(CursorSource(Cursor::new(bytes))), Some("wav")).unwrap();
        let info = *decoder.info();
        let mut out = Vec::new();
        while let Some(block) = decoder.decode_block(4096).unwrap() {
            out.extend(block.samples);
        }
        (info, out)
    }

    #[test]
    fn decodes_wav_to_interleaved_f32() {
        let (info, samples) = decode_all(wav_bytes(44_100, 300, 2));
        assert_eq!(info.sample_rate, 44_100);
        assert_eq!(info.channels, 2);
        assert_eq!(info.codec, "pcm_s16le");
        assert_eq!(samples.len(), 600);
        // First frame matches the written ramp.
        assert!((samples[0] - f32::from(-4000i16) / 32768.0f32).abs() < 1e-6);
        assert!((samples[1] - f32::from(97i16 - 4000) / 32768.0f32).abs() < 1e-6);
    }

    #[test]
    fn decode_blocks_cover_the_whole_track_in_order() {
        // Awkward block sizes must not drop or duplicate frames.
        let (_, samples) = decode_all(wav_bytes(48_000, 5000, 1));
        let mut expected = Vec::new();
        for i in 0..5000 {
            let value = ((i * 97) % 8000 - 4000) as i16;
            expected.push(f32::from(value) / 32768.0f32);
        }
        assert_eq!(samples.len(), expected.len());
        for (i, (a, b)) in samples.iter().zip(&expected).enumerate() {
            assert!((a - b).abs() < 1e-6, "frame {i}: {a} vs {b}");
        }
    }

    #[test]
    fn first_sample_accounts_blocks() {
        // Every block's first_sample chains into a gapless numbering.
        let mut decoder = TrackDecoder::open(
            Box::new(CursorSource(Cursor::new(wav_bytes(44_100, 10_000, 1)))),
            Some("wav"),
        )
        .unwrap();
        let mut cursor = 0u64;
        while let Some(block) = decoder.decode_block(333).unwrap() {
            assert_eq!(block.first_sample, cursor);
            cursor += block.samples.len() as u64;
        }
        assert_eq!(cursor, 10_000);
    }

    #[test]
    fn no_audio_track_is_an_error() {
        // A text file: the probe fails to find any format.
        let result = TrackDecoder::open(
            Box::new(CursorSource(Cursor::new(b"not audio at all".to_vec()))),
            None,
        );
        assert!(matches!(result, Err(DecodeError::Init(_))));
    }

    #[test]
    fn wav_carries_no_gapless_trim() {
        let (info, samples) = decode_all(wav_bytes(44_100, 100, 1));
        assert_eq!(info.gapless, None);
        assert!(!info.gapless_applied);
        assert_eq!(samples.len(), 100);
        assert_eq!(info.total_samples, Some(100));
    }

    #[test]
    fn seek_lands_and_continues() {
        let mut decoder = TrackDecoder::open(
            Box::new(CursorSource(Cursor::new(wav_bytes(44_100, 4096, 1)))),
            Some("wav"),
        )
        .unwrap();
        let landed = decoder.seek(1000).unwrap();
        let mut got = Vec::new();
        while let Some(block) = decoder.decode_block(4096).unwrap() {
            assert!(block.first_sample >= landed);
            got.extend(block.samples);
        }
        // We land at or before the request and play through to the end.
        assert!(landed <= 1000);
        assert_eq!(got.len(), (4096 - landed) as usize);
    }

    #[test]
    fn opus_adapter_is_registered() {
        // The registry resolves the Opus codec id through the adapter.
        use symphonia::core::audio::layouts;
        let mut params = symphonia::core::codecs::audio::AudioCodecParameters::new();
        params.for_codec(symphonia::core::codecs::audio::well_known::CODEC_ID_OPUS);
        params.with_sample_rate(48_000);
        params.with_channels(layouts::CHANNEL_LAYOUT_STEREO);
        let decoder = codec_registry().make_audio_decoder(&params, &AudioDecoderOptions::default());
        assert!(decoder.is_ok(), "opus must resolve through the adapter");
    }
}
