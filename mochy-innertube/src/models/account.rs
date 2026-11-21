//! `models/AccountInfo.kt`, `models/MediaInfo.kt`,
//! `models/ReturnYouTubeDislikeResponse.kt`, `models/SearchSuggestions.kt`,
//! `Captions.kt`'s `CaptionCue`/`VideoCaptions` — the non-renderer domain
//! models.

use serde::{Deserialize, Serialize};

use crate::client::YouTubeClient;

/// `AccountInfo.kt`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountInfo {
    pub name: String,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub channel_handle: Option<String>,
    #[serde(default)]
    pub thumbnail_url: Option<String>,
}

/// `MediaInfo.kt` — the composite the song-info sheet shows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaInfo {
    pub video_id: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub author_id: Option<String>,
    #[serde(default)]
    pub author_thumbnail: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub upload_date: Option<String>,
    #[serde(default)]
    pub subscribers: Option<String>,
    #[serde(default)]
    pub view_count: Option<i64>,
    #[serde(default)]
    pub like: Option<i64>,
    #[serde(default)]
    pub dislike: Option<i64>,
}

/// `ReturnYouTubeDislikeResponse.kt`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ReturnYouTubeDislikeResponse {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub date_created: Option<String>,
    #[serde(default)]
    pub likes: Option<i64>,
    #[serde(default)]
    pub dislikes: Option<i64>,
    #[serde(default)]
    pub rating: Option<f64>,
    #[serde(default)]
    pub view_count: Option<i64>,
    #[serde(default)]
    pub deleted: Option<bool>,
}

/// `SearchSuggestions.kt` — typed queries plus recommended items.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchSuggestions {
    pub queries: Vec<String>,
    pub recommended_items: Vec<crate::models::ytitem::YTItem>,
}

/// `Captions.kt:46` — one caption piece timed on the video: a whole line
/// for uploaded tracks, a word or two for auto-generated ones.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptionCue {
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

/// `Captions.kt:49-53` — a video's caption tracks, along with the client
/// whose links can download them.
#[derive(Debug, Clone)]
pub struct VideoCaptions {
    pub video_id: String,
    pub tracks: Vec<crate::models::response::CaptionTrack>,
    pub client: YouTubeClient,
}
