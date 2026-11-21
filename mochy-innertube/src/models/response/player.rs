//! `models/response/PlayerResponse.kt` (147 lines).

use serde::{Deserialize, Serialize};

use crate::models::common::ResponseContext;
use crate::models::thumbnails::Thumbnails;

/// `PlayerResponse.kt:9-18`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PlayerResponse {
    #[serde(default)]
    pub response_context: Option<ResponseContext>,
    #[serde(default)]
    pub playability_status: Option<PlayabilityStatus>,
    #[serde(default)]
    pub player_config: Option<PlayerConfig>,
    #[serde(default)]
    pub streaming_data: Option<StreamingData>,
    #[serde(default)]
    pub video_details: Option<VideoDetails>,
    #[serde(default)]
    pub playback_tracking: Option<PlaybackTracking>,
    #[serde(default)]
    pub captions: Option<Captions>,
}

/// `PlayerResponse.kt:20-23`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PlayabilityStatus {
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub reason: Option<String>,
}

/// `PlayerResponse.kt:25-34`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerConfig {
    pub audio_config: AudioConfig,
}

/// `PlayerResponse.kt:29-33`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AudioConfig {
    #[serde(default)]
    pub loudness_db: Option<f64>,
    #[serde(default)]
    pub perceptual_loudness_db: Option<f64>,
}

/// `PlayerResponse.kt:36-86`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct StreamingData {
    #[serde(default)]
    pub formats: Option<Vec<Format>>,
    #[serde(default)]
    pub adaptive_formats: Vec<Format>,
    #[serde(default)]
    pub expires_in_seconds: i64,
}

/// `PlayerResponse.kt:42-66` — one stream format.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Format {
    #[serde(default)]
    pub itag: i64,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub mime_type: String,
    #[serde(default)]
    pub bitrate: i64,
    #[serde(default)]
    pub width: Option<i64>,
    #[serde(default)]
    pub height: Option<i64>,
    #[serde(default)]
    pub content_length: Option<i64>,
    #[serde(default)]
    pub quality: String,
    #[serde(default)]
    pub fps: Option<i64>,
    #[serde(default)]
    pub quality_label: Option<String>,
    #[serde(default)]
    pub average_bitrate: Option<i64>,
    #[serde(default)]
    pub audio_quality: Option<String>,
    #[serde(default)]
    pub approx_duration_ms: Option<String>,
    #[serde(default)]
    pub audio_sample_rate: Option<i64>,
    #[serde(default)]
    pub audio_channels: Option<i64>,
    #[serde(default)]
    pub loudness_db: Option<f64>,
    #[serde(default)]
    pub last_modified: Option<i64>,
    #[serde(default)]
    pub signature_cipher: Option<String>,
    #[serde(default)]
    pub cipher: Option<String>,
    #[serde(default)]
    pub audio_track: Option<AudioTrack>,
    /// Where the header and the chunk index sit in the file, so a player
    /// can fetch it in small pieces and switch quality
    /// (PlayerResponse.kt:64-66).
    #[serde(default)]
    pub init_range: Option<ByteRange>,
    #[serde(default)]
    pub index_range: Option<ByteRange>,
}

impl Format {
    /// `isAudio` (PlayerResponse.kt:68-69) — no width, no picture.
    pub fn is_audio(&self) -> bool {
        self.width.is_none()
    }

    /// `isOriginal` (PlayerResponse.kt:70-71) — not an auto-dubbed track.
    pub fn is_original(&self) -> bool {
        self.audio_track
            .as_ref()
            .map(|track| track.is_auto_dubbed.is_none())
            .unwrap_or(true)
    }
}

/// `PlayerResponse.kt:73-77`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ByteRange {
    pub start: String,
    pub end: String,
}

/// `PlayerResponse.kt:79-84`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AudioTrack {
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub is_auto_dubbed: Option<bool>,
}

/// `PlayerResponse.kt:88-98`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct VideoDetails {
    #[serde(default)]
    pub video_id: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub channel_id: String,
    #[serde(default)]
    pub length_seconds: String,
    #[serde(default)]
    pub music_video_type: Option<String>,
    #[serde(default)]
    pub view_count: Option<String>,
    #[serde(default)]
    pub thumbnail: Option<Thumbnails>,
}

/// The video's subtitle tracks: ones uploaded for it, and youtube's
/// speech recognition (`PlayerResponse.kt:100-120`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Captions {
    #[serde(default)]
    pub player_captions_tracklist_renderer: Option<PlayerCaptionsTracklistRenderer>,
}

/// `PlayerResponse.kt:103-108`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PlayerCaptionsTracklistRenderer {
    #[serde(default)]
    pub caption_tracks: Option<Vec<CaptionTrack>>,
}

/// `PlayerResponse.kt:110-119`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptionTrack {
    pub base_url: String,
    #[serde(default)]
    pub language_code: Option<String>,
    /// "asr" for auto-generated captions, absent for uploaded ones.
    #[serde(default)]
    pub kind: Option<String>,
}

impl CaptionTrack {
    /// `isAutoGenerated` (PlayerResponse.kt:117-118).
    pub fn is_auto_generated(&self) -> bool {
        self.kind.as_deref() == Some("asr")
    }
}

/// `PlayerResponse.kt:122-146` — the play/watchtime/atr tracking urls.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackTracking {
    #[serde(default)]
    pub videostats_playback_url: Option<TrackingUrl>,
    #[serde(default)]
    pub videostats_watchtime_url: Option<TrackingUrl>,
    #[serde(default)]
    pub atr_url: Option<TrackingUrl>,
}

/// The `baseUrl` holder of all three tracking urls (PlayerResponse.kt:
/// 131-145 — three identical one-field classes upstream).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TrackingUrl {
    #[serde(default)]
    pub base_url: Option<String>,
}
