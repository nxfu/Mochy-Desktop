//! The small response envelopes: `models/response/AccountMenuResponse.kt`,
//! `GetTranscriptResponse.kt`, `GetSearchSuggestionsResponse.kt` (with
//! `models/SearchSuggestionsSectionRenderer.kt`), `GetQueueResponse.kt`,
//! `ContinuationResponse.kt`, `CreatePlaylistResponse.kt`,
//! `EditPlaylistResponse.kt`, `FeedbackResponse.kt`,
//! `ImageUploadResponse.kt`.

use serde::{Deserialize, Serialize};

use crate::models::music_renderers::MusicResponsiveListItemRenderer;
use crate::models::music_renderers::MusicShelfContent;
use crate::models::playlist_panel::PlaylistPanelContent;
use crate::models::response::browse;
use crate::models::runs::Runs;
use crate::models::thumbnails::Thumbnails;

// ---------------------------------------------------------------------------
// AccountMenuResponse.kt
// ---------------------------------------------------------------------------

/// `AccountMenuResponse.kt:9-11`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AccountMenuResponse {
    #[serde(default)]
    pub actions: Vec<Action>,
}

/// AccountMenuResponse.kt:13-15.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Action {
    #[serde(default)]
    pub open_popup_action: Option<OpenPopupAction>,
}

/// AccountMenuResponse.kt:17-19.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct OpenPopupAction {
    #[serde(default)]
    pub popup: Option<Popup>,
}

/// AccountMenuResponse.kt:21-23.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Popup {
    #[serde(default)]
    pub multi_page_menu_renderer: Option<MultiPageMenuRenderer>,
}

/// AccountMenuResponse.kt:25-27.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MultiPageMenuRenderer {
    #[serde(default)]
    pub header: Option<MenuHeader>,
}

/// AccountMenuResponse.kt:29-31.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MenuHeader {
    #[serde(default)]
    pub active_account_header_renderer: Option<ActiveAccountHeaderRenderer>,
}

/// AccountMenuResponse.kt:33-47 — with `toAccountInfo()`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveAccountHeaderRenderer {
    pub account_name: Runs,
    #[serde(default)]
    pub email: Option<Runs>,
    #[serde(default)]
    pub channel_handle: Option<Runs>,
    pub account_photo: Thumbnails,
}

impl ActiveAccountHeaderRenderer {
    /// `toAccountInfo()` (AccountMenuResponse.kt:39-47).
    pub fn to_account_info(&self) -> crate::models::account::AccountInfo {
        crate::models::account::AccountInfo {
            name: self
                .account_name
                .first()
                .map(|run| run.text.clone())
                .unwrap_or_default(),
            email: self
                .email
                .as_ref()
                .and_then(|runs| runs.first())
                .map(|r| r.text.clone()),
            channel_handle: self
                .channel_handle
                .as_ref()
                .and_then(|runs| runs.first())
                .map(|r| r.text.clone()),
            thumbnail_url: self
                .account_photo
                .thumbnails
                .last()
                .map(|thumb| thumb.url.clone())
                .map(|url| {
                    if url.starts_with("//") {
                        format!("https:{url}")
                    } else {
                        url
                    }
                }),
        }
    }
}

// ---------------------------------------------------------------------------
// GetTranscriptResponse.kt
// ---------------------------------------------------------------------------

/// `GetTranscriptResponse.kt:6-8`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GetTranscriptResponse {
    #[serde(default)]
    pub actions: Option<Vec<Action2>>,
}

/// GetTranscriptResponse.kt:10-12 (the transcript action).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Action2 {
    #[serde(default)]
    pub update_engagement_panel_action: Option<UpdateEngagementPanelAction>,
}

/// GetTranscriptResponse.kt:14-16.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UpdateEngagementPanelAction {
    #[serde(default)]
    pub content: Option<Content2>,
}

/// GetTranscriptResponse.kt:18-20.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Content2 {
    #[serde(default)]
    pub transcript_renderer: Option<TranscriptRenderer>,
}

/// GetTranscriptResponse.kt:22-24.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptRenderer {
    #[serde(default)]
    pub body: Option<Body>,
}

/// GetTranscriptResponse.kt:26-28.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Body {
    #[serde(default)]
    pub transcript_body_renderer: Option<TranscriptBodyRenderer>,
}

