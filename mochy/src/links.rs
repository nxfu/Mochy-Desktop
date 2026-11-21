//! YouTube link / share parsing — the pure half of the deep-link router.
//!
//! Upstream (Android, pin `bde01cb4`): `MainActivity.kt` handles deep
//! links and shares in `importSharedAudio` (1565-1595) and
//! `handleDeepLinkIntent` (1597-1684). The cited range receives either an
//! intent `data` URI or a shared-text URI (`Intent.EXTRA_TEXT.toUri()`),
//! then dispatches on the FIRST path segment:
//!
//! ```kotlin
//! // MainActivity.kt:1606-1684 (condensed)
//! when (val path = uri.pathSegments.firstOrNull()) {
//!     "playlist" -> uri.getQueryParameter("list")?.let { playlistId ->
//!         if (playlistId.startsWith("OLAK5uy_")) /* resolve album, navigate */ else navigate("online_playlist/$playlistId") }
//!     "browse"   -> uri.lastPathSegment?.let { navigate("album/$browseId") }
//!     "channel", "c" -> uri.lastPathSegment?.let { navigate("artist/$artistId") }
//!     "search"   -> uri.getQueryParameter("q")?.let { navigate(searchRoute(it)) }
//!     else -> {
//!         val videoId = when {
//!             path == "watch" -> uri.getQueryParameter("v")
//!             uri.host == "youtu.be" -> uri.pathSegments.firstOrNull()
//!             else -> null
//!         }
//!         val playlistId = uri.getQueryParameter("list")
//!         if (videoId != null) /* play queue(videoId, playlistId) */
//!         else if (playlistId != null) /* play queue(null, playlistId) */
//!     }
//! }
//! ```
//!
//! The OS intent filters (AndroidManifest.xml:138-188) register
//! `youtube.com`/`m.`/`www.`/`music.` (path prefixes /v/, /embed/,
//! /watch, /channel/, /user/, /c/, /playlist, /browse/, /search),
//! `youtu.be` (any path) and the `vnd.youtube{,.launch}` schemes; the
//! handler itself is host-agnostic (shared text is any URI), so this
//! parser is too — the OS-level registration decides what reaches it
//! (Phase 14 wires `mochy://` + file associations the same way).
//!
//! Desktop mapping of the share half (`importSharedAudio`): files sent
//! by another app or the file manager arrive as paths (command-line
//! args / open-with), and import as local songs — the actual local
//! import is Phase 22's `mochy-importer` scope; here they parse into
//! [`ParsedLink::SharedAudio`].
//!
//! Port decisions (upstream-wins conflicts recorded in HANDOFF ## Phase 07):
//! - `vnd.youtube` / `vnd.youtube.launch` URIs never fire upstream (they
//!   are opaque Android URIs: `pathSegments` is empty and `host` is
//!   null, so the `else` arm finds no video id). The manifest declares
//!   the schemes with `MEDIA_PLAY_FROM_SEARCH`, the port honors that
//!   declared intent: the scheme-specific part is the video id.
//! - The handler ignores `/v/`, `/embed/` and `/user/` links (only the
//!   `list` parameter of those would act) — parity, kept and pinned by
//!   tests.
//! - Hosts compare case-insensitively (RFC 3986; the `url` crate
//!   normalizes, Android `Uri.host` does not).
//!
//! Window/router wiring (navigation, queue start) is NOT here — Phase 10
//! consumes [`parse`] for deep links, Phase 22 the shared-audio half.

use url::Url;

