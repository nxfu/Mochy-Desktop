//! The InnerTubeX-equivalent alternate backend seam
//! (`utils/InnerTubeXStreams.kt`, behind the `innertubex` cargo
//! feature).
//!
//! Upstream's `com.metrolist.innertubex` is a full second streaming
//! engine (its own client walk, cipher service, extractor and token
//! plumbing) asked BEFORE mochy's own walk, with the walk as its
//! backup — selected by `StreamBackend.INNERTUBEX` and compiled in
//! lazily so a phone on the classic backend never pays for it.
//!
//! This module ports the **seam**: the backend selection
//! (`innerTubeXFirst`), the session mirroring, the quality-ladder
//! translation, the 12-second give-up budget and the
//! `ExtractedStream` → [`PlaybackData`] conversion. The engine itself
//! — a from-scratch Rust re-implementation of the innertubex
//! extractor — is future work recorded in HANDOFF; until it lands the
//! seam answers `None` and the walk (fully ported) serves every
//! request, exactly like upstream's `StreamBackend.CLASSIC`.
//!
//! The documented alternative engine plan (deno_core snapshotting,
//! the rustypipe-botguard approach) is NOT used — see
//! [`super::potoken`] for why the port runs BotGuard in QuickJS.

use crate::youtube::YouTube;

use super::playback::{PlaybackData, PlaybackRequest};
use super::ranking::{AudioQuality, RankingConditions};

/// How long the alternate engine may hold a song before the walk
/// takes over (`GiveUpMs`, InnerTubeXStreams.kt:238) — past this a
/// stuck lookup never holds a song for innertubex's own 30 s budget.
pub const GIVE_UP_MS: u64 = 12_000;

/// Links without an expiry are refetched after five minutes, like
/// innertubex's own default (`DefaultLinkLifeSeconds`,
/// InnerTubeXStreams.kt:240).
pub const DEFAULT_LINK_LIFE_SECONDS: i32 = 5 * 60;

/// `ContentHints` (InnerTubeXStreams.kt:120-122): only plain links
/// the player can read straight through — no hls or sabr, and none
/// that need small byte ranges.
#[derive(Debug, Clone, Copy, Default)]
pub struct ContentHints {
    pub is_uploaded: bool,
    pub allow_hls: bool,
    pub allow_sabr: bool,
    pub allow_bounded_range: bool,
}

impl ContentHints {
    /// The playback request's hints: uploads travel through the
    /// `MLPT` playlist marker, and the port's player consumes plain
    /// progressive urls only.
    pub fn for_request(request: &PlaybackRequest) -> Self {
        ContentHints {
            is_uploaded: request.is_uploaded_track(),
            allow_hls: false,
            allow_sabr: false,
            allow_bounded_range: false,
        }
    }
}

/// The quality ladder in innertubex's words
/// (InnerTubeXStreams.kt:162-171): medium is the 128k AAC rung, the
/// lossless tiers belong to the Monochrome module.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineQuality {
    Low,
    /// The 128k AAC rung.
    Mp4,
    High,
}

/// `AudioQuality.toInnerTubeX` (InnerTubeXStreams.kt:162-171).
pub fn engine_quality(conditions: &RankingConditions) -> EngineQuality {
    match conditions {
        RankingConditions {
            data_saver: true, ..
        } => EngineQuality::Low,
        RankingConditions {
            quality: AudioQuality::Low,
            ..
        } => EngineQuality::Low,
        RankingConditions {
            quality: AudioQuality::Medium,
            ..
        } => EngineQuality::Mp4,
        RankingConditions {
            quality: AudioQuality::Auto,
            network_metered: true,
            ..
        } => EngineQuality::Mp4,
        _ => EngineQuality::High,
    }
}

/// The mirrored session the engine last saw
/// (InnerTubeXStreams.kt:44-51): copied over only when something
/// changed, since every change starts the engine's per-session
/// caches again.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MirroredSession {
    pub cookie: Option<String>,
    pub visitor_data: Option<String>,
    pub data_sync_id: Option<String>,
    pub gl: String,
    pub hl: String,
    pub login_for_browse: bool,
}

