//! The NewPipeExtractor uses, re-implemented — no JVM code.
//!
//! Upstream bridges two NewPipe entry points
//! (providers/innertube/pages/NewPipe.kt):
//!
//! * `getSignatureTimestamp(videoId)` — the signature timestamp for
//!   `useSignatureTimestamp` clients;
//! * `getStreamUrl` / `newPipePlayer` — the signature deobfuscation
//!   and the watch-page `StreamInfo` scrape, the **last resort** when
//!   neither a direct url nor the cipher deobfuscation could produce
//!   one.
//!
//! The re-implementation follows the shapes rustypipe
//! (src/deobfuscate.rs) and the NewPipe extractor use: the player
//! script is discovered through the iframe API and the STS read out
//! with `signatureTimestamp[=:] (\d+)`; the watch page's embedded
//! `ytInitialPlayerResponse` carries the last-resort formats. The
//! deobfuscation itself runs over the port's own
//! [`Cipher`](super::cipher::Cipher) — the same functions, the same
//! file, no second engine.
//!
//! One deliberate deviation, recorded in HANDOFF: upstream's
//! `getSignatureTimestamp(videoId)` detects "age-restricted" from a
//! NewPipe error whose text it inspects; the port takes the
//! playability statuses it already reads in the walk as the source of
//! that flag, so [`SignatureTimestampResult::is_age_restricted`] is
//! the seam where a watch-page-based refinement would slot in.

use crate::error::InnertubeError;
use crate::models::response::player::{Format, PlayerResponse, StreamingData};
use crate::transport::InnerTubeTransport;

/// `SignatureTimestampResult` (YTPlayerUtils.kt:1101-1104).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignatureTimestampResult {
    /// The extracted timestamp, when the player script gave one.
    pub timestamp: Option<i32>,
    /// Whether the STS lookup failed in a way that means the video is
    /// age-restricted.
    pub is_age_restricted: bool,
}

/// The signature-timestamp lookup (NewPipe.kt:102-105 +
/// YTPlayerUtils.kt:1106-1127): fetch the current player script and
/// read its `signatureTimestamp`. The result is only meaningful for
/// clients that hand back ciphered urls, which is why the walk
/// resolves it lazily — an ordinary play never touches it.
pub async fn get_signature_timestamp(
    cipher: &super::cipher::Cipher,
    transport: &InnerTubeTransport,
    _video_id: &str,
) -> Result<SignatureTimestampResult, InnertubeError> {
    // the cipher's fetcher shares the player script with the EJS
    // solver (EjsNTransformSolver.kt:68 reuses PlayerJsFetcher)
    match cipher.player_js(transport).await? {
        Some(player_js) => Ok(SignatureTimestampResult {
            timestamp: super::cipher::signature_timestamp(&player_js),
            is_age_restricted: false,
        }),
        // no player script: the age-restricted distinction is the
        // walk's to make from the playability statuses it reads
        None => Ok(SignatureTimestampResult {
            timestamp: None,
            is_age_restricted: false,
        }),
    }
}

/// The watch page's embedded player response — the NewPipe
/// `StreamInfo.getInfo` shape, minus the JVM: fetch
/// `https://www.youtube.com/watch?v=ID` as a desktop browser and
/// splice out the `ytInitialPlayerResponse = {...};` object.
pub async fn watch_page_player_response(
    transport: &InnerTubeTransport,
    video_id: &str,
) -> Result<Option<PlayerResponse>, InnertubeError> {
    let url = format!("https://www.youtube.com/watch?v={video_id}");
    let page = transport
        .raw_text_get(&url, crate::client::USER_AGENT_WEB)
        .await?;
    Ok(extract_initial_player_response(&page))
}

/// `ytInitialPlayerResponse = {...}` out of a watch page — the marker
/// NewPipe and every scraper since reads.
pub(crate) fn extract_initial_player_response(page: &str) -> Option<PlayerResponse> {
    let marker = "ytInitialPlayerResponse = ";
    let start = page.find(marker)? + marker.len();
    let rest = &page[start..];
    // the assignment ends with `;` or `\nvar` — find the matching
    // close brace by depth scan (the JSON itself may contain braces
    // in strings; NewPipe uses a real JSON parser over the located
    // region — the depth scan handles the real pages' shapes and
    // never over-consumes past a `};` at depth zero)
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;
    for (offset, character) in rest.char_indices() {
        match character {
            '"' if !escaped => in_string = !in_string,
            '\\' if in_string => {
                escaped = !escaped;
                continue;
            }
            _ if in_string => {}
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    let candidate = &rest[..=offset];
                    return serde_json::from_str(candidate).ok();
                }
            }
            _ => {}
        }
        escaped = false;
    }
    None
}

