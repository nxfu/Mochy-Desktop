//! `innertube/utils/Utils.kt` + `utils/YouTubeUrlParser.kt` — the pure
//! helpers (the paging half of Utils.kt lives in [`crate::paging`]).

/// `Utils.kt:90-100` — `String.parseTime()`.
///
/// `^(?:(\p{Nd}+)[:.])?(\p{Nd}+)[:.](\p{Nd}{2})$` — some languages write
/// durations with a dot instead of a colon (indonesian "3.45", finnish,
/// danish), so either separator is accepted. Kotlin's `\d` is Unicode Nd;
/// the regex crate's `\d` is too (the conversion itself is ASCII — see
/// the HANDOFF deviation note).
pub fn parse_time(text: &str) -> Option<i32> {
    use regex::Regex;
    use std::sync::OnceLock;
    static DURATION: OnceLock<Regex> = OnceLock::new();
    let duration =
        DURATION.get_or_init(|| Regex::new(r"^(?:(\d+)[:.])?(\d+)[:.](\d{2})$").unwrap());
    let captures = duration.captures(text.trim())?;
    let hours: i64 = captures
        .get(1)
        .and_then(|m| m.as_str().parse().ok())
        .unwrap_or(0);
    let minutes: i64 = captures.get(2)?.as_str().parse().ok()?;
    let seconds: i64 = captures.get(3)?.as_str().parse().ok()?;
    Some((hours * 3600 + minutes * 60 + seconds) as i32)
}

/// `Utils.kt:102-104` — the privately-owned release marker.
pub fn is_private_id(browse_id: &str) -> bool {
    browse_id.contains("privately")
}

/// `YouTubeUrlParser.kt:6-69` — the YouTube link shapes the share sheet
/// and the deep-link router accept. The upstream `[LINK_PARSE_DEBUG]`
/// println noise is dropped (debug-build spam; behavior unchanged).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedUrl {
    /// `ParsedUrl.Video`.
    Video { id: String },
    /// `ParsedUrl.Artist`.
    Artist { id: String },
}

impl ParsedUrl {
    pub fn id(&self) -> &str {
        match self {
            ParsedUrl::Video { id } | ParsedUrl::Artist { id } => id,
        }
    }
}

/// `isYouTubeUrl` (YouTubeUrlParser.kt:33).
pub fn is_youtube_url(text: &str) -> bool {
    parse_youtube_url(text).is_some()
}

/// `parse` (YouTubeUrlParser.kt:35-61) — video links first, then the
/// music.youtube.com channel/browse shapes.
pub fn parse_youtube_url(url: &str) -> Option<ParsedUrl> {
    use regex::Regex;
    use std::sync::OnceLock;
    static VIDEO: OnceLock<Vec<Regex>> = OnceLock::new();
    static ARTIST: OnceLock<Vec<Regex>> = OnceLock::new();

    let video = VIDEO.get_or_init(|| {
        [
            r"(?:https?://)?(?:www\.)?(?:music\.)?youtube\.com/watch\?.*v=([a-zA-Z0-9_-]{11})",
            r"(?:https?://)?(?:www\.)?(?:music\.)?youtube\.com/watch\?v=([a-zA-Z0-9_-]{11})",
            r"(?:https?://)?youtu\.be/([a-zA-Z0-9_-]{11})",
            r"(?:https?://)?(?:www\.)?youtube\.com/shorts/([a-zA-Z0-9_-]{11})",
        ]
        .iter()
        .map(|pattern| Regex::new(pattern).unwrap())
        .collect()
    });
    let artist = ARTIST.get_or_init(|| {
        [
            r"(?:https?://)?(?:www\.)?music\.youtube\.com/channel/([a-zA-Z0-9_-]+)",
            r"(?:https?://)?(?:www\.)?music\.youtube\.com/browse/(MPRE[a-zA-Z0-9_-]+)",
        ]
        .iter()
        .map(|pattern| Regex::new(pattern).unwrap())
        .collect()
    });

    let trimmed = url.trim();
    for pattern in video {
        if let Some(id) = pattern
            .captures(trimmed)
            .and_then(|captures| captures.get(1))
        {
            return Some(ParsedUrl::Video {
                id: id.as_str().to_owned(),
            });
        }
    }
    if trimmed.contains("music.youtube.com") {
        for pattern in artist {
            if let Some(id) = pattern
                .captures(trimmed)
                .and_then(|captures| captures.get(1))
            {
                return Some(ParsedUrl::Artist {
                    id: id.as_str().to_owned(),
                });
            }
        }
    }
    None
}

/// `extractVideoId` (YouTubeUrlParser.kt:63).
pub fn extract_video_id(url: &str) -> Option<String> {
    match parse_youtube_url(url)? {
        ParsedUrl::Video { id } => Some(id),
        ParsedUrl::Artist { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Utils.kt:90-100 — colon and dot separators, hours optional.
    #[test]
    fn durations_parse_both_separators() {
        assert_eq!(parse_time("3:45"), Some(225));
        assert_eq!(parse_time("1:02:03"), Some(3723));
        assert_eq!(parse_time("3.45"), Some(225)); // indonesian style
        assert_eq!(parse_time(""), None);
        assert_eq!(parse_time("3:5"), None);
        assert_eq!(parse_time("song"), None);
    }

    /// Utils.kt:102-104.
    #[test]
    fn private_id_detection() {
        assert!(is_private_id(
            "FEmusic_library_privately_owned_release_detail_xyz"
        ));
        assert!(!is_private_id("MPREb_xyz"));
    }

    /// YouTubeUrlParser.kt:19-31.
    #[test]
    fn url_shapes() {
        assert_eq!(
            extract_video_id("https://music.youtube.com/watch?v=dQw4w9WgXcQ&si=x"),
            Some("dQw4w9WgXcQ".into())
        );
        assert_eq!(
            extract_video_id("https://youtu.be/dQw4w9WgXcQ"),
            Some("dQw4w9WgXcQ".into())
        );
        assert_eq!(
            extract_video_id("https://www.youtube.com/shorts/dQw4w9WgXcQ"),
            Some("dQw4w9WgXcQ".into())
        );
        assert_eq!(extract_video_id("not a link"), None);
        // artist channel
        assert_eq!(
            parse_youtube_url("https://music.youtube.com/channel/UC123abc_-"),
            Some(ParsedUrl::Artist {
                id: "UC123abc_-".into()
            })
        );
        // plain youtube.com/channel is NOT accepted (music. prefix required)
        assert_eq!(
            parse_youtube_url("https://www.youtube.com/channel/UC123"),
            None
        );
    }
}
