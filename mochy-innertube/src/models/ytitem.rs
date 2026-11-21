//! `models/YTItem.kt` — the four domain items every parser produces,
//! plus the content filters built on them.

use serde::{Deserialize, Serialize};

use crate::models::endpoint::WatchEndpoint;
use crate::models::endpoint::music_video_type;

/// `YTItem.kt:17-20`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Artist {
    pub name: String,
    #[serde(default)]
    pub id: Option<String>,
}

/// `YTItem.kt:22-26`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Album {
    pub name: String,
    pub id: String,
}

/// `YTItem.kt:28-51` — `SongItem`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SongItem {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub artists: Vec<Artist>,
    #[serde(default)]
    pub album: Option<Album>,
    #[serde(default)]
    pub duration: Option<i32>,
    #[serde(default)]
    pub music_video_type: Option<String>,
    #[serde(default)]
    pub chart_position: Option<i32>,
    #[serde(default)]
    pub chart_change: Option<String>,
    pub thumbnail: String,
    #[serde(default)]
    pub explicit: bool,
    #[serde(default)]
    pub endpoint: Option<WatchEndpoint>,
    #[serde(default)]
    pub set_video_id: Option<String>,
    #[serde(default)]
    pub library_add_token: Option<String>,
    #[serde(default)]
    pub library_remove_token: Option<String>,
    #[serde(default)]
    pub history_remove_token: Option<String>,
}

impl SongItem {
    /// `isVideoSong` (YTItem.kt:46-47).
    pub fn is_video_song(&self) -> bool {
        self.music_video_type
            .as_deref()
            .map(|t| t != music_video_type::ATV)
            .unwrap_or(false)
    }

    /// `shareLink` (YTItem.kt:49-50).
    pub fn share_link(&self) -> String {
        format!("https://music.youtube.com/watch?v={}", self.id)
    }
}

/// `YTItem.kt:53-67` — `AlbumItem` (upstream: `id` defaults to `browseId`,
/// so the port derives it from `browse_id` instead of storing a copy).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AlbumItem {
    pub browse_id: String,
    pub playlist_id: String,
    pub title: String,
    #[serde(default)]
    pub artists: Option<Vec<Artist>>,
    #[serde(default)]
    pub year: Option<i32>,
    pub thumbnail: String,
    #[serde(default)]
    pub explicit: bool,
    #[serde(default)]
    pub description: Option<String>,
}

impl AlbumItem {
    /// `override val id = browseId` (YTItem.kt:57).
    pub fn id(&self) -> &str {
        &self.browse_id
    }

    /// `shareLink` (YTItem.kt:65-66).
    pub fn share_link(&self) -> String {
        format!(
            "https://music.youtube.com/playlist?list={}",
            self.playlist_id
        )
    }
}

/// `YTItem.kt:69-85` — `PlaylistItem`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistItem {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub author: Option<Artist>,
    #[serde(default)]
    pub song_count_text: Option<String>,
    #[serde(default)]
    pub thumbnail: Option<String>,
    #[serde(default)]
    pub play_endpoint: Option<WatchEndpoint>,
    #[serde(default)]
    pub shuffle_endpoint: Option<WatchEndpoint>,
    #[serde(default)]
    pub radio_endpoint: Option<WatchEndpoint>,
    #[serde(default)]
    pub is_editable: bool,
}

impl PlaylistItem {
    /// `explicit` is always false (YTItem.kt:81-83).
    pub fn explicit(&self) -> bool {
        false
    }

    /// `shareLink` (YTItem.kt:84).
    pub fn share_link(&self) -> String {
        format!("https://music.youtube.com/playlist?list={}", self.id)
    }
}

/// `YTItem.kt:87-101` — `ArtistItem`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ArtistItem {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub thumbnail: Option<String>,
    #[serde(default)]
    pub channel_id: Option<String>,
    #[serde(default)]
    pub play_endpoint: Option<WatchEndpoint>,
    #[serde(default)]
    pub shuffle_endpoint: Option<WatchEndpoint>,
    #[serde(default)]
    pub radio_endpoint: Option<WatchEndpoint>,
}

impl ArtistItem {
    /// `explicit` is always false (YTItem.kt:97-99).
    pub fn explicit(&self) -> bool {
        false
    }

    /// `shareLink` (YTItem.kt:100).
    pub fn share_link(&self) -> String {
        format!("https://music.youtube.com/channel/{}", self.id)
    }
}

