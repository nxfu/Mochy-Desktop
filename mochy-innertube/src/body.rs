//! Request bodies — the `providers/innertube/.../models/body/` package
//! (pin `bde01cb4`), one Rust struct per Kotlin `@Serializable` data class.
//!
//! Serialization must match kotlinx-serialization with the app's Json
//! config (InnerTube.kt:80-84): `explicitNulls = false` — nullable fields
//! at `null` are OMITTED from the wire object; `encodeDefaults = true` —
//! defaults ARE emitted. The `#[serde(skip_serializing_if = "Option::is_none")]`
//! annotations reproduce the first rule for every nullable field.

use serde::{Deserialize, Serialize};

use crate::context::Context;

fn is_none<T>(option: &Option<T>) -> bool {
    option.is_none()
}

/// `body/SearchBody.kt`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchBody {
    pub context: Context,
    #[serde(skip_serializing_if = "is_none")]
    pub query: Option<String>,
    #[serde(skip_serializing_if = "is_none")]
    pub params: Option<String>,
}

/// `body/BrowseBody.kt`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowseBody {
    pub context: Context,
    #[serde(skip_serializing_if = "is_none")]
    pub browse_id: Option<String>,
    #[serde(skip_serializing_if = "is_none")]
    pub params: Option<String>,
    #[serde(skip_serializing_if = "is_none")]
    pub continuation: Option<String>,
}

/// `body/PlayerBody.kt`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerBody {
    pub context: Context,
    pub video_id: String,
    #[serde(skip_serializing_if = "is_none")]
    pub playlist_id: Option<String>,
    #[serde(skip_serializing_if = "is_none")]
    pub playback_context: Option<PlaybackContext>,
    #[serde(skip_serializing_if = "is_none")]
    pub service_integrity_dimensions: Option<ServiceIntegrityDimensions>,
    #[serde(rename = "contentCheckOk", default = "default_true")]
    pub content_check_ok: bool,
    #[serde(rename = "racyCheckOk", default = "default_true")]
    pub racy_check_ok: bool,
}

fn default_true() -> bool {
    true
}

/// `PlayerBody.PlaybackContext` (body/PlayerBody.kt:11-24).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackContext {
    pub content_playback_context: ContentPlaybackContext,
}

/// `PlayerBody.PlaybackContext.ContentPlaybackContext`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentPlaybackContext {
    pub signature_timestamp: i32,
    /// the web and tv players ask for html5 streams, and the older tv
    /// app's answer can come without links otherwise (PlayerBody.kt:19-20)
    #[serde(rename = "html5Preference", default)]
    pub html5_preference: Option<String>,
}

impl ContentPlaybackContext {
    /// Kotlin default: `"HTML5_PREF_WANTS"` (always emitted —
    /// `encodeDefaults = true`).
    pub fn new(signature_timestamp: i32) -> Self {
        ContentPlaybackContext {
            signature_timestamp,
            html5_preference: Some("HTML5_PREF_WANTS".into()),
        }
    }
}

/// `PlayerBody.ServiceIntegrityDimensions` (body/PlayerBody.kt:26-29).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceIntegrityDimensions {
    pub po_token: String,
}

/// `body/NextBody.kt`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NextBody {
    pub context: Context,
    #[serde(skip_serializing_if = "is_none")]
    pub video_id: Option<String>,
    #[serde(skip_serializing_if = "is_none")]
    pub playlist_id: Option<String>,
    #[serde(skip_serializing_if = "is_none")]
    pub playlist_set_video_id: Option<String>,
    #[serde(skip_serializing_if = "is_none")]
    pub index: Option<i32>,
    #[serde(skip_serializing_if = "is_none")]
    pub params: Option<String>,
    #[serde(skip_serializing_if = "is_none")]
    pub continuation: Option<String>,
}

/// `body/FeedbackBody.kt` (feedbackTokens carry the like/library actions
/// a `next` response hands out).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedbackBody {
    pub context: Context,
    pub feedback_tokens: Vec<String>,
    #[serde(rename = "isFeedbackTokenUnencrypted", default)]
    pub is_feedback_token_unencrypted: bool,
    #[serde(default)]
    pub should_merge: bool,
}

