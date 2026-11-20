//! Android import: the transfer-archive settings format and the raw
//! DataStore `preferences_pb` fallback.
//!
//! Upstream (importer/transfer/TransferArchive.kt):
//! - `settings/settings.json` — the DataStore as typed JSON:
//!   `{ "key": { "t": "b"|"i"|"l"|"f"|"d"|"s"|"ss"|"bytes", "v": ... } }`.
//! - `settings/prefs/<name>.json` — the carried SharedPreferences stores in
//!   the same typed-JSON shape.
//! - On import, login keys are DROPPED ("sign-ins and tokens stay behind",
//!   TransferArchive.kt:63-73) so the new app signs in again.
//!
//! The raw fallback reads an Android `files/datastore/settings.preferences_pb`
//! directly (hand-rolled protobuf-wire reader — the DataStore proto is
//! `Preferences { repeated Preference preferences = 1 }`,
//! `Preference { string key = 1; Value value = 2 }`,
//! `Value { oneof { bool boolean=1; int32 integer=2; int64 long=3;
//! float float=4; double double=5; string string=6; StringSet string_set=7;
//! bytes bytes=8 } }`, `StringSet { repeated string strings = 1 }`).

use std::collections::HashMap;

use serde_json::Value as Json;

use crate::{SettingValue, SettingsStore};

/// What the importer does with login keys (TransferArchive.kt:63-73: they
/// stay behind — the new app signs in again).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginKeyBehavior {
    /// Drop login keys, matching upstream import behavior.
    Drop,
    /// Keep them (used when migrating the app's own files in place, not from
    /// an archive).
    Keep,
}

/// The 20 login keys upstream refuses to carry (TransferArchive.kt:63-73,
/// by Android key name).
pub const LOGIN_KEYS: &[&str] = &[
    "innerTubeCookie",
    "visitorData",
    "dataSyncId",
    "accountName",
    "accountEmail",
    "accountChannelHandle",
    "discordAccessToken",
    "discordRefreshToken",
    "discordTokenExpiresAt",
    "discordLoginState",
    "discordLoginVerifier",
    "discordProfile",
    "discordRPCEnable",
    "lastfmSession",
    "lastfmUsername",
    "lastfmScrobblingEnable",
    "librefmSession",
    "librefmUsername",
    "librefmScrobblingEnable",
    "monochromePlaybackApiToken",
    "turnstileBypassToken",
];

/// Parse one typed-JSON entry (`{"t": "...", "v": ...}`) into a value
/// (TransferArchive.kt:120-128 `jsonIntoPreferences`).
fn entry_to_value(t: &str, v: &Json) -> Option<SettingValue> {
    match t {
        "b" => v.as_bool().map(SettingValue::Bool),
        "i" => v
            .as_i64()
            .and_then(|n| i32::try_from(n).ok())
            .map(SettingValue::Int),
        "l" => v.as_i64().map(SettingValue::Long),
        "f" | "d" => v.as_f64().map(|n| SettingValue::Float(n as f32)),
        "s" => v.as_str().map(|s| SettingValue::Str(s.to_owned())),
        "ss" => v.as_array().map(|items| {
            SettingValue::StringSet(
                items
                    .iter()
                    .filter_map(|i| i.as_str().map(str::to_owned))
                    .collect(),
            )
        }),
        "bytes" => v
            .as_str()
            .and_then(crate::store::base64_decode)
            .map(SettingValue::Bytes),
        _ => None,
    }
}

/// Parse a whole typed-JSON object (`{ key: {t, v} }`).
fn parse_typed_json(
    text: &str,
    logins: LoginKeyBehavior,
) -> Result<HashMap<String, SettingValue>, crate::ConfigError> {
    let map: serde_json::Map<String, Json> = serde_json::from_str(text)
        .map_err(|e| crate::ConfigError::Corrupt(format!("bad typed json: {e}")))?;
    let mut out = HashMap::with_capacity(map.len());
    for (key, entry) in map {
        let Some(entry) = entry.as_object() else {
            continue;
        };
        let t = entry.get("t").and_then(Json::as_str).unwrap_or_default();
        let Some(v) = entry.get("v") else { continue };
        if logins == LoginKeyBehavior::Drop && LOGIN_KEYS.contains(&key.as_str()) {
            continue;
        }
        if let Some(value) = entry_to_value(t, v) {
            out.insert(key, value);
        }
    }
    Ok(out)
}