/// The sealed `YTItem` hierarchy (`YTItem.kt:7-14`) — one enum, four
/// variants, the abstract members as accessors.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum YTItem {
    Song(SongItem),
    Album(AlbumItem),
    Playlist(PlaylistItem),
    Artist(ArtistItem),
}

impl YTItem {
    /// `abstract val id`.
    pub fn id(&self) -> &str {
        match self {
            YTItem::Song(item) => &item.id,
            YTItem::Album(item) => item.id(),
            YTItem::Playlist(item) => &item.id,
            YTItem::Artist(item) => &item.id,
        }
    }

    /// `abstract val title`.
    pub fn title(&self) -> &str {
        match self {
            YTItem::Song(item) => &item.title,
            YTItem::Album(item) => &item.title,
            YTItem::Playlist(item) => &item.title,
            YTItem::Artist(item) => &item.title,
        }
    }

    /// `abstract val thumbnail`.
    pub fn thumbnail(&self) -> Option<&str> {
        match self {
            YTItem::Song(item) => Some(item.thumbnail.as_str()),
            YTItem::Album(item) => Some(item.thumbnail.as_str()),
            YTItem::Playlist(item) => item.thumbnail.as_deref(),
            YTItem::Artist(item) => item.thumbnail.as_deref(),
        }
    }

    /// `abstract val explicit`.
    pub fn explicit(&self) -> bool {
        match self {
            YTItem::Song(item) => item.explicit,
            YTItem::Album(item) => item.explicit,
            YTItem::Playlist(item) => item.explicit(),
            YTItem::Artist(item) => item.explicit(),
        }
    }

    /// `abstract val shareLink`.
    pub fn share_link(&self) -> String {
        match self {
            YTItem::Song(item) => item.share_link(),
            YTItem::Album(item) => item.share_link(),
            YTItem::Playlist(item) => item.share_link(),
            YTItem::Artist(item) => item.share_link(),
        }
    }

    /// `looksAiGenerated()` (YTItem.kt:143-148) — the title, and the
    /// artist/album/playlist-owner names too.
    pub fn looks_ai_generated(&self) -> bool {
        if is_ai_generated_text(Some(self.title())) {
            return true;
        }
        match self {
            YTItem::Song(item) => {
                item.artists
                    .iter()
                    .any(|artist| is_ai_generated_text(Some(&artist.name)))
                    || item
                        .album
                        .as_ref()
                        .map(|album| is_ai_generated_text(Some(&album.name)))
                        .unwrap_or(false)
            }
            YTItem::Album(item) => item
                .artists
                .as_ref()
                .map(|artists| {
                    artists
                        .iter()
                        .any(|artist| is_ai_generated_text(Some(&artist.name)))
                })
                .unwrap_or(false),
            YTItem::Playlist(item) => item
                .author
                .as_ref()
                .map(|author| is_ai_generated_text(Some(&author.name)))
                .unwrap_or(false),
            YTItem::Artist(_) => false,
        }
    }
}

/// `List<T>.filterExplicit()` (YTItem.kt:103-108).
pub fn filter_explicit(items: &[YTItem], enabled: bool) -> Vec<YTItem> {
    if enabled {
        items
            .iter()
            .filter(|item| !item.explicit())
            .cloned()
            .collect()
    } else {
        items.to_vec()
    }
}

/// `List<T>.filterVideoSongs()` (YTItem.kt:110-115).
pub fn filter_video_songs(items: &[YTItem], disable_videos: bool) -> Vec<YTItem> {
    if disable_videos {
        items
            .iter()
            .filter(|item| !(matches!(item, YTItem::Song(song) if song.is_video_song())))
            .cloned()
            .collect()
    } else {
        items.to_vec()
    }
}

/// `SHORT_MAX_SECONDS` (YTItem.kt:118).
const SHORT_MAX_SECONDS: i32 = 90;

/// `SongItem.isLikelyShort` (YTItem.kt:123-125): a `#shorts` tag, or a
/// fan upload barely a minute long.
pub fn song_is_likely_short(song: &SongItem) -> bool {
    let hashtag = song.title.to_lowercase().contains("#shorts")
        || song.title.to_lowercase().contains("#short");
    hashtag
        || (song.music_video_type.as_deref() == Some(music_video_type::UGC)
            && song
                .duration
                .map(|d| d <= SHORT_MAX_SECONDS)
                .unwrap_or(false))
}