/// `body/GetSearchSuggestionsBody.kt`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetSearchSuggestionsBody {
    pub context: Context,
    pub input: String,
}

/// `body/GetQueueBody.kt`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetQueueBody {
    pub context: Context,
    #[serde(skip_serializing_if = "is_none")]
    pub video_ids: Option<Vec<String>>,
    #[serde(skip_serializing_if = "is_none")]
    pub playlist_id: Option<String>,
}

/// `body/GetTranscriptBody.kt` — `params` is the base64 envelope built by
/// [`crate::endpoints::transcript_params`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetTranscriptBody {
    pub context: Context,
    pub params: String,
}

/// `body/VisitorIdBody.kt` — the whole body of a `visitor_id` mint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisitorIdBody {
    pub context: Context,
}

/// `body/AccountMenuBody.kt` (theme fields always emitted — Kotlin
/// defaults with `encodeDefaults = true`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountMenuBody {
    pub context: Context,
    #[serde(rename = "deviceTheme", default = "device_theme_default")]
    pub device_theme: String,
    #[serde(rename = "userInterfaceTheme", default = "ui_theme_default")]
    pub user_interface_theme: String,
}

fn device_theme_default() -> String {
    "DEVICE_THEME_SELECTED".into()
}

fn ui_theme_default() -> String {
    "USER_INTERFACE_THEME_DARK".into()
}

/// `body/LikeBody.kt` — the polymorphic target: `videoId` for songs,
/// `playlistId` for playlists (Kotlin sealed class with the field name
/// changing per subclass).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LikeBody {
    pub context: Context,
    pub target: LikeTarget,
}

/// `LikeBody.Target` — serializes to `{"videoId": ...}` or
/// `{"playlistId": ...}` exactly like the two Kotlin subclasses.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged, rename_all_fields = "camelCase")]
pub enum LikeTarget {
    Video { video_id: String },
    Playlist { playlist_id: String },
}

impl LikeTarget {
    /// Field names use YouTube's casing on the wire.
    pub fn wire_json(&self) -> serde_json::Value {
        match self {
            LikeTarget::Video { video_id } => {
                serde_json::json!({ "videoId": video_id })
            }
            LikeTarget::Playlist { playlist_id } => {
                serde_json::json!({ "playlistId": playlist_id })
            }
        }
    }
}

/// `body/SubscribeBody.kt`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscribeBody {
    pub channel_ids: Vec<String>,
    pub context: Context,
}

/// `body/CreatePlaylistBody.kt` (+ the `PrivacyStatus` constants).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatePlaylistBody {
    pub context: Context,
    pub title: String,
    #[serde(default = "privacy_private")]
    pub privacy_status: String,
    #[serde(skip_serializing_if = "is_none")]
    pub video_ids: Option<Vec<String>>,
}

pub(crate) fn privacy_private() -> String {
    "PRIVATE".into()
}

/// `CreatePlaylistBody.PrivityStatus` constants (body/CreatePlaylistBody.kt).
pub mod privacy_status {
    pub const PRIVATE: &str = "PRIVATE";
    pub const PUBLIC: &str = "PUBLIC";
    pub const UNLISTED: &str = "UNLISTED";
}

/// `models/PlaylistDeleteBody.kt`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistDeleteBody {
    pub context: Context,
    pub playlist_id: String,
}

/// `body/EditPlaylistBody.kt` — the `browse/edit_playlist` body. The
/// actions are the polymorphic `Action` sealed class.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditPlaylistBody {
    pub context: Context,
    pub playlist_id: String,
    pub actions: Vec<EditAction>,
}

