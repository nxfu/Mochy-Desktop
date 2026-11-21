//! Continuation paging — the caller-side helper.
//!
//! Upstream paging lives in two shapes:
//! - `innertube/utils/Utils.kt:9-38` (`completedLibrary`): the playlist
//!   songs loop — break on a duplicate token, on a failed page, on an
//!   empty page, or after 50 requests;
//! - `Utils.kt:41-74` (`completedPlaylist`): the library items loop —
//!   same dedup/ceiling, but tolerates ONE empty page and breaks on the
//!   second consecutive empty response;
//! - `YouTube.kt`'s own loops (`library`, `playlist`, `search`, …) drive
//!   continuations through the facade with the same 50-request ceiling.
//!
//! This module extracts the loop STATE MACHINE so callers (Phase 05's
//! facade, the YT sync engine) drive it: the helper decides whether a
//! token may be fetched, tracks the ceiling and the empty-page policy,
//! and never performs IO itself — the ceiling is enforced caller-side by
//! construction.

use std::collections::HashSet;

/// The upstream request ceiling (`Utils.kt:16` / `Utils.kt:48`:
/// `val maxRequests = 50`).
pub const MAX_REQUESTS: u32 = 50;

/// How an empty continuation page is treated
/// (`Utils.kt:27` vs `Utils.kt:60-66`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmptyPagePolicy {
    /// Playlist songs: "an empty page never carries a next token, and
    /// youtube only hands out a token when more songs follow" — break at
    /// the first empty page.
    BreakOnFirstEmpty,
    /// Library items: tolerate one empty page, break on the second
    /// consecutive empty response.
    BreakOnSecondConsecutiveEmpty,
}

/// Why the pager told the caller to stop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopReason {
    /// The token was already fetched — paging is looping.
    DuplicateToken,
    /// The 50-request ceiling was reached.
    CeilingReached,
    /// The empty-page policy fired.
    EmptyPage,
}

/// The paging state machine. Create per paging run; the caller's loop is:
///
/// ```ignore
/// let mut pager = ContinuationPager::playlist_songs();
/// while let Some(token) = continuation {
///     match pager.admit(&token) {
///         Admit::Fetch => {}
///         Admit::Stop(_) => break,
///     }
///     let page = fetch(token).await?;          // caller IO
///     let empty = page.items.is_empty();
///     continuation = page.continuation;
///     if !pager.record_page(empty) { break; }  // EmptyPage policy
///     items.extend(page.items);
/// }
/// ```
///
/// Upstream keeps the failing/looping token in the page's continuation
/// slot so "a sync knows the list is short" (Utils.kt:8) — the caller
/// owns that token; the pager only reports the stop reason.
#[derive(Debug)]
pub struct ContinuationPager {
    seen: HashSet<String>,
    request_count: u32,
    consecutive_empty_responses: u32,
    policy: EmptyPagePolicy,
}

/// The admit decision for one continuation token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Admit {
    /// Fetch this token (it has been recorded).
    Fetch,
    /// Stop paging — with the reason.
    Stop(StopReason),
}

impl ContinuationPager {
    /// The playlist-songs policy (`Utils.kt:9-38`).
    pub fn playlist_songs() -> ContinuationPager {
        ContinuationPager::new(EmptyPagePolicy::BreakOnFirstEmpty)
    }

    /// The library-items policy (`Utils.kt:41-74`).
    pub fn library_items() -> ContinuationPager {
        ContinuationPager::new(EmptyPagePolicy::BreakOnSecondConsecutiveEmpty)
    }

    /// A pager with an explicit policy.
    pub fn new(policy: EmptyPagePolicy) -> ContinuationPager {
        ContinuationPager {
            seen: HashSet::new(),
            request_count: 0,
            consecutive_empty_responses: 0,
            policy,
        }
    }

    /// Entry check for one continuation token (`Utils.kt:18-23` /
    /// `Utils.kt:51-56`): the ceiling first, then the seen-set — the same
    /// order upstream evaluates them.
    pub fn admit(&mut self, token: &str) -> Admit {
        if self.request_count >= MAX_REQUESTS {
            return Admit::Stop(StopReason::CeilingReached);
        }
        if self.seen.contains(token) {
            return Admit::Stop(StopReason::DuplicateToken);
        }
        self.seen.insert(token.to_owned());
        self.request_count += 1;
        Admit::Fetch
    }