/// `List<T>.filterYoutubeShorts()` (YTItem.kt:127-132) — a playlist whose
/// id starts with `SS` is a short too.
pub fn filter_youtube_shorts(items: &[YTItem], enabled: bool) -> Vec<YTItem> {
    if enabled {
        items
            .iter()
            .filter(|item| {
                !(matches!(item, YTItem::Playlist(p) if p.id.starts_with("SS"))
                    || matches!(item, YTItem::Song(s) if song_is_likely_short(s)))
            })
            .cloned()
            .collect()
    } else {
        items.to_vec()
    }
}

/// `AI_GENERATED_PATTERN` (YTItem.kt:135-137) and `isAiGeneratedText`
/// (YTItem.kt:140). Words that give away music made with ai; "suno"
/// alone is hindi for "listen", so only "suno ai" counts.
pub fn is_ai_generated_text(text: Option<&str>) -> bool {
    use regex::Regex;
    use std::sync::OnceLock;
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    let pattern = PATTERN.get_or_init(|| {
        Regex::new(
            r"(?i)\b(ai[\s-]*(covers?|generated|music|remix|version|voice|vocals?)|suno\s*ai|udio|(made|created|generated|produced)\s*(with|by|using)\s*ai)\b|[(\[]\s*ai\s*[)\]]",
        )
        .unwrap()
    });
    text.is_some_and(|text| pattern.is_match(text))
}

/// `List<T>.filterAiGenerated()` (YTItem.kt:150-155).
pub fn filter_ai_generated(items: &[YTItem], enabled: bool) -> Vec<YTItem> {
    if enabled {
        items
            .iter()
            .filter(|item| !item.looks_ai_generated())
            .cloned()
            .collect()
    } else {
        items.to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song(music_video_type: Option<&str>, duration: Option<i32>, title: &str) -> SongItem {
        SongItem {
            id: "dQw4w9WgXcQ".into(),
            title: title.into(),
            artists: vec![],
            album: None,
            duration,
            music_video_type: music_video_type.map(str::to_owned),
            chart_position: None,
            chart_change: None,
            thumbnail: "https://example.com/t.jpg".into(),
            explicit: false,
            endpoint: None,
            set_video_id: None,
            library_add_token: None,
            library_remove_token: None,
            history_remove_token: None,
        }
    }

    /// YTItem.kt:46-47.
    #[test]
    fn video_song_detection() {
        assert!(song(Some("MUSIC_VIDEO_TYPE_OMV"), None, "T").is_video_song());
        assert!(!song(Some("MUSIC_VIDEO_TYPE_ATV"), None, "T").is_video_song());
        assert!(!song(None, None, "T").is_video_song());
    }

    /// YTItem.kt:117-125.
    #[test]
    fn likely_short_heuristic() {
        // #shorts tag wins regardless of type
        assert!(song_is_likely_short(&song(
            Some("MUSIC_VIDEO_TYPE_UGC"),
            Some(200),
            "cool clip #Shorts"
        )));
        // UGC under 90s
        assert!(song_is_likely_short(&song(
            Some("MUSIC_VIDEO_TYPE_UGC"),
            Some(76),
            "fan upload"
        )));
        // UGC long, or ATV, or no type
        assert!(!song_is_likely_short(&song(
            Some("MUSIC_VIDEO_TYPE_UGC"),
            Some(160),
            "fan upload"
        )));
        assert!(!song_is_likely_short(&song(
            Some("MUSIC_VIDEO_TYPE_ATV"),
            Some(60),
            "official"
        )));
        assert!(!song_is_likely_short(&song(None, Some(60), "official")));
    }

    /// YTItem.kt:134-140.
    #[test]
    fn ai_text_heuristic() {
        assert!(is_ai_generated_text(Some("AI generated music")));
        assert!(is_ai_generated_text(Some("A Song (AI)")));
        assert!(is_ai_generated_text(Some("made with AI")));
        assert!(is_ai_generated_text(Some("suno ai hit")));
        assert!(is_ai_generated_text(Some("an udio track")));
        assert!(!is_ai_generated_text(Some("suno"))); // hindi for "listen"
        assert!(!is_ai_generated_text(Some("Radioactive")));
        assert!(!is_ai_generated_text(None));
    }

    /// YTItem.kt:103-155 — the filter family drops empty-handed sections
    /// upstream; the plain list versions here only drop items.
    #[test]
    fn filters_drop_matching_items() {
        let items = vec![
            YTItem::Song(song(Some("MUSIC_VIDEO_TYPE_ATV"), None, "clean")),
            YTItem::Song({
                let mut s = song(None, None, "explicit");
                s.explicit = true;
                s
            }),
        ];
        assert_eq!(filter_explicit(&items, true).len(), 1);
        assert_eq!(filter_explicit(&items, false).len(), 2);
    }
}
