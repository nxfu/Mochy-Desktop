//! Small helpers shared by the query layer (clock + id generation).
//!
//! The upstream code reads the wall clock at write sites
//! (`LocalDateTime.now()`) and generates `"LA" + RandomStringUtils(8)`
//! ids (ArtistEntity.kt:51-53, PlaylistEntity.kt:44-46). The desktop passes
//! explicit timestamps (`now_ms`) so tests are deterministic; these helpers
//! provide the defaults.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Current time in epoch ms (true UTC — `System.currentTimeMillis`).
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// (year, month) of a true-UTC epoch-ms timestamp.
///
/// Upstream: `LocalDateTime.now().atOffset(ZoneOffset.UTC)` then
/// `.year`/`.monthValue` (DatabaseDao.kt:1466-1476). Civil-from-days
/// algorithm (Howard Hinnant's) — no chrono dependency for one function.
pub fn year_month_utc(ms: i64) -> (i32, i32) {
    let days = ms.div_euclid(86_400_000);
    // days -> civil date (Howard Hinnant, "chrono-compatible low-level
    // date algorithms"), valid for the entire epoch-ms range.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    #[allow(clippy::let_and_return)]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let _ = d;
    (if m <= 2 { y + 1 } else { y } as i32, m as i32)
}

/// A monotonically increasing pseudo-random alphanumeric suffix for
/// generated artist ids, matching the `"LA" + 8 chars` shape of
/// `ArtistEntity.generateArtistId()` (ArtistEntity.kt:51-53).
pub fn random_artist_suffix() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0x9E37_79B9_7F4A_7C15);
    const ALPHABET: &[u8; 62] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut x = COUNTER.fetch_add(0x2545_F491_4F6C_DD1D, Ordering::Relaxed);
    // xorshift64* — same spirit as RandomStringUtils' SHA1PRNG bytes: the
    // id only needs to be unique, not cryptographically strong (upstream
    // uses `insecure()` explicitly).
    let mut out = [0u8; 8];
    for slot in out.iter_mut() {
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        let r = x.wrapping_mul(0x2545_F491_4F6C_DD1D);
        *slot = ALPHABET[(r % ALPHABET.len() as u64) as usize];
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Same shape for `PlaylistEntity.generatePlaylistId()`
/// (PlaylistEntity.kt:44-46): `"LP" + 8 chars`.
pub fn generate_playlist_id() -> String {
    format!("LP{}", random_artist_suffix())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn year_month_utc_matches_known_dates() {
        // 2026-11-20T00:00:00Z = 1795132800000 ms.
        assert_eq!(year_month_utc(1_795_132_800_000), (2026, 11));
        // 1970-01-01.
        assert_eq!(year_month_utc(0), (1970, 1));
        // 2000-02-29T12:00:00Z = 951825600000 (leap day).
        assert_eq!(year_month_utc(951_825_600_000), (2000, 2));
        // 2024-12-31T23:59:59Z = 1735689599000.
        assert_eq!(year_month_utc(1_735_689_599_000), (2024, 12));
        // 2025-01-01T00:00:00Z = 1735689600000.
        assert_eq!(year_month_utc(1_735_689_600_000), (2025, 1));
    }

    #[test]
    fn artist_suffix_shape() {
        let s = random_artist_suffix();
        assert_eq!(s.len(), 8);
        assert!(s.chars().all(|c| c.is_ascii_alphanumeric()));
    }
}
