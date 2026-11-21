//! `Captions.kt` — turning a video's captions into lyrics: which track to
//! use, and its cues as LRC lines.

use crate::models::response::{CaptionTrack, PlayerResponse};

/// `pickCaptionTrack` (Captions.kt:15-21) — the auto-generated track or an
/// uploaded one in its language; the auto track says which language is
/// sung, uploads are as often translations, and with none nothing is
/// picked.
pub fn pick_caption_track(tracks: &[CaptionTrack]) -> Option<&CaptionTrack> {
    let auto = tracks.iter().find(|track| track.is_auto_generated())?;
    let language = auto
        .language_code
        .as_ref()
        .map(|code| code.split('-').next().unwrap_or(code).to_owned());
    Some(
        tracks
            .iter()
            .find(|track| {
                !track.is_auto_generated()
                    && track
                        .language_code
                        .as_ref()
                        .map(|code| {
                            Some(code.split('-').next().unwrap_or(code)) == language.as_deref()
                        })
                        .unwrap_or(false)
            })
            .unwrap_or(auto),
    )
}

/// The caption tracks off a player response (`playerCaptionsTracklistRenderer`).
pub fn player_caption_tracks(response: &PlayerResponse) -> Vec<CaptionTrack> {
    response
        .captions
        .as_ref()
        .and_then(|captions| captions.player_captions_tracklist_renderer.as_ref())
        .and_then(|renderer| renderer.caption_tracks.clone())
        .unwrap_or_default()
}

/// `timedTextToLrc` (Captions.kt:23-43) — timed-text json (fmt=json3) as
/// lrc, one line per caption; the empty line-break events drop out and
/// sound labels like "[music]" and note signs are not words and go.
pub fn timed_text_to_lrc(json: &str) -> String {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
        return String::new();
    };
    let Some(events) = value.get("events").and_then(|events| events.as_array()) else {
        return String::new();
    };
    let mut lines = Vec::new();
    let mut previous: Option<String> = None;
    for event in events {
        let Some(start) = event.get("tStartMs").and_then(|v| v.as_i64()) else {
            continue;
        };
        let Some(segs) = event.get("segs").and_then(|segs| segs.as_array()) else {
            continue;
        };
        let text = clean_caption_text(
            &segs
                .iter()
                .map(|seg| {
                    seg.get("utf8")
                        .and_then(|utf8| utf8.as_str())
                        .unwrap_or_default()
                })
                .collect::<String>(),
        );
        // a rolling caption repeats its line as the next one scrolls in
        if text.is_empty() || previous.as_deref() == Some(text.as_str()) {
            continue;
        }
        previous = Some(text.clone());
        lines.push(format!(
            "[{:02}:{:02}.{:03}]{}",
            start / 60_000,
            start / 1000 % 60,
            start % 1000,
            text
        ));
    }
    lines.join("\n")
}

/// `timedTextToCues` (Captions.kt:56-85) — timed-text json (fmt=json3) as
/// cues, split wherever youtube timed a piece on its own, so auto
/// captions keep their word timing.
pub fn timed_text_to_cues(json: &str) -> Vec<crate::models::account::CaptionCue> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
        return Vec::new();
    };
    let Some(events) = value.get("events").and_then(|events| events.as_array()) else {
        return Vec::new();
    };
    let mut cues = Vec::new();
    for event in events {
        let Some(start) = event.get("tStartMs").and_then(|v| v.as_i64()) else {
            continue;
        };
        let end = start
            + event
                .get("dDurationMs")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
        let Some(segs) = event.get("segs").and_then(|segs| segs.as_array()) else {
            continue;
        };
        // each piece as (start, text); a seg with no offset of its own
        // belongs to the piece before it (Captions.kt:64-71)
        let mut pieces: Vec<(i64, String)> = Vec::new();
        for seg in segs {
            let text = seg
                .get("utf8")
                .and_then(|utf8| utf8.as_str())
                .unwrap_or_default();
            let offset = seg.get("tOffsetMs").and_then(|v| v.as_i64());
            match offset {
                Some(offset) => pieces.push((start + offset, text.to_owned())),
                None if pieces.is_empty() => pieces.push((start, text.to_owned())),
                None => {
                    if let Some(last) = pieces.last_mut() {
                        last.1.push_str(text);
                    }
                }
            }
        }
        for (index, (piece_start, raw)) in pieces.iter().enumerate() {
            let text = clean_caption_text(raw);
            if text.is_empty() {
                continue;
            }
            let piece_end = pieces.get(index + 1).map(|(next, _)| *next).unwrap_or(end);
            cues.push(crate::models::account::CaptionCue {
                start_ms: *piece_start,
                end_ms: piece_end.max(*piece_start),
                text,
            });
        }
    }
    cues.sort_by_key(|cue| cue.start_ms);
    cues
}

