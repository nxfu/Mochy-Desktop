//! Song versions — the studio-swap and stand-in lookups:
//! `utils/SongVersions.kt` (the `songVersionId` and `otherUploads`
//! halves) plus the two title/artist normalizers it borrows from
//! `ui/player/CanvasArtworkUtils.kt` (their full home arrives with
//! the canvas artwork phase; the stream lookups need exactly these
//! two).
//!
//! Lists the app puts together itself (YouTube's feed rows, the
//! charts) play songs, not videos: a music video in one is swapped
//! for the song version, which brings the album cover with it. And
//! when YouTube refuses an upload from every client, the artist's
//! other upload of the same song usually still plays — the walk's
//! `orStandIn` step asks here for the candidates (≤ 2 are tried:
//! each stand-in costs another walk through every client).

use crate::youtube::YouTube;

/// `MAX_STAND_INS` (YTPlayerUtils.kt:1196-1197): each stand-in costs
/// another walk through every client, so only the closest few are
/// tried.
pub const MAX_STAND_INS: usize = 2;

/// `normalizeCanvasSongTitle` (CanvasArtworkUtils.kt:114-146).
pub(crate) fn normalize_song_title(raw: &str) -> String {
    let bracketed = regex::Regex::new(r"\s*\[[^]]*]").unwrap();
    let feat = regex::Regex::new(r"(?i)\s*\((?:feat\.?|ft\.?|featuring|with)\b[^)]*\)").unwrap();
    let label = regex::Regex::new(
        r"(?i)\s*\((?:official\s*)?(?:music\s*)?(?:video|mv|lyrics?|audio|visualizer|live|remaster(?:ed)?|version|edit|mix|remix)[^)]*\)",
    )
    .unwrap();
    let label_tail = regex::Regex::new(
        r"(?i)\s*-\s*(?:official\s*)?(?:music\s*)?(?:video|mv|lyrics?|audio|visualizer|live|remaster(?:ed)?|version|edit|mix|remix)\b.*$",
    )
    .unwrap();
    let spaces = regex::Regex::new(r"\s+").unwrap();
    let no_brackets = bracketed.replace_all(raw, "");
    let no_feat = feat.replace_all(&no_brackets, "");
    let no_labels = label.replace_all(&no_feat, "");
    let stripped = label_tail.replace_all(&no_labels, "");
    let collapsed = spaces.replace_all(stripped.trim(), " ");
    collapsed.trim().trim_matches('-').trim().to_owned()
}

/// `normalizeCanvasArtistName` (CanvasArtworkUtils.kt:148-160).
pub(crate) fn normalize_artist_name(raw: &str) -> String {
    let separators = regex::Regex::new(
        r"(?i)(?:\s*,\s*|\s*&\s*|\s+×\s+|\s+x\s+|\bfeat\.?\b|\bft\.?\b|\bfeaturing\b|\bwith\b)",
    )
    .unwrap();
    let spaces = regex::Regex::new(r"\s+").unwrap();
    let first = separators.split(raw).next().unwrap_or_default();
    spaces.replace_all(first.trim(), " ").trim().to_owned()
}

/// "Artist - Title" is how most video titles read
/// (SongVersions.kt:142-143).
fn strip_artist_prefix(title: &str, artist: &str) -> String {
    let pattern =
        regex::Regex::new(&format!(r"(?i)^\s*{}\s*[-–—|:]\s*", regex::escape(artist))).unwrap();
    pattern.replace(title, "").into_owned()
}

/// `sameTitle` (SongVersions.kt:145-147).
fn same_title(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
        || (a.len() >= 4
            && b.len() >= 4
            && (a.to_lowercase().contains(&b.to_lowercase())
                || b.to_lowercase().contains(&a.to_lowercase())))
}

/// `sameArtist` (SongVersions.kt:149-150).
fn same_artist(a: &str, b: &str) -> bool {
    !a.is_empty()
        && !b.is_empty()
        && (a.to_lowercase().contains(&b.to_lowercase())
            || b.to_lowercase().contains(&a.to_lowercase()))
}

/// Words that mean a different recording of the song, in its title or
/// its album's name (SongVersions.kt:153-155).
fn other_take_pattern() -> &'static regex::Regex {
    static PATTERN: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    PATTERN.get_or_init(|| {
        regex::Regex::new(
            r"(?i)\b(?:live|acoustic|karaoke|instrumental|cover|remix|sped ?up|slowed|reverb|nightcore|piano|demo|8d)\b",
        )
        .unwrap()
    })
}

/// `STAND_IN_LENGTH_SLACK_SECONDS` (SongVersions.kt:158): a longer
/// intro or outro would leave the lyrics out of step.
const STAND_IN_LENGTH_SLACK_SECONDS: i32 = 10;

/// One stand-in candidate: the artist's other upload of the same song
/// (another release, its music video), closest length first.
#[derive(Debug, Clone, PartialEq)]
pub struct StandIn {
    pub id: String,
    /// The stand-in is the song's music video (the refusal was on the
    /// studio upload): the playback data keeps the original details
    /// and reports `fromMusicVideo`.
    pub is_video_song: bool,
}

/// `songVersionId` (SongVersions.kt:65 + 95-108): the album version
/// of a music video known only by its name and artists, for lining
/// the video up with it. `None` when there is none.
pub async fn song_version_id(
    youtube: &YouTube,
    video_title: &str,
    artists: &str,
) -> Option<String> {
    find_song(youtube, video_title, artists)
        .await
        .map(|song| song.id)
}