impl MirroredSession {
    /// Read the current session out of the transport
    /// (`mirrorSession`, InnerTubeXStreams.kt:137-159).
    pub fn current(youtube: &YouTube) -> Self {
        let guard = youtube
            .transport()
            .session()
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        MirroredSession {
            cookie: guard.cookie.clone(),
            visitor_data: guard.visitor_data.clone(),
            data_sync_id: guard.data_sync_id.clone(),
            gl: guard.locale.gl.clone(),
            hl: guard.locale.hl.clone(),
            login_for_browse: guard.use_login_for_browse,
        }
    }
}

/// The alternate engine's answer for one video, in the port's
/// playback shape (`ExtractedStream.toPlaybackData`,
/// InnerTubeXStreams.kt:173-234).
#[derive(Debug, Clone)]
pub struct ExtractedStream {
    pub video_id: String,
    /// The audio url — blank means the engine found nothing the
    /// player can consume.
    pub audio_url: String,
    pub itag: i64,
    pub mime_type: String,
    pub codecs: Option<String>,
    pub bitrate: Option<i64>,
    pub content_length_bytes: Option<i64>,
    pub sample_rate: Option<i64>,
    pub loudness_db: Option<f64>,
    pub perceptual_loudness_db: Option<f64>,
    /// `None` → [`DEFAULT_LINK_LIFE_SECONDS`].
    pub expires_in_seconds: Option<i32>,
    /// The client label for the diagnostics page ("InnerTubeX · …").
    pub client_name: String,
    pub headers: Vec<(String, String)>,
    /// A sabr bootstrap means the engine went down a path the port's
    /// player cannot consume — the walk takes over.
    pub sabr_bootstrap: bool,
}

impl ExtractedStream {
    /// `toPlaybackData` (InnerTubeXStreams.kt:173-234): the engine's
    /// answer becomes the walk's answer, with the client labelled so
    /// the diagnostics page and the source banner show which backend
    /// found it.
    pub fn to_playback_data(&self) -> Option<PlaybackData> {
        if self.sabr_bootstrap || self.audio_url.is_empty() {
            return None;
        }
        let full_mime_type = match &self.codecs {
            Some(codecs) if !codecs.is_empty() => {
                format!("{}; codecs=\"{codecs}\"", self.mime_type)
            }
            _ => self.mime_type.clone(),
        };
        let expires = self
            .expires_in_seconds
            .unwrap_or(DEFAULT_LINK_LIFE_SECONDS)
            .max(1);
        let format = crate::models::response::player::Format {
            itag: self.itag,
            url: Some(self.audio_url.clone()),
            mime_type: full_mime_type,
            bitrate: self.bitrate.unwrap_or(0),
            content_length: self.content_length_bytes,
            audio_sample_rate: self.sample_rate,
            loudness_db: self.loudness_db,
            average_bitrate: self.bitrate,
            ..Default::default()
        };
        let audio_config = (self.loudness_db.is_some() || self.perceptual_loudness_db.is_some())
            .then_some(crate::models::response::player::AudioConfig {
                loudness_db: self.loudness_db,
                perceptual_loudness_db: self.perceptual_loudness_db,
            });
        Some(PlaybackData::from_parts(
            audio_config,
            None,
            None,
            format,
            self.audio_url.clone(),
            expires,
            &format!("InnerTubeX · {}", self.client_name),
            self.headers.clone(),
        ))
    }
}

/// The engine contract the seam calls when the feature is on. The
/// future innertubex port implements this; the default
/// implementation answers "nothing found" so the walk serves every
/// request. The futures are boxed: the seam holds the engine as a
/// `dyn` trait object, and `impl Future` returns are not dyn-safe.
pub trait AlternateStreamEngine: Send + Sync {
    /// Fetch and compile the player script ahead of the first song
    /// (`prewarm`, InnerTubeXStreams.kt:104-109).
    fn prewarm(&self) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
        Box::pin(std::future::ready(()))
    }

    /// A playable stream for `video_id`, or `None` when the engine
    /// has none and the walk should try (`resolve`,
    /// InnerTubeXStreams.kt:112-134). A library that won't even load
    /// counts as a miss, not a crash.
    fn resolve(
        &self,
        youtube: &YouTube,
        request: &PlaybackRequest,
        hints: &ContentHints,
        quality: EngineQuality,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Option<ExtractedStream>> + Send + '_>>
    {
        let _ = (youtube, request, hints, quality);
        Box::pin(std::future::ready(None))
    }
}