/// What a handed-over link or share resolves to — one variant per
/// upstream navigation/play decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedLink {
    /// `playlist` path with an `OLAK5uy_` list id — upstream resolves
    /// the album the auto-playlist was generated from
    /// (`YouTube.albumSongs(playlistId)` → first song's album browse id)
    /// and navigates there.
    AlbumFromAutoPlaylist { playlist_id: String },
    /// `playlist` path with any other list id → the online playlist page.
    OnlinePlaylist { playlist_id: String },
    /// `browse` path → the album page for the browse id.
    Album { browse_id: String },
    /// `channel` / `c` path → the artist page.
    Artist { artist_id: String },
    /// `search` path + `q` → the search route.
    Search { query: String },
    /// A video id (watch page, youtu.be, vnd.youtube) with an optional
    /// playlist context — upstream starts
    /// `YouTubeQueue(WatchEndpoint(videoId, playlistId), first)`.
    PlayVideo {
        video_id: String,
        playlist_id: Option<String>,
    },
    /// No video id but a `list` parameter — upstream starts the playlist
    /// queue (`YouTube.queue(null, playlistId)`).
    PlayPlaylist { playlist_id: String },
    /// Files shared into the app (`importSharedAudio`,
    /// MainActivity.kt:1565-1595): imported as local songs later.
    SharedAudio { paths: Vec<String> },
}

/// The schemes the manifest registers for the bare YouTube player
/// intent filter (AndroidManifest.xml:176-188).
const VND_SCHEMES: [&str; 2] = ["vnd.youtube", "vnd.youtube.launch"];

/// `importSharedAudio` (MainActivity.kt:1565-1595): `Some` iff there is
/// at least one stream to import (upstream bails out on an empty
/// `EXTRA_STREAM` list; every shared file is attempted, whatever its
/// extension — filtering happens inside the local importer).
pub fn parse_shared_files(paths: &[String]) -> Option<ParsedLink> {
    if paths.is_empty() {
        None
    } else {
        Some(ParsedLink::SharedAudio {
            paths: paths.to_vec(),
        })
    }
}

/// `handleDeepLinkIntent`'s URI half (MainActivity.kt:1606-1684): parse
/// a data URI or shared-text URI into the navigation/play decision.
/// `None` = nothing upstream would do (including an unparseable string).
pub fn parse(uri_text: &str) -> Option<ParsedLink> {
    let text = uri_text.trim();
    if text.is_empty() {
        return None;
    }
    let url = Url::parse(text).ok()?;

    // The port's vnd.youtube extension (see the module docs): the
    // scheme-specific part is the video id, in either the opaque
    // (`vnd.youtube:ID`) or hierarchical (`vnd.youtube://ID`) form.
    if VND_SCHEMES.contains(&url.scheme()) {
        return vnd_video_id(&url).map(|video_id| ParsedLink::PlayVideo {
            video_id,
            playlist_id: query_param(&url, "list"),
        });
    }

    let path_segments = path_segments(&url);
    let path: Option<&str> = path_segments.first().map(String::as_str);

    match path {
        // MainActivity.kt:1610-1623
        Some("playlist") => {
            let playlist_id = query_param(&url, "list")?;
            if playlist_id.starts_with("OLAK5uy_") {
                Some(ParsedLink::AlbumFromAutoPlaylist { playlist_id })
            } else {
                Some(ParsedLink::OnlinePlaylist { playlist_id })
            }
        }
        // MainActivity.kt:1625-1627
        Some("browse") => {
            let browse_id = last_path_segment(&url)?;
            Some(ParsedLink::Album { browse_id })
        }
        // MainActivity.kt:1629-1631
        Some("channel") | Some("c") => {
            let artist_id = last_path_segment(&url)?;
            Some(ParsedLink::Artist { artist_id })
        }
        // MainActivity.kt:1633-1638
        Some("search") => {
            let query = query_param(&url, "q")?;
            Some(ParsedLink::Search { query })
        }
        // MainActivity.kt:1639-1684
        _ => {
            let video_id = match path {
                // path == "watch" -> the v parameter
                Some("watch") => query_param(&url, "v"),
                // host == "youtu.be" -> the first path segment
                _ if url.host_str() == Some("youtu.be") && !path_segments.is_empty() => {
                    Some(path_segments[0].clone())
                }
                _ => None,
            };
            let playlist_id = query_param(&url, "list");
            match (video_id, playlist_id) {
                (Some(video_id), playlist_id) => Some(ParsedLink::PlayVideo {
                    video_id,
                    playlist_id,
                }),
                (None, Some(playlist_id)) => Some(ParsedLink::PlayPlaylist { playlist_id }),
                (None, None) => None,
            }
        }
    }
}