/// GetTranscriptResponse.kt:30-32.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptBodyRenderer {
    #[serde(default)]
    pub cue_groups: Vec<CueGroup>,
}

/// GetTranscriptResponse.kt:34-36.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CueGroup {
    #[serde(default)]
    pub transcript_cue_group_renderer: Option<TranscriptCueGroupRenderer>,
}

/// GetTranscriptResponse.kt:38-40.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptCueGroupRenderer {
    #[serde(default)]
    pub cues: Vec<Cue>,
}

/// GetTranscriptResponse.kt:42-44.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Cue {
    #[serde(default)]
    pub transcript_cue_renderer: Option<TranscriptCueRenderer>,
}

/// GetTranscriptResponse.kt:46-55.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptCueRenderer {
    #[serde(default)]
    pub cue: Option<SimpleText>,
    #[serde(default)]
    pub start_offset_ms: i64,
    #[serde(default)]
    pub duration_ms: i64,
}

/// GetTranscriptResponse.kt:52-54.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SimpleText {
    #[serde(default)]
    pub simple_text: String,
}

// ---------------------------------------------------------------------------
// GetSearchSuggestionsResponse.kt + models/SearchSuggestionsSectionRenderer.kt
// ---------------------------------------------------------------------------

/// `GetSearchSuggestionsResponse.kt:7-9`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GetSearchSuggestionsResponse {
    #[serde(default)]
    pub contents: Option<Vec<Content3>>,
}

/// GetSearchSuggestionsResponse.kt:11-13.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Content3 {
    #[serde(default)]
    pub search_suggestions_section_renderer: Option<SearchSuggestionsSectionRenderer>,
}

/// `SearchSuggestionsSectionRenderer.kt:6-8`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SearchSuggestionsSectionRenderer {
    #[serde(default)]
    pub contents: Vec<SuggestionContent>,
}

/// SearchSuggestionsSectionRenderer.kt:10-13.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SuggestionContent {
    #[serde(default)]
    pub search_suggestion_renderer: Option<SearchSuggestionRenderer>,
    #[serde(default)]
    pub music_responsive_list_item_renderer: Option<MusicResponsiveListItemRenderer>,
}

/// SearchSuggestionsSectionRenderer.kt:14-18.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchSuggestionRenderer {
    pub suggestion: Runs,
    pub navigation_endpoint: crate::models::endpoint::NavigationEndpoint,
}

// ---------------------------------------------------------------------------
// GetQueueResponse.kt
// ---------------------------------------------------------------------------

/// `GetQueueResponse.kt:7-9`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GetQueueResponse {
    #[serde(default)]
    pub queue_datas: Vec<QueueData>,
}

/// GetQueueResponse.kt:11-13.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct QueueData {
    #[serde(default)]
    pub content: Option<PlaylistPanelContent>,
}

// ---------------------------------------------------------------------------
// ContinuationResponse.kt
// ---------------------------------------------------------------------------

/// `ContinuationResponse.kt:7-9`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ContinuationResponse {
    #[serde(default)]
    pub on_response_received_actions: Option<Vec<ResponseAction2>>,
}

/// ContinuationResponse.kt:11-14.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ResponseAction2 {
    #[serde(default)]
    pub append_continuation_items_action: Option<ContinuationItems2>,
}

/// ContinuationResponse.kt:16-19.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ContinuationItems2 {
    #[serde(default)]
    pub continuation_items: Option<Vec<MusicShelfContent>>,
}

// ---------------------------------------------------------------------------
// CreatePlaylistResponse.kt / EditPlaylistResponse.kt / FeedbackResponse.kt
// / ImageUploadResponse.kt
// ---------------------------------------------------------------------------

/// `CreatePlaylistResponse.kt`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CreatePlaylistResponse {
    #[serde(default)]
    pub playlist_id: String,
}

/// `EditPlaylistResponse.kt`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct EditPlaylistResponse {
    #[serde(default)]
    pub new_header: Option<browse::Header>,
}

/// `FeedbackResponse.kt`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FeedbackResponse {
    #[serde(default)]
    pub feedback_responses: Vec<Status>,
}

/// FeedbackResponse.kt:10-12.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    #[serde(default)]
    pub is_processed: bool,
}

/// `ImageUploadResponse.kt`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ImageUploadResponse {
    #[serde(default)]
    pub encrypted_blob_id: String,
}
