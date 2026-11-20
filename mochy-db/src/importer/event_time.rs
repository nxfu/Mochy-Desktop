//! Transform (a): the Android wall-clock-as-UTC timestamp fix.
//!
//! Upstream `Converters.kt` maps `LocalDateTime` to epoch ms through
//! `ZoneOffset.UTC`, and every write site builds the value with
//! `LocalDateTime.now()` — device-local WALL CLOCK. The stored number is
//! therefore "local time pretending to be UTC": for a UTC+8 device, a play
//! at 20:00 local (12:00 true UTC) is stored as 20:00-as-UTC.
//!
//! Android reads it back through the same converter (UTC) and formats the
//! `LocalDateTime` directly, so the errors cancel ON the device. The port
//! stores TRUE UTC (blueprint ch. 7.2: recap month boundaries otherwise
//! shift with the user's timezone), so the importer shifts every
//! converter-stamped column once:
//!
//! `true_utc_ms = stored_ms - offset_minutes * 60_000`
//!
//! (stored = wall clock = true UTC + offset, hence the subtraction).
//!
//! The one column that is ALREADY true UTC upstream —
//! `speed_dial_item.createDate` (`System.currentTimeMillis()`,
//! SpeedDialItem.kt:57) — is excluded.
//!
//! Decision Q4 (recorded in HANDOFF Phase 03): the shift uses ONE fixed
//! offset for the whole import (the device zone at import time), not a DST
//! rules engine — the Android DB never recorded a zone, so per-instant
//! offsets are unknowable; worst case error is one DST hour, never a day.

/// Columns that carry the Room `LocalDateTime` converter (wall clock as
/// UTC) and must be shifted on import. Derived from the `@TypeConverters`
/// field types in `db/entities/*.kt`.
pub const WALL_CLOCK_AS_UTC_COLUMNS: &[(&str, &str)] = &[
    // song: date, dateModified, likedDate, inLibrary, dateDownload
    ("song", "date"),
    ("song", "dateModified"),
    ("song", "likedDate"),
    ("song", "inLibrary"),
    ("song", "dateDownload"),
    // artist: lastUpdateTime, bookmarkedAt
    ("artist", "lastUpdateTime"),
    ("artist", "bookmarkedAt"),
    // album: lastUpdateTime, bookmarkedAt, likedDate, inLibrary
    ("album", "lastUpdateTime"),
    ("album", "bookmarkedAt"),
    ("album", "likedDate"),
    ("album", "inLibrary"),
    // playlist: createdAt, lastUpdateTime, bookmarkedAt
    ("playlist", "createdAt"),
    ("playlist", "lastUpdateTime"),
    ("playlist", "bookmarkedAt"),
    // event: timestamp (the one the blueprint calls out — recap boundaries)
    ("event", "timestamp"),
    // playlist_event: timestamp (same `LocalDateTime.now()` write shape)
    ("playlist_event", "timestamp"),
    // recognition_history: recognizedAt
    ("recognition_history", "recognizedAt"),
    // import bookkeeping
    ("imported_song", "importedAt"),
    ("import_session", "startedAt"),
    ("imported_playlist", "importedAt"),
    ("imported_playlist", "lastSyncedAt"),
    // NOT here: speed_dial_item.createDate (true UTC upstream already).
];

/// Whether `(table, column)` needs the wall-clock shift.
pub fn is_wall_clock_as_utc(table: &str, column: &str) -> bool {
    WALL_CLOCK_AS_UTC_COLUMNS
        .iter()
        .any(|(t, c)| *t == table && *c == column)
}

/// Shifts one stored value to true UTC.
///
/// `offset_minutes` is the device zone's offset EAST of UTC (e.g. +330 for
/// IST, +480 for UTC+8, -300 for EST).
pub fn normalize(stored_ms: i64, offset_minutes: i32) -> i64 {
    stored_ms - (offset_minutes as i64) * 60_000
}

/// Formats an offset as `+HH:MM` / `-HH:MM` for the import record.
pub fn offset_label(offset_minutes: i32) -> String {
    let sign = if offset_minutes < 0 { '-' } else { '+' };
    let abs = offset_minutes.unsigned_abs();
    format!("{sign}{:02}:{:02}", abs / 60, abs % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_plus_8_shifts_back_eight_hours() {
        // A play at 2026-11-19T12:00:00Z on a UTC+8 device shows 20:00
        // wall clock; stored as 20:00-as-UTC = 1795118400000.
        let stored = 1_795_118_400_000;
        // True UTC = 12:00Z = 1795089600000.
        assert_eq!(normalize(stored, 480), 1_795_089_600_000);
    }

    #[test]
    fn negative_offset_shifts_forward() {
        // EST (-5): a play at 12:00Z shows 07:00 wall clock, stored as
        // 07:00-as-UTC; true UTC is 5h ahead of the stored value.
        let stored = 1_795_089_600_000;
        assert_eq!(normalize(stored, -300), stored + 300 * 60_000);
    }

    #[test]
    fn zero_offset_is_identity() {
        assert_eq!(normalize(123_456, 0), 123_456);
    }

    #[test]
    fn speed_dial_create_date_is_excluded() {
        assert!(!is_wall_clock_as_utc("speed_dial_item", "createDate"));
        assert!(is_wall_clock_as_utc("event", "timestamp"));
    }

    #[test]
    fn labels() {
        assert_eq!(offset_label(480), "+08:00");
        assert_eq!(offset_label(330), "+05:30");
        assert_eq!(offset_label(-300), "-05:00");
        assert_eq!(offset_label(0), "+00:00");
    }
}