/// Import an Android transfer-archive `settings/settings.json` into a store.
/// Keys map 1:1 onto the registry by their Android names; unknown keys are
/// preserved verbatim (forward compatibility); login keys are dropped per
/// upstream behavior. Returns the number of keys imported.
pub fn import_archive_settings_json(
    store: &SettingsStore,
    text: &str,
    logins: LoginKeyBehavior,
) -> Result<usize, crate::ConfigError> {
    let values = parse_typed_json(text, logins)?;
    let count = values.len();
    let mut snapshot = store.snapshot();
    snapshot.values.extend(values);
    store.replace_settings(snapshot);
    Ok(count)
}

/// Import one transfer-archive prefs file (`settings/prefs/<name>.json`)
/// into the matching config section. Only the 6 carried stores exist in
/// archives, but any store name is accepted (merged into its section).
pub fn import_archive_prefs_json(
    store: &SettingsStore,
    store_name: &str,
    text: &str,
) -> Result<usize, crate::ConfigError> {
    let values = parse_typed_json(text, LoginKeyBehavior::Drop)?;
    let count = values.len();
    let mut snapshot = store.snapshot();
    snapshot.prefs_section_mut(store_name).extend(values);
    store.replace_settings(snapshot);
    Ok(count)
}

// ---------------------------------------------------------------------------
// Raw preferences_pb fallback reader (Android DataStore protobuf).
// ---------------------------------------------------------------------------

/// A parsed raw DataStore preference map.
#[derive(Debug, Default)]
pub struct RawPreferences {
    pub values: HashMap<String, SettingValue>,
}

/// Parse `settings.preferences_pb` (the Android DataStore file). Unknown
/// fields are skipped, so future proto additions degrade gracefully.
pub fn parse_preferences_pb(bytes: &[u8]) -> Result<RawPreferences, crate::ConfigError> {
    let mut values = HashMap::new();
    let mut rest = bytes;
    while let Some((field, wire, payload, tail)) = read_field(rest) {
        rest = tail;
        if field == 1 && wire == 2 {
            // repeated Preference
            if let Some((key, value)) = parse_preference(payload) {
                values.insert(key, value);
            }
        }
        // unknown top-level fields: skip (proto3 forward compat)
    }
    Ok(RawPreferences { values })
}

/// `Preference { string key = 1; Value value = 2 }`.
fn parse_preference(mut bytes: &[u8]) -> Option<(String, SettingValue)> {
    let mut key = None;
    let mut value = None;
    while let Some((field, wire, payload, tail)) = read_field(bytes) {
        bytes = tail;
        match (field, wire) {
            (1, 2) => key = Some(String::from_utf8_lossy(payload).into_owned()),
            (2, 2) => value = parse_value(payload),
            _ => {}
        }
    }
    key.zip(value)
}

/// `Value { oneof ... }` — exactly the androidx datastore-preferences proto.
fn parse_value(mut bytes: &[u8]) -> Option<SettingValue> {
    let mut out = None;
    while let Some((field, wire, payload, tail)) = read_field(bytes) {
        bytes = tail;
        let parsed = match (field, wire) {
            (1, 0) => read_varint(payload).map(|(v, _)| SettingValue::Bool(v != 0)),
            (2, 0) => read_varint(payload).map(|(v, _)| SettingValue::Int(v as i32)),
            (3, 0) => read_varint(payload).map(|(v, _)| SettingValue::Long(v as i64)),
            (4, 5) => read_f32(payload).map(SettingValue::Float),
            (5, 1) => read_f64(payload).map(|v| SettingValue::Float(v as f32)),
            (6, 2) => Some(SettingValue::Str(
                String::from_utf8_lossy(payload).into_owned(),
            )),
            (7, 2) => parse_string_set(payload).map(SettingValue::StringSet),
            (8, 2) => Some(SettingValue::Bytes(payload.to_vec())),
            _ => None,
        };
        if let Some(value) = parsed {
            out = Some(value);
        }
    }
    out
}