/// The 7 `browse/edit_playlist` actions (body/EditPlaylistBody.kt:9-112).
/// Each serializes with its own `action` discriminator field plus the
/// action-specific fields, exactly like the Kotlin sealed subclasses.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all_fields = "camelCase")]
pub enum EditAction {
    /// `ACTION_ADD_VIDEO` (EditPlaylistBody.kt:13-17).
    #[serde(rename = "ACTION_ADD_VIDEO")]
    AddVideo { added_video_id: String },
    /// `ACTION_ADD_PLAYLIST` (EditPlaylistBody.kt:19-23).
    #[serde(rename = "ACTION_ADD_PLAYLIST")]
    AddPlaylist { added_full_list_id: String },
    /// `ACTION_MOVE_VIDEO_BEFORE` (EditPlaylistBody.kt:25-30).
    #[serde(rename = "ACTION_MOVE_VIDEO_BEFORE")]
    MoveVideo {
        set_video_id: String,
        #[serde(skip_serializing_if = "is_none")]
        moved_set_video_id_successor: Option<String>,
    },
    /// `ACTION_REMOVE_VIDEO` (EditPlaylistBody.kt:32-38).
    #[serde(rename = "ACTION_REMOVE_VIDEO")]
    RemoveVideo {
        set_video_id: String,
        removed_video_id: String,
    },
    /// `ACTION_SET_PLAYLIST_NAME` (EditPlaylistBody.kt:40-44).
    #[serde(rename = "ACTION_SET_PLAYLIST_NAME")]
    RenamePlaylist { playlist_name: String },
    /// `ACTION_SET_CUSTOM_THUMBNAIL` (EditPlaylistBody.kt:46-77).
    #[serde(rename = "ACTION_SET_CUSTOM_THUMBNAIL")]
    SetCustomThumbnail {
        added_custom_thumbnail: AddedCustomThumbnail,
    },
    /// `ACTION_REMOVE_CUSTOM_THUMBNAIL` (EditPlaylistBody.kt:79-96).
    #[serde(rename = "ACTION_REMOVE_CUSTOM_THUMBNAIL")]
    RemoveCustomThumbnail {
        #[serde(default = "deleted_custom_thumbnail_default")]
        deleted_custom_thumbnail: DeletedCustomThumbnail,
    },
}

/// `Action.SetCustomThumbnailAction.AddedCustomThumbnail`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddedCustomThumbnail {
    #[serde(default = "studio_square_thumbnail_key")]
    pub image_key: ImageKey,
    pub playlist_scotty_encrypted_blob_id: String,
}

/// `AddedCustomThumbnail.ImageKey`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImageKey {
    #[serde(default = "studio_square_thumbnail_name")]
    pub name: String,
    #[serde(rename = "type", default = "custom_thumbnail_type")]
    pub kind: String,
}

fn studio_square_thumbnail_key() -> ImageKey {
    ImageKey {
        name: "studio_square_thumbnail".into(),
        kind: "PLAYLIST_IMAGE_TYPE_CUSTOM_THUMBNAIL".into(),
    }
}

fn studio_square_thumbnail_name() -> String {
    "studio_square_thumbnail".into()
}

fn custom_thumbnail_type() -> String {
    "PLAYLIST_IMAGE_TYPE_CUSTOM_THUMBNAIL".into()
}

/// `Action.RemoveCustomThumbnailAction.DeletedCustomThumbnail`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeletedCustomThumbnail {
    #[serde(default = "studio_square_thumbnail_name")]
    pub name: String,
    #[serde(rename = "type", default = "custom_thumbnail_type")]
    pub kind: String,
}

fn deleted_custom_thumbnail_default() -> DeletedCustomThumbnail {
    DeletedCustomThumbnail {
        name: "studio_square_thumbnail".into(),
        kind: "PLAYLIST_IMAGE_TYPE_CUSTOM_THUMBNAIL".into(),
    }
}

/// Convenience constructors matching the upstream InnerTube wrappers
/// (InnerTube.kt:533-740) — the `VL` prefix handling lives here so the
/// endpoint layer stays a thin shape.
impl EditAction {
    /// `addToPlaylist`/`addVideosToPlaylist` use `ACTION_ADD_VIDEO`
    /// (InnerTube.kt:544-546, 563).
    pub fn add_video(added_video_id: impl Into<String>) -> EditAction {
        EditAction::AddVideo {
            added_video_id: added_video_id.into(),
        }
    }

    /// `addPlaylistToPlaylist` uses `ACTION_ADD_PLAYLIST`
    /// (InnerTube.kt:581).
    pub fn add_playlist(added_full_list_id: impl Into<String>) -> EditAction {
        EditAction::AddPlaylist {
            added_full_list_id: added_full_list_id.into(),
        }
    }