/// Decoded path segments (`android.net.Uri.getPathSegments`) —
/// percent-decoded, skipping the empty segments a leading/double slash
/// produces (Android's PathSplitter behavior; the `url` crate keeps
/// empty segments for `//`, so filter them like Android does).
fn path_segments(url: &Url) -> Vec<String> {
    url.path_segments()
        .map(|segments| {
            segments
                .filter(|segment| !segment.is_empty())
                .map(percent_decode)
                .collect()
        })
        .unwrap_or_default()
}

/// `android.net.Uri.getLastPathSegment` — the final path segment,
/// percent-decoded, with empty segments skipped (the port's
/// normalization of trailing/double slashes; the behavior is pinned by
/// tests — a bare `/browse` navigates to `album/browse` exactly like
/// upstream, and `/browse/` normalizes to the same quirk).
fn last_path_segment(url: &Url) -> Option<String> {
    let segments = path_segments(url);
    segments.last().cloned()
}

/// `android.net.Uri.getQueryParameter(name)` — form-style decoding
/// (`+` is space, `%XX` decoded), first occurrence wins.
fn query_param(url: &Url, name: &str) -> Option<String> {
    url.query_pairs()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.into_owned())
}

/// The `vnd.youtube` scheme-specific part: the path of the opaque form
/// (`vnd.youtube:ID`), the host of the hierarchical form
/// (`vnd.youtube://ID`), query stripped; `?t=`-style fragments ignored.
fn vnd_video_id(url: &Url) -> Option<String> {
    if url.cannot_be_a_base() {
        let id = url.path().trim_start_matches('/');
        (!id.is_empty()).then(|| id.to_owned())
    } else {
        let host = url.host_str()?;
        let path = url.path().trim_matches('/');
        if path.is_empty() {
            Some(host.to_owned())
        } else {
            Some(path.to_owned())
        }
    }
}

