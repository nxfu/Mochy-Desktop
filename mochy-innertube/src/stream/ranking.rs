//! Format ranking — the pure heart of stream selection:
//! `selectAudioFormatCandidates` (YTPlayerUtils.kt:939-1047).
//!
//! Every audio itag YouTube serves is ranked by **comparators**, not
//! by an itag list (the itag ladder exists only in an upstream
//! comment): direct-URL presence, YouTube's own `AUDIO_QUALITY_*`
//! label, channel count, bitrate, codec family (Opus over AAC) and
//! sample rate, in that order. LOW and MEDIUM are bitrate *ceilings*
//! (70 kbps / 135 kbps) — best stream at or under the ceiling, then
//! the cheapest above it, so a sparse ladder still answers.
//!
//! The caller owns the two runtime inputs upstream reads from
//! Android globals: whether the active network is metered
//! (`ConnectivityManager.isActiveNetworkMetered`) and whether data
//! saver is on for it (`DataSaver.isActive(metered)`,
//! utils/DataSaver.kt:38-42 — the settings store + metered state).
//! Keeping them parameters is what makes the function pure and
//! table-testable; the app shell computes them per play.

use crate::models::response::player::{Format, PlayerResponse};

/// `AudioQuality` (constants/PreferenceKeys.kt:174-186).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AudioQuality {
    /// Best available from whichever backend is active.
    Auto,
    // YouTube tiers
    Low,
    Medium,
    High,
    // Monochrome (TIDAL-backed) tiers — when YouTube is the source
    // these still mean "its best available stream"
    Lossless,
    HiResLossless,
    DolbyAtmos,
}

impl AudioQuality {
    /// The settings-registry value names (mochy-config persists these
    /// as strings; the mapping is the enum name).
    pub fn as_setting_str(self) -> &'static str {
        match self {
            AudioQuality::Auto => "AUTO",
            AudioQuality::Low => "LOW",
            AudioQuality::Medium => "MEDIUM",
            AudioQuality::High => "HIGH",
            AudioQuality::Lossless => "LOSSLESS",
            AudioQuality::HiResLossless => "HI_RES_LOSSLESS",
            AudioQuality::DolbyAtmos => "DOLBY_ATMOS",
        }
    }

    /// Parse the settings-registry name (None for anything else).
    pub fn from_setting_str(value: &str) -> Option<Self> {
        Some(match value {
            "AUTO" => AudioQuality::Auto,
            "LOW" => AudioQuality::Low,
            "MEDIUM" => AudioQuality::Medium,
            "HIGH" => AudioQuality::High,
            "LOSSLESS" => AudioQuality::Lossless,
            "HI_RES_LOSSLESS" => AudioQuality::HiResLossless,
            "DOLBY_ATMOS" => AudioQuality::DolbyAtmos,
            _ => return None,
        })
    }
}

/// The runtime inputs the ranking needs — what upstream reads from
/// Android globals at call time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RankingConditions {
    /// The user's audio quality setting.
    pub quality: AudioQuality,
    /// `connectivityManager.isActiveNetworkMetered`.
    pub network_metered: bool,
    /// `DataSaver.isActive(networkMetered)` (DataSaver.kt:35-42: on
    /// when its switch is on, and only on mobile data unless set to
    /// always).
    pub data_saver: bool,
}

/// `MEDIUM`'s ceiling sits just under the ~160 kbps Opus rung so it
/// lands on the 128 kbps AAC stream; `LOW`'s on the ~70 kbps Opus
/// rung (YTPlayerUtils.kt:989-995).
const LOW_CEILING_BPS: i64 = 70_000;
const MEDIUM_CEILING_BPS: i64 = 135_000;

/// The effective quality after the data-saver and AUTO rules
/// (YTPlayerUtils.kt:978-987).
pub(crate) fn effective_quality(conditions: &RankingConditions) -> AudioQuality {
    match conditions.quality {
        // data saver keeps youtube on its lightest stream, whatever
        // was picked
        _ if conditions.data_saver => AudioQuality::Low,
        AudioQuality::Auto => {
            if conditions.network_metered {
                AudioQuality::Medium
            } else {
                AudioQuality::High
            }
        }
        // Lossless and Atmos tiers were Monochrome's (a source module
        // now); when YouTube is the source we still want its best
        // available stream.
        AudioQuality::Lossless
        | AudioQuality::HiResLossless
        | AudioQuality::DolbyAtmos
        | AudioQuality::High => AudioQuality::High,
        AudioQuality::Low | AudioQuality::Medium => conditions.quality,
    }
}