/// The shared text scrub of both converters (Captions.kt:33-36 + 75-78):
/// sound labels in brackets, note signs and runs of whitespace go.
fn clean_caption_text(text: &str) -> String {
    use regex::Regex;
    use std::sync::OnceLock;
    static SOUND_LABEL: OnceLock<Regex> = OnceLock::new();
    static WHITESPACE: OnceLock<Regex> = OnceLock::new();
    let sound_label = SOUND_LABEL.get_or_init(|| Regex::new(r"\[[^\]]*\]").unwrap());
    let whitespace = WHITESPACE.get_or_init(|| Regex::new(r"\s+").unwrap());
    let text = sound_label.replace_all(text, " ");
    let text = text.replace('♪', " ");
    whitespace.replace_all(&text, " ").trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Captions.kt:15-21 — the uploaded track in the auto track's
    /// language wins; else the auto track itself.
    #[test]
    fn picks_the_uploaded_track_in_language() {
        let tracks = vec![
            CaptionTrack {
                base_url: "auto-en".into(),
                language_code: Some("en-US".into()),
                kind: Some("asr".into()),
            },
            CaptionTrack {
                base_url: "uploaded-en".into(),
                language_code: Some("en".into()),
                kind: None,
            },
            CaptionTrack {
                base_url: "uploaded-de".into(),
                language_code: Some("de".into()),
                kind: None,
            },
        ];
        assert_eq!(pick_caption_track(&tracks).unwrap().base_url, "uploaded-en");
        // only the auto track -> itself
        assert_eq!(
            pick_caption_track(&tracks[..1]).unwrap().base_url,
            "auto-en"
        );
        // no auto track -> nothing
        assert!(pick_caption_track(&tracks[1..]).is_none());
    }

    /// Captions.kt:23-43.
    #[test]
    fn json3_to_lrc() {
        let json = r#"{
            "events": [
                { "tStartMs": 0, "dDurationMs": 2000, "segs": [ { "utf8": "[Music] " } ] },
                { "tStartMs": 1000, "dDurationMs": 1000, "segs": [ { "utf8": "\n" } ] },
                { "tStartMs": 2000, "dDurationMs": 3000, "segs": [ { "utf8": "hello ♪ world" } ] },
                { "tStartMs": 5000, "dDurationMs": 1000, "segs": [ { "utf8": "hello ♪ world" } ] },
                { "tStartMs": 61000, "dDurationMs": 1000, "segs": [ { "utf8": "again" } ] }
            ]
        }"#;
        let lrc = timed_text_to_lrc(json);
        assert_eq!(lrc, "[00:02.000]hello world\n[01:01.000]again");
    }

    /// Captions.kt:56-85 — word-timed pieces become their own cues.
    #[test]
    fn json3_to_cues_keeps_word_timing() {
        let json = r#"{
            "events": [
                {
                    "tStartMs": 4000, "dDurationMs": 2000,
                    "segs": [
                        { "utf8": "one ", "tOffsetMs": 0 },
                        { "utf8": "two", "tOffsetMs": 1000 }
                    ]
                }
            ]
        }"#;
        let cues = timed_text_to_cues(json);
        assert_eq!(cues.len(), 2);
        assert_eq!(
            (cues[0].start_ms, cues[0].end_ms, cues[0].text.as_str()),
            (4000, 5000, "one")
        );
        assert_eq!(
            (cues[1].start_ms, cues[1].end_ms, cues[1].text.as_str()),
            (5000, 6000, "two")
        );
    }
}