/// Percent-decode a path segment the way Android's
/// `Uri.getPathSegments` does (decode each segment, keep `+` literal —
/// only query values treat `+` as space).
fn percent_decode(segment: &str) -> String {
    let bytes = segment.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' if index + 2 < bytes.len() => {
                let hi = hex_val(bytes[index + 1]);
                let lo = hex_val(bytes[index + 2]);
                match (hi, lo) {
                    (Some(hi), Some(lo)) => {
                        out.push(hi << 4 | lo);
                        index += 3;
                    }
                    _ => {
                        out.push(b'%');
                        index += 1;
                    }
                }
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every dispatch arm of MainActivity.kt:1606-1684 plus the manifest
    /// surfaces, table-driven. `(input, expected)`; `expected == None`
    /// pins an upstream "does nothing" quirk.
    #[test]
    fn link_table_covers_every_upstream_case() {
        let video = "dQw4w9WgXcQ".to_owned();
        let list = "PLabc123".to_owned();
        let table: &[(&str, Option<ParsedLink>)] = &[
            // ---- watch pages (all four manifest hosts) ----
            (
                "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
                Some(ParsedLink::PlayVideo {
                    video_id: video.clone(),
                    playlist_id: None,
                }),
            ),
            (
                "https://music.youtube.com/watch?v=dQw4w9WgXcQ&list=PLabc123",
                Some(ParsedLink::PlayVideo {
                    video_id: video.clone(),
                    playlist_id: Some(list.clone()),
                }),
            ),
            (
                "http://m.youtube.com/watch?v=dQw4w9WgXcQ",
                Some(ParsedLink::PlayVideo {
                    video_id: video.clone(),
                    playlist_id: None,
                }),
            ),
            (
                "https://youtube.com/watch?v=dQw4w9WgXcQ&list=PLabc123&index=2",
                Some(ParsedLink::PlayVideo {
                    video_id: video.clone(),
                    playlist_id: Some(list.clone()),
                }),
            ),
            // ---- youtu.be short links ----
            (
                "https://youtu.be/dQw4w9WgXcQ",
                Some(ParsedLink::PlayVideo {
                    video_id: video.clone(),
                    playlist_id: None,
                }),
            ),
            (
                "https://youtu.be/dQw4w9WgXcQ?list=PLabc123",
                Some(ParsedLink::PlayVideo {
                    video_id: video.clone(),
                    playlist_id: Some(list.clone()),
                }),
            ),
            // ---- playlists ----
            (
                "https://www.youtube.com/playlist?list=PLabc123",
                Some(ParsedLink::OnlinePlaylist {
                    playlist_id: list.clone(),
                }),
            ),
            (
                "https://music.youtube.com/playlist?list=OLAK5uy_lX9Kq",
                Some(ParsedLink::AlbumFromAutoPlaylist {
                    playlist_id: "OLAK5uy_lX9Kq".into(),
                }),
            ),
            // playlist path without a list parameter: upstream does nothing
            ("https://www.youtube.com/playlist", None),
            // ---- browse ----
            (
                "https://music.youtube.com/browse/MPREb_albumbrowse",
                Some(ParsedLink::Album {
                    browse_id: "MPREb_albumbrowse".into(),
                }),
            ),
            // upstream quirk: a bare /browse navigates to
            // album/browse (lastPathSegment is the literal "browse")
            (
                "https://music.youtube.com/browse",
                Some(ParsedLink::Album {
                    browse_id: "browse".into(),
                }),
            ),
            // ---- channels ----
            (
                "https://music.youtube.com/channel/UCabc_artist",
                Some(ParsedLink::Artist {
                    artist_id: "UCabc_artist".into(),
                }),
            ),
            (
                "https://www.youtube.com/c/artisthandle",
                Some(ParsedLink::Artist {
                    artist_id: "artisthandle".into(),
                }),
            ),
            // ---- search ----
            (
                "https://www.youtube.com/search?q=hello%20world",
                Some(ParsedLink::Search {
                    query: "hello world".into(),
                }),
            ),
            (
                "https://music.youtube.com/search?q=hello+world",
                Some(ParsedLink::Search {
                    query: "hello world".into(),
                }),
            ),
            // search without q: nothing
            ("https://www.youtube.com/search", None),
            // ---- list-only play (video id absent) ----
            (
                "https://www.youtube.com/watch?list=PLabc123",
                Some(ParsedLink::PlayPlaylist {
                    playlist_id: list.clone(),
                }),
            ),
            (
                "https://youtu.be/?list=PLabc123",
                Some(ParsedLink::PlayPlaylist {
                    playlist_id: list.clone(),
                }),
            ),
            // ---- upstream quirks: manifest registers, handler ignores ----
            // /v/ prefix: only the list parameter would act
            ("https://www.youtube.com/v/dQw4w9WgXcQ", None),
            // /embed/ prefix: same
            ("https://www.youtube.com/embed/dQw4w9WgXcQ", None),
            // /user/ prefix: the `when` has "channel", "c" only
            ("https://www.youtube.com/user/artisthandle", None),
            // a bare music.youtube.com landing has no video, no list
            ("https://music.youtube.com/", None),
            // host is irrelevant to dispatch (shared text is any URI)
            (
                "https://example.com/watch?v=dQw4w9WgXcQ",
                Some(ParsedLink::PlayVideo {
                    video_id: video.clone(),
                    playlist_id: None,
                }),
            ),
            // ---- vnd.youtube schemes (port extension; see module docs) ----
            (
                "vnd.youtube:dQw4w9WgXcQ",
                Some(ParsedLink::PlayVideo {
                    video_id: video.clone(),
                    playlist_id: None,
                }),
            ),
            (
                "vnd.youtube:dQw4w9WgXcQ?t=42",
                Some(ParsedLink::PlayVideo {
                    video_id: video.clone(),
                    playlist_id: None,
                }),
            ),
            (
                "vnd.youtube.launch:dQw4w9WgXcQ",
                Some(ParsedLink::PlayVideo {
                    video_id: video.clone(),
                    playlist_id: None,
                }),
            ),
            (
                "vnd.youtube://dQw4w9WgXcQ",
                Some(ParsedLink::PlayVideo {
                    video_id: video.clone(),
                    playlist_id: None,
                }),
            ),
            // ---- shared text (EXTRA_TEXT.toUri()) ----
            (
                "  https://youtu.be/dQw4w9WgXcQ  ",
                Some(ParsedLink::PlayVideo {
                    video_id: video.clone(),
                    playlist_id: None,
                }),
            ),
            // garbage text is not a URI upstream either
            ("check out this song!!!", None),
            ("", None),
        ];

        for (input, expected) in table {
            assert_eq!(&parse(input), expected, "input: {input:?}");
        }
    }

    /// Host comparison is case-insensitive in the port (RFC 3986;
    /// Android keeps `Uri.host` verbatim — the deviation is recorded in
    /// HANDOFF ## Phase 07).
    #[test]
    fn host_comparison_is_case_insensitive() {
        assert_eq!(
            parse("https://YOUTU.BE/dQw4w9WgXcQ"),
            Some(ParsedLink::PlayVideo {
                video_id: "dQw4w9WgXcQ".into(),
                playlist_id: None
            })
        );
    }

    /// Path segments decode like Android's (percent-decoded, empty
    /// segments skipped).
    #[test]
    fn path_segments_decode() {
        assert_eq!(
            parse("https://music.youtube.com/channel/UC%20id"),
            Some(ParsedLink::Artist {
                artist_id: "UC id".into()
            })
        );
        // trailing slash does not create a spurious empty last segment
        assert_eq!(
            parse("https://music.youtube.com/channel/UCid/"),
            Some(ParsedLink::Artist {
                artist_id: "UCid".into()
            })
        );
        // the port normalizes empty segments away, so "/browse/" behaves
        // like the upstream "/browse" quirk: lastPathSegment == "browse"
        // navigates to album/browse (pinned; Android's Uri is ambiguous
        // here, the normalization is the recorded decision)
        assert_eq!(
            parse("https://music.youtube.com/browse/"),
            Some(ParsedLink::Album {
                browse_id: "browse".into()
            })
        );
        assert_eq!(
            parse("https://music.youtube.com/browse"),
            Some(ParsedLink::Album {
                browse_id: "browse".into()
            })
        );
    }

    /// `importSharedAudio` (MainActivity.kt:1565-1595): any non-empty
    /// stream list imports; empty shares fall through to the URI half.
    #[test]
    fn shared_audio_needs_at_least_one_file() {
        assert_eq!(
            parse_shared_files(&[]),
            None,
            "empty share must not consume the link handling"
        );
        assert_eq!(
            parse_shared_files(&["/home/z/song.flac".into(), "/tmp/b side.mp3".into()]),
            Some(ParsedLink::SharedAudio {
                paths: vec!["/home/z/song.flac".into(), "/tmp/b side.mp3".into()],
            })
        );
        // every file is attempted, whatever the extension (upstream
        // defers filtering to the local importer)
        assert_eq!(
            parse_shared_files(&["/tmp/unknown.bin".into()]),
            Some(ParsedLink::SharedAudio {
                paths: vec!["/tmp/unknown.bin".into()]
            })
        );
    }

    /// The phone-link action opens the devices sheet before any URI
    /// parsing (MainActivity.kt:1599-1603) — that behavior belongs to
    /// the Phase 23 link protocol and is intentionally absent here; the
    /// test pins the parse of a link:// style URI as inert so the seam
    /// stays observable.
    #[test]
    fn phone_link_actions_are_not_this_parsers_business() {
        // a link-protocol URL is not a YouTube link; parse() reports
        // nothing rather than inventing navigation
        assert_eq!(parse("https://link.mochy.local/tv"), None);
    }
}