/// `StringSet { repeated string strings = 1 }`.
fn parse_string_set(mut bytes: &[u8]) -> Option<Vec<String>> {
    let mut out = Vec::new();
    while let Some((field, wire, payload, tail)) = read_field(bytes) {
        bytes = tail;
        if field == 1 && wire == 2 {
            out.push(String::from_utf8_lossy(payload).into_owned());
        }
    }
    Some(out)
}

/// Read one protobuf field header + payload. Returns
/// `(field_number, wire_type, payload, remaining)`.
fn read_field(bytes: &[u8]) -> Option<(u32, u8, &[u8], &[u8])> {
    let (tag, rest) = read_varint(bytes)?;
    let field = (tag >> 3) as u32;
    let wire = (tag & 7) as u8;
    match wire {
        0 => {
            // payload = the value varint only (the tag is already consumed)
            let (_value, tail) = read_varint(rest)?;
            let len = rest.len() - tail.len();
            Some((field, wire, &rest[..len], tail))
        }
        1 => {
            if rest.len() < 8 {
                return None;
            }
            Some((field, wire, &rest[..8], &rest[8..]))
        }
        2 => {
            let (len, rest) = read_varint(rest)?;
            let len = len as usize;
            if rest.len() < len {
                return None;
            }
            Some((field, wire, &rest[..len], &rest[len..]))
        }
        5 => {
            if rest.len() < 4 {
                return None;
            }
            Some((field, wire, &rest[..4], &rest[4..]))
        }
        _ => None, // groups (3/4) are not used by this proto
    }
}

fn read_varint(bytes: &[u8]) -> Option<(u64, &[u8])> {
    let mut value = 0u64;
    for (i, &b) in bytes.iter().enumerate() {
        if i > 9 {
            return None;
        }
        value |= u64::from(b & 0x7f) << (7 * i);
        if b & 0x80 == 0 {
            return Some((value, &bytes[i + 1..]));
        }
    }
    None
}

fn read_f32(bytes: &[u8]) -> Option<f32> {
    bytes.try_into().ok().map(f32::from_le_bytes)
}

fn read_f64(bytes: &[u8]) -> Option<f64> {
    bytes.try_into().ok().map(f64::from_le_bytes)
}

/// Encode a `preferences_pb` blob (test helper + re-export path for the
/// desktop export feature; mirrors the proto above).
pub fn encode_preferences_pb(values: &HashMap<String, SettingValue>) -> Vec<u8> {
    let mut out = Vec::new();
    for (key, value) in values {
        let mut preference = Vec::new();
        emit_field(&mut preference, 1, 2, key.as_bytes());
        let mut value_bytes = Vec::new();
        match value {
            SettingValue::Bool(v) => emit_varint_field(&mut value_bytes, 1, *v as u64),
            SettingValue::Int(v) => emit_varint_field(&mut value_bytes, 2, *v as u64),
            SettingValue::Long(v) => emit_varint_field(&mut value_bytes, 3, *v as u64),
            SettingValue::Float(v) => emit_fixed32_field(&mut value_bytes, 4, v.to_bits()),
            SettingValue::Str(v) => emit_field(&mut value_bytes, 6, 2, v.as_bytes()),
            SettingValue::StringSet(items) => {
                let mut set = Vec::new();
                for item in items {
                    emit_field(&mut set, 1, 2, item.as_bytes());
                }
                emit_field(&mut value_bytes, 7, 2, &set);
            }
            SettingValue::Bytes(v) => emit_field(&mut value_bytes, 8, 2, v),
        }
        emit_field(&mut preference, 2, 2, &value_bytes);
        emit_field(&mut out, 1, 2, &preference);
    }
    out
}

