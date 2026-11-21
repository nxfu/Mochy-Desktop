//! The ytcfg scrape — `READ_CONFIG_JS`
//! (ui/screens/settings/importsync/YouTubeLoginScreen.kt:68-80).
//!
//! Upstream evaluates this in the signed-in music.youtube.com page:
//!
//! ```js
//! (function() {
//!   try {
//!     var get = function(k) {
//!       if (window.ytcfg && ytcfg.get) { var v = ytcfg.get(k); if (v) return v; }
//!       if (window.yt && yt.config_ && yt.config_[k]) return yt.config_[k];
//!       return null;
//!     };
//!     return { visitorData: get('VISITOR_DATA'), dataSyncId: get('DATASYNC_ID') };
//!   } catch (e) { return null; }
//! })();
//! ```
//!
//! and parses the JSON-encoded result: each field survives only when
//! it is a non-blank string other than the literal `"null"`
//! (YouTubeLoginScreen.kt:169-171 `optString(k).takeIf { it.isNotBlank()
//! && it != "null" }`). The `DATASYNC_ID` `"||"` normalization
//! (YouTubeLoginScreen.kt:177-182) is NOT applied to the persisted
//! value — the raw id goes into the store; App.kt's loader (the
//! session mirror, `normalize_data_sync_id`) normalizes on the way
//! into the session.
//!
//! wry's `evaluate_script_with_callback` delivers "the evaluation
//! result serialized into a JSON string" — for this script either a
//! JSON object or `null`. Some engines double-encode string results,
//! so a JSON string that itself parses as an object is unwrapped
//! once.

/// The script, verbatim upstream (YouTubeLoginScreen.kt:69-80).
pub const READ_CONFIG_JS: &str = r#"
(function() {
  try {
    var get = function(k) {
      if (window.ytcfg && ytcfg.get) { var v = ytcfg.get(k); if (v) return v; }
      if (window.yt && yt.config_ && yt.config_[k]) return yt.config_[k];
      return null;
    };
    return { visitorData: get('VISITOR_DATA'), dataSyncId: get('DATASYNC_ID') };
  } catch (e) { return null; }
})();
"#;

/// The session ids the scrape yields — each optional the way upstream's
/// `takeIf` filters them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct YtcfgIds {
    pub visitor_data: Option<String>,
    pub data_sync_id: Option<String>,
}

/// Parse the JSON-serialized script result
/// (YouTubeLoginScreen.kt:169-171).
pub fn parse_ytcfg_result(raw: &str) -> YtcfgIds {
    let mut value: serde_json::Value = match serde_json::from_str(raw) {
        Ok(value) => value,
        Err(_) => return YtcfgIds::default(),
    };
    // tolerate the double-encoded string form some engines produce
    if let serde_json::Value::String(text) = &value
        && let Ok(inner) = serde_json::from_str::<serde_json::Value>(text)
    {
        value = inner;
    }
    let serde_json::Value::Object(fields) = value else {
        return YtcfgIds::default();
    };
    YtcfgIds {
        visitor_data: take_field(fields.get("visitorData")),
        data_sync_id: take_field(fields.get("dataSyncId")),
    }
}

/// `optString(k).takeIf { it.isNotBlank() && it != "null" }` — a JSON
/// null, a non-string, a blank string or the literal `"null"` all read
/// as absent.
fn take_field(value: Option<&serde_json::Value>) -> Option<String> {
    let text = value?.as_str()?;
    if text.trim().is_empty() || text == "null" {
        return None;
    }
    Some(text.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// YouTubeLoginScreen.kt:169-171 — the takeIf filter, the null
    /// forms, the double-encoding tolerance.
    #[test]
    fn parses_the_recorded_result_shapes() {
        // the normal shape: both ids present
        assert_eq!(
            parse_ytcfg_result(r#"{"visitorData":"Cgt2aXNpdG9y","dataSyncId":"abc||"}"#),
            YtcfgIds {
                visitor_data: Some("Cgt2aXNpdG9y".into()),
                data_sync_id: Some("abc||".into()),
            }
        );

        // JS null (the try/catch's null return, or missing fields)
        assert_eq!(parse_ytcfg_result("null"), YtcfgIds::default());
        assert_eq!(parse_ytcfg_result("{}"), YtcfgIds::default());

        // the literal "null" string and blanks are absent
        assert_eq!(
            parse_ytcfg_result(r#"{"visitorData":"null","dataSyncId":""}"#),
            YtcfgIds::default()
        );
        assert_eq!(
            parse_ytcfg_result(r#"{"visitorData":"  ","dataSyncId":"ok"}"#),
            YtcfgIds {
                visitor_data: None,
                data_sync_id: Some("ok".into()),
            }
        );

        // non-string values are absent (ytcfg can hold objects)
        assert_eq!(
            parse_ytcfg_result(r#"{"visitorData":123,"dataSyncId":"sync"}"#),
            YtcfgIds {
                visitor_data: None,
                data_sync_id: Some("sync".into()),
            }
        );

        // a double-encoded string result is unwrapped once
        assert_eq!(
            parse_ytcfg_result(r#""{\"visitorData\":\"vd\",\"dataSyncId\":null}""#),
            YtcfgIds {
                visitor_data: Some("vd".into()),
                data_sync_id: None,
            }
        );

        // garbage is absent, never an error (the caller keeps the
        // already-valid cookie)
        assert_eq!(parse_ytcfg_result("not json"), YtcfgIds::default());
    }

    /// The script is the upstream one, byte-for-byte in the parts that
    /// matter (a regression guard against accidental rewrites).
    #[test]
    fn read_config_js_matches_upstream() {
        assert!(READ_CONFIG_JS.contains("ytcfg.get"));
        assert!(READ_CONFIG_JS.contains("yt.config_[k]"));
        assert!(READ_CONFIG_JS.contains("VISITOR_DATA"));
        assert!(READ_CONFIG_JS.contains("DATASYNC_ID"));
    }
}