/// The no-op engine the seam runs until the real port lands: every
/// resolve is a miss, so the walk takes over.
#[derive(Debug, Default)]
pub struct UnimplementedEngine;

impl AlternateStreamEngine for UnimplementedEngine {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quality_ladder_translates_the_conditions() {
        let conditions = |quality, metered, saver| RankingConditions {
            quality,
            network_metered: metered,
            data_saver: saver,
        };
        assert_eq!(
            engine_quality(&conditions(AudioQuality::High, false, true)),
            EngineQuality::Low,
            "data saver keeps youtube on its lightest stream"
        );
        assert_eq!(
            engine_quality(&conditions(AudioQuality::Low, false, false)),
            EngineQuality::Low
        );
        assert_eq!(
            engine_quality(&conditions(AudioQuality::Medium, true, false)),
            EngineQuality::Mp4
        );
        assert_eq!(
            engine_quality(&conditions(AudioQuality::Auto, true, false)),
            EngineQuality::Mp4
        );
        assert_eq!(
            engine_quality(&conditions(AudioQuality::Auto, false, false)),
            EngineQuality::High
        );
        assert_eq!(
            engine_quality(&conditions(AudioQuality::Lossless, false, false)),
            EngineQuality::High,
            "the lossless tiers belong to modules"
        );
    }

    #[tokio::test]
    async fn extracted_streams_become_playback_data() {
        let stream = ExtractedStream {
            video_id: "abc".into(),
            audio_url: "https://gonecdf/v?itag=140".into(),
            itag: 140,
            mime_type: "audio/mp4".into(),
            codecs: Some("mp4a.40.2".into()),
            bitrate: Some(128_000),
            content_length_bytes: Some(2_000_000),
            sample_rate: Some(44_100),
            loudness_db: Some(-3.5),
            perceptual_loudness_db: None,
            expires_in_seconds: None,
            client_name: "WEB".into(),
            headers: Vec::new(),
            sabr_bootstrap: false,
        };
        let data = stream.to_playback_data().expect("consumable");
        assert_eq!(data.stream_client, "InnerTubeX · WEB");
        assert_eq!(data.format.mime_type, "audio/mp4; codecs=\"mp4a.40.2\"");
        assert_eq!(data.stream_expires_in_seconds, DEFAULT_LINK_LIFE_SECONDS);
        assert_eq!(data.audio_config.unwrap().loudness_db, Some(-3.5));
        // sabr bootstrap and blank urls are misses, not data
        let mut sabr = stream.clone();
        sabr.sabr_bootstrap = true;
        assert!(sabr.to_playback_data().is_none());
        let mut blank = stream;
        blank.audio_url = String::new();
        assert!(blank.to_playback_data().is_none());
    }

    #[tokio::test]
    async fn the_unimplemented_engine_misses_everything() {
        let engine = UnimplementedEngine;
        assert!(
            engine
                .resolve(
                    &YouTube::new(),
                    &PlaybackRequest::new(
                        "x",
                        RankingConditions {
                            quality: AudioQuality::High,
                            network_metered: false,
                            data_saver: false,
                        }
                    ),
                    &ContentHints::default(),
                    EngineQuality::High
                )
                .await
                .is_none()
        );
    }

    #[test]
    fn hints_mark_uploads_and_plain_links_only() {
        let request = PlaybackRequest::new(
            "abc",
            RankingConditions {
                quality: AudioQuality::Auto,
                network_metered: false,
                data_saver: false,
            },
        )
        .with_playlist(Some("MLPT".into()));
        let hints = ContentHints::for_request(&request);
        assert!(hints.is_uploaded);
        assert!(!hints.allow_hls && !hints.allow_sabr && !hints.allow_bounded_range);
    }
}