    /// `removeFromPlaylist` uses `ACTION_REMOVE_VIDEO`
    /// (InnerTube.kt:601-605).
    pub fn remove_video(
        set_video_id: impl Into<String>,
        removed_video_id: impl Into<String>,
    ) -> EditAction {
        EditAction::RemoveVideo {
            set_video_id: set_video_id.into(),
            removed_video_id: removed_video_id.into(),
        }
    }

    /// `moveSongPlaylist` uses `ACTION_MOVE_VIDEO_BEFORE`
    /// (InnerTube.kt:624-627).
    pub fn move_video(set_video_id: impl Into<String>, successor: Option<String>) -> EditAction {
        EditAction::MoveVideo {
            set_video_id: set_video_id.into(),
            moved_set_video_id_successor: successor,
        }
    }

    /// `renamePlaylist` uses `ACTION_SET_PLAYLIST_NAME`
    /// (InnerTube.kt:661-664).
    pub fn rename_playlist(playlist_name: impl Into<String>) -> EditAction {
        EditAction::RenamePlaylist {
            playlist_name: playlist_name.into(),
        }
    }

    /// `setThumbnailPlaylist` uses `ACTION_SET_CUSTOM_THUMBNAIL`
    /// (InnerTube.kt:713-718).
    pub fn set_custom_thumbnail(
        playlist_scotty_encrypted_blob_id: impl Into<String>,
    ) -> EditAction {
        EditAction::SetCustomThumbnail {
            added_custom_thumbnail: AddedCustomThumbnail {
                image_key: studio_square_thumbnail_key(),
                playlist_scotty_encrypted_blob_id: playlist_scotty_encrypted_blob_id.into(),
            },
        }
    }

    /// `removeThumbnailPlaylist` uses `ACTION_REMOVE_CUSTOM_THUMBNAIL`
    /// (InnerTube.kt:735).
    pub fn remove_custom_thumbnail() -> EditAction {
        EditAction::RemoveCustomThumbnail {
            deleted_custom_thumbnail: deleted_custom_thumbnail_default(),
        }
    }
}

