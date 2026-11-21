//! `models/Thumbnails.kt` + `models/ThumbnailRenderer.kt`.

use serde::{Deserialize, Serialize};

/// `Thumbnails.kt:6-8`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Thumbnails {
    #[serde(default)]
    pub thumbnails: Vec<Thumbnail>,
}

/// `Thumbnails.kt:10-15`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Thumbnail {
    pub url: String,
    #[serde(default)]
    pub width: Option<i64>,
    #[serde(default)]
    pub height: Option<i64>,
}

/// `ThumbnailRenderer.kt:8-14`.
///
/// Upstream marks `musicThumbnailRenderer` with
/// `@JsonNames("croppedSquareThumbnailRenderer")` AND declares a separate
/// `croppedSquareThumbnailRenderer` field, so either key can feed either
/// read path. serde aliases cannot serve two masters, so the port decodes
/// through a raw mirror and folds: a `croppedSquareThumbnailRenderer` key
/// populates BOTH fields when `musicThumbnailRenderer` is absent — the
/// observable behavior of the kotlinx alias.
#[derive(Debug, Clone, PartialEq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ThumbnailRenderer {
    #[serde(default)]
    pub music_thumbnail_renderer: Option<MusicThumbnailRenderer>,
    #[serde(default)]
    pub music_animated_thumbnail_renderer: Option<MusicAnimatedThumbnailRenderer>,
    #[serde(default)]
    pub cropped_square_thumbnail_renderer: Option<MusicThumbnailRenderer>,
}

impl<'de> Deserialize<'de> for ThumbnailRenderer {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize, Default)]
        #[serde(rename_all = "camelCase")]
        struct Raw {
            #[serde(default)]
            music_thumbnail_renderer: Option<MusicThumbnailRenderer>,
            #[serde(default)]
            music_animated_thumbnail_renderer: Option<MusicAnimatedThumbnailRenderer>,
            #[serde(default)]
            cropped_square_thumbnail_renderer: Option<MusicThumbnailRenderer>,
        }
        let raw = Raw::deserialize(deserializer)?;
        Ok(ThumbnailRenderer {
            music_thumbnail_renderer: raw
                .music_thumbnail_renderer
                .or(raw.cropped_square_thumbnail_renderer.clone()),
            music_animated_thumbnail_renderer: raw.music_animated_thumbnail_renderer,
            cropped_square_thumbnail_renderer: raw.cropped_square_thumbnail_renderer,
        })
    }
}

/// `ThumbnailRenderer.kt:15-23`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicThumbnailRenderer {
    pub thumbnail: Thumbnails,
    #[serde(default)]
    pub thumbnail_crop: Option<String>,
    #[serde(default)]
    pub thumbnail_scale: Option<String>,
}

impl MusicThumbnailRenderer {
    /// `getThumbnailUrl()` (`ThumbnailRenderer.kt:21`).
    pub fn get_thumbnail_url(&self) -> Option<&str> {
        self.thumbnail.thumbnails.last().map(|t| t.url.as_str())
    }
}

/// `ThumbnailRenderer.kt:24-28`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicAnimatedThumbnailRenderer {
    pub animated_thumbnail: Thumbnails,
    pub backup_renderer: MusicThumbnailRenderer,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The JsonNames fold: `croppedSquareThumbnailRenderer` feeds both the
    /// aliased field and the explicit one (ThumbnailRenderer.kt:9-13).
    #[test]
    fn cropped_square_alias_folds_into_music_thumbnail() {
        let value = serde_json::json!({
            "croppedSquareThumbnailRenderer": {
                "thumbnail": { "thumbnails": [
                    { "url": "https://example.com/120.jpg", "width": 120, "height": 120 },
                    { "url": "https://example.com/544.jpg", "width": 544, "height": 544 }
                ]}
            }
        });
        let renderer: ThumbnailRenderer = serde_json::from_value(value).unwrap();
        let music = renderer.music_thumbnail_renderer.as_ref().unwrap();
        assert_eq!(
            music.get_thumbnail_url(),
            Some("https://example.com/544.jpg")
        );
        assert!(renderer.cropped_square_thumbnail_renderer.is_some());
    }

    /// The primary name wins and does not leak into the explicit field.
    #[test]
    fn primary_name_stays_primary() {
        let value = serde_json::json!({
            "musicThumbnailRenderer": {
                "thumbnail": { "thumbnails": [{ "url": "https://example.com/a.jpg" }] }
            }
        });
        let renderer: ThumbnailRenderer = serde_json::from_value(value).unwrap();
        assert!(renderer.music_thumbnail_renderer.is_some());
        assert!(renderer.cropped_square_thumbnail_renderer.is_none());
    }
}
