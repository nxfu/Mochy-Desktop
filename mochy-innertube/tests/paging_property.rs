//! Paging-helper property tests — the Phase 04 acceptance criterion:
//! "paging helper property test: dedup + 50-ceiling behavior" over
//! generated token streams (blueprint ch. 15: queue/paging state-machine
//! property tests; proptest was pinned for exactly this).

use mochy_innertube::paging::{
    Admit, ContinuationPager, EmptyPagePolicy, MAX_REQUESTS, StopReason,
};
use proptest::prelude::*;

/// Drive a pager over a token stream with per-page empty flags; returns
/// (fetched tokens, stop reason). The caller-side loop shape from
/// `ContinuationPager`'s docs.
fn drive(
    policy: EmptyPagePolicy,
    tokens: Vec<String>,
    empties: Vec<bool>,
) -> (Vec<String>, Option<StopReason>) {
    let mut pager = ContinuationPager::new(policy);
    let mut fetched = Vec::new();
    let mut stop = None;
    for (index, token) in tokens.into_iter().enumerate() {
        match pager.admit(&token) {
            Admit::Fetch => fetched.push(token),
            Admit::Stop(reason) => {
                stop = Some(reason);
                break;
            }
        }
        if !pager.record_page(empties[index % empties.len().max(1)]) {
            stop = Some(StopReason::EmptyPage);
            break;
        }
    }
    (fetched, stop)
}

proptest! {
    /// Dedup: no token is ever fetched twice, whatever the stream. When a
    /// repeat is reached it stops with DuplicateToken (Utils.kt:19-21).
    #[test]
    fn no_token_is_fetched_twice(
        tokens in prop::collection::vec("[a-z]{1,4}", 0..40),
        empties in prop::collection::vec(any::<bool>(), 0..40),
    ) {
        let empties = if empties.is_empty() { vec![false] } else { empties };
        // playlist policy with all-non-empty pages isolates the dedup
        let (fetched, stop) = drive(
            EmptyPagePolicy::BreakOnFirstEmpty,
            tokens.clone(),
            vec![false],
        );
        let mut unique = fetched.clone();
        unique.sort();
        unique.dedup();
        prop_assert_eq!(unique.len(), fetched.len(), "a token was fetched twice");
        // any token in the input that appears twice and is reached -> stop
        if let Some(reason) = stop {
            prop_assert_eq!(reason, StopReason::DuplicateToken);
        }
        // every fetched token was in the input
        for token in &fetched {
            prop_assert!(tokens.contains(token));
        }
        // the empty flags were never consulted for stopping (all non-empty)
        let _ = empties;
    }

    /// Ceiling: over any stream of DISTINCT tokens with non-empty pages,
    /// at most 50 requests are admitted; the 51st admit stops with
    /// CeilingReached (Utils.kt:16/18).
    #[test]
    fn ceiling_caps_at_fifty_requests(
        count in 0usize..120,
    ) {
        let tokens: Vec<String> = (0..count).map(|i| format!("t{i}")).collect();
        let mut pager = ContinuationPager::playlist_songs();
        let mut fetched = 0;
        for token in &tokens {
            match pager.admit(token) {
                Admit::Fetch => {
                    fetched += 1;
                    assert!(pager.record_page(false));
                }
                Admit::Stop(reason) => {
                    prop_assert_eq!(reason, StopReason::CeilingReached);
                    prop_assert_eq!(fetched, MAX_REQUESTS);
                    return Ok(());
                }
            }
        }
        // the stream ran out first: at most the ceiling was admitted
        prop_assert!(fetched <= MAX_REQUESTS);
        prop_assert_eq!(fetched as usize, count.min(MAX_REQUESTS as usize));
    }

    /// The library policy survives exactly one consecutive empty page and
    /// stops on the second (Utils.kt:60-66), for any interleaving.
    #[test]
    fn library_policy_needs_two_consecutive_empties(
        empties in prop::collection::vec(any::<bool>(), 1..24),
    ) {
        let mut pager = ContinuationPager::library_items();
        let mut consecutive = 0;
        for (index, empty) in empties.iter().enumerate() {
            let token = format!("t{index}");
            prop_assert_eq!(pager.admit(&token), Admit::Fetch);
            let may_continue = pager.record_page(*empty);
            if *empty {
                consecutive += 1;
                prop_assert_eq!(
                    may_continue,
                    consecutive < 2,
                    "continue after 1 consecutive empty, stop at 2"
                );
            } else {
                consecutive = 0;
                prop_assert!(may_continue, "a non-empty page always continues");
            }
        }
    }

    /// The playlist policy stops at the FIRST empty page (Utils.kt:27),
    /// wherever it falls.
    #[test]
    fn playlist_policy_stops_at_first_empty(
        position in 0usize..24,
    ) {
        let mut pager = ContinuationPager::playlist_songs();
        for index in 0..=position {
            let token = format!("t{index}");
            prop_assert_eq!(pager.admit(&token), Admit::Fetch);
            let last = index == position;
            prop_assert_eq!(pager.record_page(last), !last);
        }
    }
}

/// The scripted transcription, kept alongside the properties as the
/// readable spec (mirrors Utils.kt:9-38 exactly).
#[test]
fn direct_transcription_of_the_playlist_loop() {
    let mut pager = ContinuationPager::playlist_songs();
    let mut songs: Vec<&str> = vec!["page0"];
    let mut continuation = Some("tok1");
    // tok1 -> 2 songs + tok2; tok2 -> 0 songs (empty: stop)
    let pages = [("tok1", 2usize, Some("tok2")), ("tok2", 0, None)];
    while let Some(token) = continuation {
        if matches!(pager.admit(token), Admit::Stop(_)) {
            break;
        }
        let (_, count, next) = pages.iter().find(|(t, _, _)| *t == token).copied().unwrap();
        continuation = next;
        if !pager.record_page(count == 0) {
            break;
        }
        songs.extend((0..count).map(|_| "song"));
    }
    assert_eq!(songs, vec!["page0", "song", "song"]);
    assert_eq!(pager.request_count(), 2);
}