/// `playlistId.removePrefix("VL")` — applied by the add/remove wrappers
/// (InnerTube.kt:543, 562, 579, 598) but NOT by move/rename/thumbnail
/// (InnerTube.kt:622, 659, 711, 733).
pub fn strip_vl_prefix(playlist_id: &str) -> &str {
    playlist_id.strip_prefix("VL").unwrap_or(playlist_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The wire shapes must match kotlinx-serialization byte-for-byte for
    /// the fields YouTube reads. These golden fragments pin the encodings
    /// the Phase 05 parsers and any capture-diff against the Android app
    /// depend on.
    #[test]
    fn edit_actions_serialize_like_kotlin() {
        // ACTION_ADD_VIDEO
        assert_eq!(
            serde_json::to_value(EditAction::add_video("dQw4w9WgXcQ")).unwrap(),
            serde_json::json!({"action": "ACTION_ADD_VIDEO", "addedVideoId": "dQw4w9WgXcQ"})
        );
        // ACTION_MOVE_VIDEO_BEFORE with a null successor: explicitNulls=false
        // omits the field entirely
        assert_eq!(
            serde_json::to_value(EditAction::move_video("SET1", None)).unwrap(),
            serde_json::json!({"action": "ACTION_MOVE_VIDEO_BEFORE", "setVideoId": "SET1"})
        );
        assert_eq!(
            serde_json::to_value(EditAction::move_video("SET1", Some("SET2".into()))).unwrap(),
            serde_json::json!({
                "action": "ACTION_MOVE_VIDEO_BEFORE",
                "setVideoId": "SET1",
                "movedSetVideoIdSuccessor": "SET2"
            })
        );
        // ACTION_REMOVE_VIDEO
        assert_eq!(
            serde_json::to_value(EditAction::remove_video("SET9", "vid9")).unwrap(),
            serde_json::json!({"action": "ACTION_REMOVE_VIDEO", "setVideoId": "SET9", "removedVideoId": "vid9"})
        );
        // ACTION_SET_PLAYLIST_NAME
        assert_eq!(
            serde_json::to_value(EditAction::rename_playlist("My List")).unwrap(),
            serde_json::json!({"action": "ACTION_SET_PLAYLIST_NAME", "playlistName": "My List"})
        );
        // ACTION_SET_CUSTOM_THUMBNAIL with the nested default image key
        assert_eq!(
            serde_json::to_value(EditAction::set_custom_thumbnail("BLOB1")).unwrap(),
            serde_json::json!({
                "action": "ACTION_SET_CUSTOM_THUMBNAIL",
                "addedCustomThumbnail": {
                    "imageKey": {
                        "name": "studio_square_thumbnail",
                        "type": "PLAYLIST_IMAGE_TYPE_CUSTOM_THUMBNAIL"
                    },
                    "playlistScottyEncryptedBlobId": "BLOB1"
                }
            })
        );
        // ACTION_REMOVE_CUSTOM_THUMBNAIL with its defaulted body
        assert_eq!(
            serde_json::to_value(EditAction::remove_custom_thumbnail()).unwrap(),
            serde_json::json!({
                "action": "ACTION_REMOVE_CUSTOM_THUMBNAIL",
                "deletedCustomThumbnail": {
                    "name": "studio_square_thumbnail",
                    "type": "PLAYLIST_IMAGE_TYPE_CUSTOM_THUMBNAIL"
                }
            })
        );
    }

    #[test]
    fn vl_prefix_stripping_matches_remove_prefix() {
        // InnerTube.kt:543/562/579/598 vs 622/659/711/733
        assert_eq!(strip_vl_prefix("VL1234"), "1234");
        assert_eq!(strip_vl_prefix("1234"), "1234");
        assert_eq!(strip_vl_prefix("VLVL12"), "VL12");
        assert_eq!(strip_vl_prefix(""), "");
    }

    #[test]
    fn like_targets_use_youtube_casing() {
        let video = LikeTarget::Video {
            video_id: "abc".into(),
        };
        assert_eq!(
            serde_json::to_value(&video).unwrap(),
            serde_json::json!({"videoId": "abc"})
        );
        let playlist = LikeTarget::Playlist {
            playlist_id: "PL1".into(),
        };
        assert_eq!(
            serde_json::to_value(&playlist).unwrap(),
            serde_json::json!({"playlistId": "PL1"})
        );
    }

    #[test]
    fn player_body_defaults_are_emitted() {
        // contentCheckOk/racyCheckOk default true and ARE on the wire
        // (encodeDefaults=true, PlayerBody.kt:8-9)
        let body = PlayerBody {
            context: Context::default(),
            video_id: "v".into(),
            playlist_id: None,
            playback_context: Some(PlaybackContext {
                content_playback_context: ContentPlaybackContext::new(20_000),
            }),
            service_integrity_dimensions: None,
            content_check_ok: true,
            racy_check_ok: true,
        };
        let json = serde_json::to_value(&body).unwrap();
        assert_eq!(json["contentCheckOk"], true);
        assert_eq!(json["racyCheckOk"], true);
        assert_eq!(
            json["playbackContext"]["contentPlaybackContext"]["signatureTimestamp"],
            20_000
        );
        assert_eq!(
            json["playbackContext"]["contentPlaybackContext"]["html5Preference"],
            "HTML5_PREF_WANTS"
        );
        assert!(json.get("playlistId").is_none());
        assert!(json.get("serviceIntegrityDimensions").is_none());
    }

    #[test]
    fn account_menu_body_defaults() {
        let body = AccountMenuBody {
            context: Context::default(),
            device_theme: device_theme_default(),
            user_interface_theme: ui_theme_default(),
        };
        let json = serde_json::to_value(&body).unwrap();
        assert_eq!(json["deviceTheme"], "DEVICE_THEME_SELECTED");
        assert_eq!(json["userInterfaceTheme"], "USER_INTERFACE_THEME_DARK");
    }

    #[test]
    fn create_playlist_body_defaults_to_private() {
        let body = CreatePlaylistBody {
            context: Context::default(),
            title: "T".into(),
            privacy_status: privacy_private(),
            video_ids: None,
        };
        let json = serde_json::to_value(&body).unwrap();
        assert_eq!(json["privacyStatus"], "PRIVATE");
        assert!(json.get("videoIds").is_none());
    }
}