/// `otherUploads` (SongVersions.kt:68-91): the artist's other uploads
/// of the same song, for when YouTube won't serve this one. The walk
/// tries at most [`MAX_STAND_INS`] of them.
pub async fn other_uploads(
    youtube: &YouTube,
    video_id: &str,
    title: &str,
    artists: &str,
    length_seconds: i32,
) -> Vec<StandIn> {
    let artist = normalize_artist_name(artists.trim_end_matches(" - Topic"));
    let name = normalize_song_title(&strip_artist_prefix(title, &artist));
    if name.len() < 2 || artist.len() < 2 {
        return Vec::new();
    }

    // both filters, in parallel like the upstream async pair
    let query = format!("{name} {artist}");
    let songs = crate::youtube::SearchFilter::song();
    let videos = crate::youtube::SearchFilter::video();
    let (song_results, video_results) = tokio::join!(
        youtube.search(&query, &songs),
        youtube.search(&query, &videos),
    );
    let mut found: Vec<crate::models::ytitem::SongItem> = Vec::new();
    for result in [song_results, video_results].into_iter().flatten() {
        found.extend(result.items.into_iter().filter_map(|item| match item {
            crate::models::ytitem::YTItem::Song(song) => Some(song),
            _ => None,
        }));
    }

    let wants_other_take = other_take_pattern().is_match(title);
    let mut candidates: Vec<crate::models::ytitem::SongItem> = found
        .into_iter()
        .filter(|song| song.id != video_id)
        // a longer intro or outro would leave the lyrics out of step
        .filter(|song| {
            song.duration
                .map(|duration| (duration - length_seconds).abs() <= STAND_IN_LENGTH_SLACK_SECONDS)
                .unwrap_or(false)
        })
        .filter(|song| {
            same_title(
                &normalize_song_title(&strip_artist_prefix(&song.title, &artist)),
                &name,
            )
        })
        // an upload from anyone but the artist is a lyric video, a
        // cover or a karaoke track
        .filter(|song| {
            song.artists
                .iter()
                .any(|song_artist| same_artist(&normalize_artist_name(&song_artist.name), &artist))
        })
        .filter(|song| {
            wants_other_take
                || !other_take_pattern().is_match(&format!(
                    "{} {}",
                    song.title,
                    song.album
                        .as_ref()
                        .map(|a| a.name.as_str())
                        .unwrap_or_default()
                ))
        })
        .collect();
    candidates.sort_by_key(|song| (song.duration.unwrap_or(0) - length_seconds).unsigned_abs());
    candidates.dedup_by(|a, b| a.id == b.id);
    candidates
        .into_iter()
        .map(|song| StandIn {
            is_video_song: song.is_video_song(),
            id: song.id,
        })
        .collect()
}

/// `findSong` (SongVersions.kt:95-108): the first song-filter hit by
/// the artist with the same title that is not a video song.
async fn find_song(
    youtube: &YouTube,
    video_title: &str,
    artists: &str,
) -> Option<crate::models::ytitem::SongItem> {
    let artist = normalize_artist_name(artists);
    let title = normalize_song_title(&strip_artist_prefix(video_title, &artist));
    if title.len() < 2 || artist.len() < 2 {
        return None;
    }
    let query = format!("{title} {artist}");
    let result = youtube
        .search(&query, &crate::youtube::SearchFilter::song())
        .await
        .ok()?;
    let songs: Vec<crate::models::ytitem::SongItem> = result
        .items
        .into_iter()
        .filter_map(|item| match item {
            crate::models::ytitem::YTItem::Song(song) => Some(song),
            _ => None,
        })
        .filter(|song| !song.is_video_song())
        .collect();
    songs.into_iter().find(|song| {
        same_title(&normalize_song_title(&song.title), &title)
            && song
                .artists
                .iter()
                .any(|song_artist| same_artist(&normalize_artist_name(&song_artist.name), &artist))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_video_titles() {
        assert_eq!(
            normalize_song_title("Artist - Song (Official Music Video)"),
            "Artist - Song"
        );
        assert_eq!(normalize_song_title("Song [Live at Wembley]"), "Song");
        assert_eq!(normalize_song_title("Song (feat. Someone)"), "Song");
        assert_eq!(normalize_song_title("Song - Remastered 2011"), "Song");
        assert_eq!(normalize_song_title("  Song   Name "), "Song Name");
    }

    #[test]
    fn normalizes_artist_lists_to_the_first() {
        assert_eq!(normalize_artist_name("A & B"), "A");
        assert_eq!(normalize_artist_name("A, B, C"), "A");
        assert_eq!(normalize_artist_name("A feat. B"), "A");
        assert_eq!(normalize_artist_name("A × B"), "A");
        assert_eq!(normalize_artist_name("  Spaced  Out "), "Spaced Out");
    }

    #[test]
    fn strips_the_artist_prefix() {
        assert_eq!(strip_artist_prefix("Artist - Song", "Artist"), "Song");
        assert_eq!(strip_artist_prefix("Artist – Song", "Artist"), "Song");
        // no prefix, no change
        assert_eq!(strip_artist_prefix("Just A Song", "Artist"), "Just A Song");
    }

    #[test]
    fn same_title_accepts_containment_over_four_chars() {
        // equality always matches, whatever the length
        assert!(same_title("ab", "ab"));
        assert!(same_title("Song Name", "song name"));
        // containment needs both sides at four characters
        assert!(same_title("Song Name (Remix)", "Song Name"));
        assert!(!same_title("abc", "abcd"));
    }

    #[test]
    fn other_take_words_are_detected() {
        assert!(other_take_pattern().is_match("Song (Live)"));
        assert!(other_take_pattern().is_match("Song - Sped Up"));
        assert!(other_take_pattern().is_match("8D Audio"));
        assert!(!other_take_pattern().is_match("Song"));
        assert!(!other_take_pattern().is_match("Liver Pool Anthem"));
    }
}