impl Format {
    /// A non-empty `url` field (Kotlin `!it.url.isNullOrEmpty()`).
    pub(crate) fn has_direct_url(&self) -> bool {
        self.url.as_deref().is_some_and(|url| !url.is_empty())
    }

    /// `extractCodec` (YTPlayerUtils.kt:1037-1040): the first codec
    /// of the `codecs="…"` list inside the mime type.
    pub(crate) fn codec(&self) -> Option<&str> {
        let mime = self.mime_type.as_str();
        let start = mime.find("codecs=\"")? + "codecs=\"".len();
        let rest = &mime[start..];
        let end = rest.find('"')?;
        rest[..end].split(',').next().map(str::trim)
    }

    /// `codecRank` (YTPlayerUtils.kt:1042-1047).
    pub(crate) fn codec_rank(&self) -> i64 {
        match self.codec() {
            None => 0,
            Some("") => 0,
            Some(codec) if codec.to_ascii_lowercase().contains("opus") => 3,
            Some(codec) if codec.to_ascii_lowercase().contains("mp4a") => 2,
            Some(_) => 1,
        }
    }

    /// `qualityRank` (YTPlayerUtils.kt:1001-1006): YouTube's own
    /// `AUDIO_QUALITY_*` label outranks raw bitrate — a track can
    /// carry a higher-bitrate stream that YouTube still labels
    /// MEDIUM, and picking it is what made the HIGH setting feel
    /// like it did nothing.
    pub(crate) fn audio_quality_rank(&self) -> i64 {
        match self.audio_quality.as_deref() {
            Some("AUDIO_QUALITY_HIGH") => 3,
            Some("AUDIO_QUALITY_MEDIUM") => 2,
            Some("AUDIO_QUALITY_LOW") => 1,
            _ => 0,
        }
    }

    /// `it.audioChannels ?: 2` (mono streams report 1).
    pub(crate) fn channels_or_default(&self) -> i64 {
        self.audio_channels.unwrap_or(2)
    }

    /// `it.audioSampleRate ?: 0`.
    pub(crate) fn sample_rate_or_zero(&self) -> i64 {
        self.audio_sample_rate.unwrap_or(0)
    }
}

/// The `preferHigher` comparator (YTPlayerUtils.kt:1008-1013):
/// direct URLs first, then YouTube's quality label, channels,
/// bitrate, codec family, sample rate — all descending.
fn prefer_higher(a: &Format, b: &Format) -> std::cmp::Ordering {
    let key = |format: &Format| {
        (
            format.has_direct_url(),
            format.audio_quality_rank(),
            format.channels_or_default(),
            format.bitrate,
            format.codec_rank(),
            format.sample_rate_or_zero(),
        )
    };
    key(b).cmp(&key(a))
}

/// The `preferLowerAboveTarget` comparator
/// (YTPlayerUtils.kt:1015-1018): direct URLs first, then the
/// **cheapest** stream above the ceiling, codec family and sample
/// rate as tiebreaks.
fn prefer_lower_above_target(a: &Format, b: &Format) -> std::cmp::Ordering {
    let key = |format: &Format| {
        (
            format.has_direct_url(),
            std::cmp::Reverse(format.bitrate),
            format.codec_rank(),
            format.sample_rate_or_zero(),
        )
    };
    key(b).cmp(&key(a))
}