    /// Post-fetch accounting (`Utils.kt:27` / `Utils.kt:60-66`):
    /// reports whether paging may CONTINUE after this page. A non-empty
    /// page resets the consecutive-empty streak; the library policy
    /// tolerates one empty page and stops on the second consecutive
    /// (`consecutiveEmptyResponses >= 2` upstream breaks, so count < 2
    /// continues).
    pub fn record_page(&mut self, was_empty: bool) -> bool {
        if was_empty {
            self.consecutive_empty_responses += 1;
            match self.policy {
                EmptyPagePolicy::BreakOnFirstEmpty => false,
                EmptyPagePolicy::BreakOnSecondConsecutiveEmpty => {
                    self.consecutive_empty_responses < 2
                }
            }
        } else {
            self.consecutive_empty_responses = 0;
            true
        }
    }

    /// Requests admitted so far (the ceiling counter).
    pub fn request_count(&self) -> u32 {
        self.request_count
    }

    /// Tokens already fetched.
    pub fn seen_tokens(&self) -> usize {
        self.seen.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A direct transcription of `Utils.kt:9-38` (playlist songs) against
    /// a scripted page source — the behavioral acceptance for the dedup +
    /// ceiling semantics.
    #[test]
    fn playlist_pager_transcribes_upstream() {
        let mut pager = ContinuationPager::playlist_songs();
        let mut songs: Vec<String> = vec!["page0".to_owned()];
        let mut continuation = Some("tok1".to_owned());
        let mut fetched: Vec<String> = Vec::new();

        // pages: tok1 -> 2 songs + tok2; tok2 -> EMPTY (stop)
        let pages: &[(&str, usize, Option<&str>)] = &[("tok1", 2, Some("tok2")), ("tok2", 0, None)];
        while let Some(token) = continuation {
            match pager.admit(&token) {
                Admit::Fetch => fetched.push(token.clone()),
                Admit::Stop(_) => break,
            }
            let (_, count, next) = pages.iter().find(|(t, _, _)| *t == token).copied().unwrap();
            let empty = count == 0;
            continuation = next.map(str::to_owned);
            if !pager.record_page(empty) {
                break;
            }
            songs.extend((0..count).map(|i| format!("song{i}")));
        }
        assert_eq!(fetched, vec!["tok1", "tok2"]);
        // page0 + 2 from tok1; tok2's empty page contributes nothing
        assert_eq!(songs.len(), 3);
        // both tokens were admitted and fetched (the empty page IS
        // fetched upstream — Utils.kt:25-27 — before the break)
        assert_eq!(pager.request_count(), 2);
    }

    #[test]
    fn library_pager_tolerates_one_empty() {
        // Utils.kt:41-74 — two consecutive empties stop, one is survived
        let mut pager = ContinuationPager::library_items();
        assert!(pager.record_page(false));
        assert!(pager.record_page(true)); // first empty: keep going
        assert!(pager.record_page(false)); // reset streak
        assert!(pager.record_page(true));
        assert!(!pager.record_page(true)); // second consecutive: stop
    }

    #[test]
    fn duplicate_token_stops_paging() {
        // Utils.kt:19-21
        let mut pager = ContinuationPager::playlist_songs();
        assert_eq!(pager.admit("tokA"), Admit::Fetch);
        assert_eq!(pager.admit("tokA"), Admit::Stop(StopReason::DuplicateToken));
        // a different token still fetches
        assert_eq!(pager.admit("tokB"), Admit::Fetch);
        assert_eq!(pager.seen_tokens(), 2);
    }

    #[test]
    fn ceiling_is_fifty_requests() {
        // Utils.kt:16
        let mut pager = ContinuationPager::playlist_songs();
        for i in 0..MAX_REQUESTS {
            assert_eq!(
                pager.admit(&format!("tok{i}")),
                Admit::Fetch,
                "request {i} must be admitted"
            );
            assert!(pager.record_page(false)); // non-empty pages
        }
        assert_eq!(
            pager.admit("tok-overflow"),
            Admit::Stop(StopReason::CeilingReached)
        );
        assert_eq!(pager.request_count(), MAX_REQUESTS);
    }
}