fn emit_field(out: &mut Vec<u8>, field: u32, wire: u8, payload: &[u8]) {
    emit_varint(out, (u64::from(field) << 3) | u64::from(wire));
    if wire == 2 {
        emit_varint(out, payload.len() as u64);
    }
    out.extend_from_slice(payload);
}

fn emit_varint_field(out: &mut Vec<u8>, field: u32, value: u64) {
    emit_varint(out, u64::from(field) << 3);
    emit_varint(out, value);
}

fn emit_fixed32_field(out: &mut Vec<u8>, field: u32, bits: u32) {
    emit_varint(out, (u64::from(field) << 3) | 5);
    out.extend_from_slice(&bits.to_le_bytes());
}

fn emit_varint(out: &mut Vec<u8>, mut value: u64) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            break;
        }
        out.push(byte | 0x80);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SettingId;
    use crate::audio_lab::AudioLab;

    /// Build a synthetic archive settings.json in the upstream typed format.
    fn synthetic_archive() -> String {
        let mut map = serde_json::Map::new();
        let mut put = |key: &str, t: &str, v: Json| {
            map.insert(key.to_owned(), serde_json::json!({ "t": t, "v": v }));
        };
        put("persistentQueue", "b", true.into());
        put("maxSongCacheSize", "i", 2048.into());
        put("last_full_sync", "l", 1_760_000_000_000i64.into());
        put("crossfadeDuration", "f", 7.5.into());
        put("audioQuality", "s", "HIGH".into());
        put(
            "selectedYtmPlaylists",
            "ss",
            serde_json::json!(["PL1", "PL2"]),
        );
        // a login key that must be dropped on import:
        put("innerTubeCookie", "s", "SECRET=1".into());
        r#"{"persistentQueue":{"t":"b","v":true},"maxSongCacheSize":{"t":"i","v":2048},"last_full_sync":{"t":"l","v":1760000000000},"crossfadeDuration":{"t":"f","v":7.5},"audioQuality":{"t":"s","v":"HIGH"},"selectedYtmPlaylists":{"t":"ss","v":["PL1","PL2"]},"innerTubeCookie":{"t":"s","v":"SECRET=1"}}"#.to_owned()
    }

    #[test]
    fn synthetic_archive_maps_keys_1_to_1_with_zero_unknown() {
        let store = SettingsStore::with_defaults();
        let count =
            import_archive_settings_json(&store, &synthetic_archive(), LoginKeyBehavior::Drop)
                .unwrap();
        assert_eq!(count, 6);
        let snap = store.snapshot();
        // every imported key resolves to a registry row -> zero unknown keys
        assert_eq!(snap.unknown_keys(), Vec::<&str>::new());
        // typed values landed
        assert!(snap.PersistentQueue());
        assert_eq!(snap.MaxSongCacheSize(), 2048);
        assert_eq!(snap.LastFullSync(), 1_760_000_000_000);
        assert_eq!(snap.CrossfadeDuration(), 7.5);
        assert_eq!(snap.AudioQuality(), crate::AudioQuality::HIGH);
        assert_eq!(
            snap.raw("selectedYtmPlaylists"),
            Some(&SettingValue::StringSet(vec![
                "PL1".to_owned(),
                "PL2".to_owned()
            ]))
        );
        // the login value stayed behind: the row reads as its (empty)
        // default, never as the secret the archive carried
        assert_ne!(
            snap.raw("innerTubeCookie"),
            Some(&SettingValue::Str("SECRET=1".to_owned()))
        );
        assert_eq!(
            snap.raw("innerTubeCookie"),
            Some(&SettingValue::Str(String::new()))
        );
    }

    #[test]
    fn login_keys_list_matches_upstream() {
        // TransferArchive.kt:63-73 — 21 keys by name
        assert_eq!(LOGIN_KEYS.len(), 21);
        assert!(LOGIN_KEYS.contains(&"innerTubeCookie"));
        assert!(LOGIN_KEYS.contains(&"turnstileBypassToken"));
        assert!(LOGIN_KEYS.contains(&"discordRPCEnable"));
    }

    #[test]
    fn prefs_store_section_import() {
        let store = SettingsStore::with_defaults();
        let text =
            r#"{"eq":{"t":"s","v":"{\"enabled\":true}"},"migrated_legacy_eq":{"t":"b","v":true}}"#;
        let count = import_archive_prefs_json(&store, "audio_lab", text).unwrap();
        assert_eq!(count, 2);
        let lab = AudioLab::from_settings(&store.snapshot());
        assert!(lab.eq.enabled);
        assert!(lab.migrated_legacy_eq);
    }

    #[test]
    fn preferences_pb_round_trip() {
        let mut values = HashMap::new();
        values.insert("isFirstRun".to_owned(), SettingValue::Bool(false));
        values.insert("maxImageCacheSize".to_owned(), SettingValue::Int(256));
        values.insert("last_full_sync".to_owned(), SettingValue::Long(123_456_789));
        values.insert("crossfadeDuration".to_owned(), SettingValue::Float(6.25));
        values.insert(
            "audioQuality".to_owned(),
            SettingValue::Str("LOW".to_owned()),
        );
        values.insert(
            "selectedYtmPlaylists".to_owned(),
            SettingValue::StringSet(vec!["a".to_owned(), "b".to_owned()]),
        );
        let bytes = encode_preferences_pb(&values);
        let parsed = parse_preferences_pb(&bytes).unwrap();
        assert_eq!(parsed.values.len(), 6);
        assert_eq!(
            parsed.values.get("isFirstRun"),
            Some(&SettingValue::Bool(false))
        );
        assert_eq!(
            parsed.values.get("maxImageCacheSize"),
            Some(&SettingValue::Int(256))
        );
        assert_eq!(
            parsed.values.get("last_full_sync"),
            Some(&SettingValue::Long(123_456_789))
        );
        assert_eq!(
            parsed.values.get("crossfadeDuration"),
            Some(&SettingValue::Float(6.25))
        );
        assert_eq!(
            parsed.values.get("audioQuality"),
            Some(&SettingValue::Str("LOW".to_owned()))
        );
        assert_eq!(
            parsed.values.get("selectedYtmPlaylists"),
            Some(&SettingValue::StringSet(vec![
                "a".to_owned(),
                "b".to_owned()
            ]))
        );
    }

    #[test]
    fn pb_parse_survives_garbage() {
        assert!(parse_preferences_pb(&[]).is_ok());
        assert!(parse_preferences_pb(&[0xff]).is_ok()); // truncated varint
        assert!(parse_preferences_pb(&[0x0a, 0xff]).is_ok()); // bad length
    }

    #[test]
    fn static_key_count_at_least_395() {
        // Verified correction (PORT_SPEC §2): 398 static keys upstream; the
        // registry folds the one alias row, so >= 395 with margin.
        let count = SettingId::COUNT;
        println!("mochy-config static registry key count: {count}");
        assert!(count >= 395, "registry has only {count} keys");
    }

    #[test]
    fn every_row_round_trips_through_typed_json() {
        // encode/decode every default through the archive format
        for id in SettingId::ALL {
            let entry = crate::store::value_to_typed_entry(&id.default_value());
            let json = serde_json::to_string(&entry).unwrap();
            let parsed: crate::store::TypedEntry = serde_json::from_str(&json).unwrap();
            let back = crate::store::typed_entry_to_value(&parsed)
                .unwrap_or_else(|| panic!("entry decode failed for {}", id.key_name()));
            assert_eq!(
                back,
                id.default_value(),
                "round trip failed for {}",
                id.key_name()
            );
        }
    }
}