/// `selectAudioFormatCandidates` (YTPlayerUtils.kt:957-1035): every
/// usable audio format ordered best-first for the conditions.
///
/// Callers walk this list so that a format whose URL cannot be
/// resolved (ciphered without a working deobfuscator, for example)
/// falls back to the next best one instead of discarding the whole
/// client.
pub fn select_audio_format_candidates(
    player_response: &PlayerResponse,
    conditions: &RankingConditions,
) -> Vec<Format> {
    // audio formats with a bitrate and some form of URL
    // (YTPlayerUtils.kt:964-967)
    let audio_formats: Vec<&Format> = player_response
        .streaming_data
        .as_ref()
        .map(|streaming| streaming.adaptive_formats.iter())
        .unwrap_or_default()
        .filter(|format| format.is_audio() && format.bitrate > 0)
        .filter(|format| {
            format.has_direct_url()
                || format
                    .signature_cipher
                    .as_deref()
                    .is_some_and(|value| !value.is_empty())
                || format
                    .cipher
                    .as_deref()
                    .is_some_and(|value| !value.is_empty())
        })
        .collect();
    if audio_formats.is_empty() {
        return Vec::new();
    }

    // Prefer the original (non auto-dubbed) track when the response
    // offers both (YTPlayerUtils.kt:974-976).
    let original_formats: Vec<&Format> = audio_formats
        .iter()
        .copied()
        .filter(|format| format.is_original())
        .collect();
    let usable: Vec<&Format> = if original_formats.is_empty() {
        audio_formats
    } else {
        original_formats
    };

    let effective = effective_quality(conditions);
    let target_bitrate_bps: Option<i64> = match effective {
        AudioQuality::Low => Some(LOW_CEILING_BPS),
        AudioQuality::Medium => Some(MEDIUM_CEILING_BPS),
        _ => None, // HIGH: take the best available
    };

    let candidates: Vec<Format> = match target_bitrate_bps {
        None => {
            let mut sorted: Vec<Format> = usable.into_iter().cloned().collect();
            sorted.sort_by(prefer_higher);
            sorted
        }
        // Best stream at or below the target, then the cheapest one
        // above it (YTPlayerUtils.kt:1020-1026).
        Some(target) => {
            let mut at_or_below: Vec<Format> = usable
                .iter()
                .filter(|format| format.bitrate <= target)
                .cloned()
                .cloned()
                .collect();
            at_or_below.sort_by(prefer_higher);
            let mut above: Vec<Format> = usable
                .iter()
                .filter(|format| format.bitrate > target)
                .cloned()
                .cloned()
                .collect();
            above.sort_by(prefer_lower_above_target);
            at_or_below.extend(above);
            at_or_below
        }
    };
    for format in &candidates {
        tracing::debug!(
            target: "mochy_stream",
            itag = format.itag,
            bitrate = format.bitrate,
            mime = %format.mime_type,
            direct = format.has_direct_url(),
            effective_quality = ?effective,
            "format candidate"
        );
    }
    candidates
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::response::player::StreamingData;

    /// One audio format shorthand.
    fn audio(itag: i64, bitrate: i64, mime: &str) -> Format {
        Format {
            itag,
            url: Some("https://gonecdf.example/videoplayback?direct=1".into()),
            mime_type: mime.into(),
            bitrate,
            ..Default::default()
        }
    }

    fn with_formats(formats: Vec<Format>) -> PlayerResponse {
        PlayerResponse {
            streaming_data: Some(StreamingData {
                adaptive_formats: formats,
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn unmetered_high() -> RankingConditions {
        RankingConditions {
            quality: AudioQuality::High,
            network_metered: false,
            data_saver: false,
        }
    }

    fn itags(candidates: &[Format]) -> Vec<i64> {
        candidates.iter().map(|format| format.itag).collect()
    }

    /// YouTube's real audio ladder (the itags exist only as a comment
    /// upstream — the ranking must not depend on them).
    fn ladder() -> Vec<Format> {
        vec![
            audio(139, 48_000, "audio/mp4; codecs=\"mp4a.40.5\""),
            audio(249, 50_000, "audio/webm; codecs=\"opus\""),
            audio(250, 70_000, "audio/webm; codecs=\"opus\""),
            audio(140, 128_000, "audio/mp4; codecs=\"mp4a.40.2\""),
            audio(251, 160_000, "audio/webm; codecs=\"opus\""),
        ]
    }

    #[test]
    fn high_takes_the_best_bitrate() {
        let candidates = select_audio_format_candidates(&with_formats(ladder()), &unmetered_high());
        assert_eq!(itags(&candidates), vec![251, 140, 250, 249, 139]);
    }

    #[test]
    fn auto_is_medium_on_metered_and_high_otherwise() {
        let auto = RankingConditions {
            quality: AudioQuality::Auto,
            network_metered: true,
            data_saver: false,
        };
        // MEDIUM's 135k ceiling lands on the 128k AAC rung
        let candidates = select_audio_format_candidates(&with_formats(ladder()), &auto);
        assert_eq!(candidates[0].itag, 140, "metered AUTO picks the 128k rung");
        let auto_unmetered = RankingConditions {
            quality: AudioQuality::Auto,
            network_metered: false,
            data_saver: false,
        };
        let candidates = select_audio_format_candidates(&with_formats(ladder()), &auto_unmetered);
        assert_eq!(candidates[0].itag, 251, "unmetered AUTO picks the best");
    }

    #[test]
    fn medium_ceiling_lands_on_the_128k_aac_then_cheapest_above() {
        let medium = RankingConditions {
            quality: AudioQuality::Medium,
            network_metered: false,
            data_saver: false,
        };
        let candidates = select_audio_format_candidates(&with_formats(ladder()), &medium);
        // at-or-under 135k best-first: 140 (128k), 250 (70k), 249, 139;
        // above the ceiling cheapest-first: 251 (160k)
        assert_eq!(itags(&candidates), vec![140, 250, 249, 139, 251]);
    }

    #[test]
    fn low_ceiling_is_the_70k_rung() {
        let low = RankingConditions {
            quality: AudioQuality::Low,
            network_metered: false,
            data_saver: false,
        };
        let candidates = select_audio_format_candidates(&with_formats(ladder()), &low);
        assert_eq!(candidates[0].itag, 250, "LOW lands on the 70k opus rung");
        // the cheapest above the ceiling follows before the rungs under it
        assert_eq!(itags(&candidates), vec![250, 249, 139, 140, 251]);
    }

    #[test]
    fn data_saver_forces_low_whatever_was_picked() {
        let conditions = RankingConditions {
            quality: AudioQuality::High,
            network_metered: false,
            data_saver: true,
        };
        let candidates = select_audio_format_candidates(&with_formats(ladder()), &conditions);
        assert_eq!(candidates[0].itag, 250);
    }

    #[test]
    fn lossless_tiers_map_to_youtubes_best() {
        for quality in [
            AudioQuality::Lossless,
            AudioQuality::HiResLossless,
            AudioQuality::DolbyAtmos,
        ] {
            let conditions = RankingConditions {
                quality,
                network_metered: false,
                data_saver: false,
            };
            let candidates = select_audio_format_candidates(&with_formats(ladder()), &conditions);
            assert_eq!(candidates[0].itag, 251, "{quality:?} behaves as HIGH");
        }
    }

    #[test]
    fn opus_beats_aac_at_equal_bitrate() {
        let formats = vec![
            audio(140, 128_000, "audio/mp4; codecs=\"mp4a.40.2\""),
            audio(599, 128_000, "audio/webm; codecs=\"opus\""),
        ];
        let candidates = select_audio_format_candidates(&with_formats(formats), &unmetered_high());
        assert_eq!(
            itags(&candidates),
            vec![599, 140],
            "codec rank breaks the bitrate tie"
        );
    }

    #[test]
    fn sample_rate_is_the_final_tiebreak() {
        let mut low_rate = audio(600, 128_000, "audio/webm; codecs=\"opus\"");
        low_rate.audio_sample_rate = Some(44_100);
        let mut high_rate = audio(601, 128_000, "audio/webm; codecs=\"opus\"");
        high_rate.audio_sample_rate = Some(48_000);
        let candidates = select_audio_format_candidates(
            &with_formats(vec![low_rate, high_rate.clone()]),
            &unmetered_high(),
        );
        assert_eq!(
            candidates[0].itag, 601,
            "48 kHz beats 44.1 kHz at equal rank"
        );
    }

    #[test]
    fn youtubes_quality_label_outranks_raw_bitrate() {
        let mut big_unlabeled = audio(140, 160_000, "audio/mp4; codecs=\"mp4a.40.2\"");
        big_unlabeled.audio_quality = None;
        let mut small_high = audio(251, 128_000, "audio/webm; codecs=\"opus\"");
        small_high.audio_quality = Some("AUDIO_QUALITY_HIGH".into());
        let candidates = select_audio_format_candidates(
            &with_formats(vec![big_unlabeled, small_high]),
            &unmetered_high(),
        );
        assert_eq!(
            candidates[0].itag, 251,
            "the AUDIO_QUALITY_HIGH label beats a higher unlabeled bitrate"
        );
    }

    #[test]
    fn direct_urls_beat_ciphered_ones() {
        let mut ciphered = audio(251, 160_000, "audio/webm; codecs=\"opus\"");
        ciphered.url = None;
        ciphered.signature_cipher =
            Some("s=abcdef&sp=sig&url=https%3A%2F%2Fgonecdf.example%2Fvideoplayback".into());
        let direct = audio(140, 128_000, "audio/mp4; codecs=\"mp4a.40.2\"");
        let candidates = select_audio_format_candidates(
            &with_formats(vec![ciphered, direct]),
            &unmetered_high(),
        );
        assert_eq!(
            candidates[0].itag, 140,
            "a resolvable direct URL wins the first pick"
        );
    }

    #[test]
    fn original_tracks_beat_auto_dubbed_ones() {
        let mut dubbed = ladder()[4].clone();
        dubbed.audio_track = Some(crate::models::response::player::AudioTrack {
            display_name: Some("dubbed".into()),
            id: None,
            is_auto_dubbed: Some(true),
        });
        let original = ladder()[3].clone();
        let candidates = select_audio_format_candidates(
            &with_formats(vec![dubbed, original.clone()]),
            &unmetered_high(),
        );
        assert_eq!(candidates[0].itag, original.itag);
        assert!(
            candidates.iter().all(|format| format.is_original()),
            "the auto-dubbed track is dropped entirely when originals exist"
        );
    }

    #[test]
    fn stereo_beats_mono_before_bitrate() {
        let mut mono = audio(700, 160_000, "audio/webm; codecs=\"opus\"");
        mono.audio_channels = Some(1);
        let stereo = audio(701, 128_000, "audio/webm; codecs=\"opus\"");
        let candidates =
            select_audio_format_candidates(&with_formats(vec![mono, stereo]), &unmetered_high());
        assert_eq!(candidates[0].itag, 701, "channels rank above bitrate");
    }

    #[test]
    fn video_and_silent_formats_are_not_candidates() {
        let mut video = audio(137, 2_000_000, "video/mp4; codecs=\"avc1.640028\"");
        video.width = Some(1920);
        video.height = Some(1080);
        let mut silent = audio(999, 128_000, "audio/mp4; codecs=\"mp4a.40.2\"");
        silent.url = None;
        silent.signature_cipher = None;
        let candidates = select_audio_format_candidates(
            &with_formats(vec![video, silent, ladder()[3].clone()]),
            &unmetered_high(),
        );
        assert_eq!(itags(&candidates), vec![140]);
    }

    #[test]
    fn sparse_ladder_still_answers_low() {
        // only the 128k rung exists: LOW must still fall back to it
        let conditions = RankingConditions {
            quality: AudioQuality::Low,
            network_metered: false,
            data_saver: false,
        };
        let candidates =
            select_audio_format_candidates(&with_formats(vec![ladder()[3].clone()]), &conditions);
        assert_eq!(itags(&candidates), vec![140]);
    }

    #[test]
    fn empty_response_yields_no_candidates() {
        assert!(
            select_audio_format_candidates(&PlayerResponse::default(), &unmetered_high())
                .is_empty()
        );
    }

    #[test]
    fn setting_names_round_trip() {
        for quality in [
            AudioQuality::Auto,
            AudioQuality::Low,
            AudioQuality::Medium,
            AudioQuality::High,
            AudioQuality::Lossless,
            AudioQuality::HiResLossless,
            AudioQuality::DolbyAtmos,
        ] {
            assert_eq!(
                AudioQuality::from_setting_str(quality.as_setting_str()),
                Some(quality)
            );
        }
        assert_eq!(AudioQuality::from_setting_str("MADE_UP"), None);
    }
}