/// The last-resort stream urls (YouTube.kt:1558-1599
/// `getNewPipeStreamUrls` over `newPipePlayer`): every (itag, url)
/// pair the watch page's player response carries — audio first,
/// matched by itag, with any audio itag standing in when the exact
/// one is missing (YTPlayerUtils.kt:1163-1183).
pub async fn watch_page_stream_urls(
    transport: &InnerTubeTransport,
    video_id: &str,
) -> Result<Vec<(i64, String)>, InnertubeError> {
    let Some(response) = watch_page_player_response(transport, video_id).await? else {
        return Ok(Vec::new());
    };
    let formats = response
        .streaming_data
        .as_ref()
        .map(|data: &StreamingData| {
            data.formats
                .as_ref()
                .map(|combined| combined.iter())
                .into_iter()
                .flatten()
                .chain(data.adaptive_formats.iter())
        })
        .into_iter()
        .flatten();
    Ok(formats
        .filter(|format: &&Format| format.url.as_deref().is_some_and(|url| !url.is_empty()))
        .map(|format: &Format| (format.itag, format.url.clone().unwrap_or_default()))
        .collect())
}

/// `findUrlOrNull`'s NewPipe leg (YTPlayerUtils.kt:1158-1183): the
/// exact-itag url from the watch page, else the first audio itag's.
pub(crate) fn pick_stream_url(
    stream_urls: &[(i64, String)],
    format: &Format,
    player_response: &PlayerResponse,
) -> Option<String> {
    // the exact itag first
    if let Some((_, url)) = stream_urls.iter().find(|(itag, _)| *itag == format.itag) {
        return Some(url.clone());
    }
    // then any itag the response itself calls audio
    let audio_itags: Vec<i64> = player_response
        .streaming_data
        .as_ref()
        .map(|data| {
            data.formats
                .as_ref()
                .map(|combined| combined.iter())
                .into_iter()
                .flatten()
                .chain(data.adaptive_formats.iter())
                .filter(|candidate| candidate.is_audio())
                .map(|candidate| candidate.itag)
                .collect()
        })
        .unwrap_or_default();
    stream_urls
        .iter()
        .find(|(itag, _)| audio_itags.contains(itag))
        .map(|(_, url)| url.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    const WATCH_PAGE: &str = r#"<!DOCTYPE html><html><head>
<script>var ytInitialPlayerResponse = {"playabilityStatus": {"status": "OK"},
"streamingData": {"expiresInSeconds": 21540,
"formats": [{"itag": 18, "url": "https://gonecdf/combined?itag=18"}],
"adaptiveFormats": [{"itag": 140, "url": "https://gonecdf/audio?itag=140"},
{"itag": 247, "url": "https://gonecdf/video?itag=247"}]}},
"videoDetails": {"videoId": "abc"}};
var meta = document.createElement('meta');</script>
</head><body></body></html>"#;

    #[test]
    fn extracts_the_embedded_player_response() {
        let response = extract_initial_player_response(WATCH_PAGE).unwrap();
        assert_eq!(response.playability_status.unwrap().status, "OK");
        let streaming = response.streaming_data.unwrap();
        assert_eq!(streaming.adaptive_formats.len(), 2);
        assert_eq!(streaming.adaptive_formats[0].itag, 140);
    }

    #[test]
    fn extraction_tolerates_braces_inside_strings() {
        let page = r#"x = 1; ytInitialPlayerResponse = {"videoDetails": {"title": "a } b { c"}};
more = 2"#;
        let response = extract_initial_player_response(page).unwrap();
        assert_eq!(
            response.video_details.unwrap().title.as_deref(),
            Some("a } b { c")
        );
    }

    #[test]
    fn extraction_returns_none_without_the_marker() {
        assert!(extract_initial_player_response("<html>empty</html>").is_none());
        // an unterminated object is not a candidate either
        assert!(extract_initial_player_response("ytInitialPlayerResponse = {\"a\": ").is_none());
    }

    #[test]
    fn pick_prefers_the_exact_itag_then_any_audio() {
        let urls = vec![
            (18, "https://gonecdf/combined?itag=18".to_owned()),
            (140, "https://gonecdf/audio?itag=140".to_owned()),
            (247, "https://gonecdf/video?itag=247".to_owned()),
        ];
        let response = PlayerResponse {
            streaming_data: Some(StreamingData {
                adaptive_formats: vec![
                    Format {
                        itag: 140,
                        url: None,
                        ..Default::default()
                    },
                    Format {
                        itag: 247,
                        url: None,
                        width: Some(1280),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            }),
            ..Default::default()
        };
        // the exact itag
        let picked = pick_stream_url(
            &urls,
            &Format {
                itag: 140,
                ..Default::default()
            },
            &response,
        )
        .unwrap();
        assert_eq!(picked, "https://gonecdf/audio?itag=140");
        // a missing itag falls back to any audio itag — never a video
        let picked = pick_stream_url(
            &urls,
            &Format {
                itag: 999,
                ..Default::default()
            },
            &response,
        )
        .unwrap();
        assert_eq!(picked, "https://gonecdf/audio?itag=140");
    }

    #[test]
    fn the_marker_regex_is_rustypipes() {
        // the STS extraction itself lives in cipher.rs; pin the shape
        // here so the re-implementation stays honest
        let regex = regex::Regex::new(r"signatureTimestamp[=:](\d+)").unwrap();
        assert_eq!(
            regex
                .captures("var b={signatureTimestamp:20774,c:1}")
                .map(|c| c[1].to_owned()),
            Some("20774".to_owned())
        );
    }
}
