//! The ranged request cache — the player cache, ported.
//!
//! Absorbs the Android `SimpleCache` configuration for the player's stream
//! cache (`di/AppModule.kt:86-104`): a least-recently-used byte cache of
//! **1024 MB by default**, `-1` meaning unlimited (upstream's
//! `LeastRecentlyUsedCacheEvictor` vs `NoOpCacheEvictor`), keyed per song
//! (`MaxSongCacheSizeKey`, default 1024).
//!
//! The stale-splice eviction rule is the one Phase 06 pinned
//! (`mochy-innertube/src/stream/validate.rs`, upstream
//! "if itag or content-length changed, evict cached spans"): splicing new
//! bytes into old ones produces a corrupt file, so when the stream
//! identity changes, **every cached span of the old stream is evicted
//! before any new span is stored**.
//!
//! Subranges are **512 KB aligned** — the chunk length the player reads in
//! (`MusicService.kt:5608`, `CHUNK_LENGTH = 512 * 1024L`): a span is
//! stored and fetched on 512 KB boundaries so partial-range misses still
//! land on the same alignment the network fetcher uses.

use std::collections::HashMap;
use std::sync::Mutex;

/// The subrange/chunk alignment: 512 KB (`MusicService.kt:5608`).
pub const SUBRANGE_BYTES: u64 = 512 * 1024;
/// The default byte budget: 1024 MB (`AppModule.kt:88`, data store
/// `MaxSongCacheSizeKey` default).
pub const DEFAULT_BUDGET_BYTES: Option<u64> = Some(1024 * 1024 * 1024);

/// A stream's identity for the stale-splice rule — the itag and
/// content-length of the resolved format (Phase 06's `stale_splice`
/// inputs; see `validate.rs:104-112`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StreamIdentity {
    /// The resolved format's itag.
    pub itag: u32,
    /// The resolved format's content length, when known.
    pub content_length: Option<u64>,
}

impl StreamIdentity {
    /// Whether a fresh identity invalidates a cached one
    /// (`stale_splice`: itag or content-length change → evict).
    pub fn stale_against(&self, fresh: &StreamIdentity) -> bool {
        self.itag != fresh.itag || self.content_length != fresh.content_length
    }
}

#[derive(Debug)]
struct Span {
    data: Vec<u8>,
    start: u64,
    last_use: u64,
}

#[derive(Debug, Default)]
struct Inner {
    // key → identity → spans. The identity layer IS the stale-splice rule:
    // a changed identity orphans the old span set wholesale.
    streams: HashMap<String, (StreamIdentity, Vec<Span>)>,
    use_counter: u64,
    bytes: u64,
}

/// The LRU byte cache. `budget: None` = unlimited (the upstream `-1`).
///
/// Interior locking is fine: this lives on the decode/IO side, never in
/// the audio callback.
#[derive(Debug)]
pub struct ByteSpanCache {
    budget: Option<u64>,
    inner: Mutex<Inner>,
}

impl Default for ByteSpanCache {
    fn default() -> Self {
        ByteSpanCache::new()
    }
}

impl ByteSpanCache {
    /// A cache with the default 1024 MB budget.
    pub fn new() -> ByteSpanCache {
        ByteSpanCache::with_budget(DEFAULT_BUDGET_BYTES)
    }

    /// A cache with an explicit byte budget; `None` = unlimited
    /// (`MaxSongCacheSizeKey == -1` → `NoOpCacheEvictor`).
    pub fn with_budget(budget: Option<u64>) -> ByteSpanCache {
        ByteSpanCache {
            budget,
            inner: Mutex::new(Inner::default()),
        }
    }

    /// Announces the stream a key belongs to. When the identity is stale
    /// against the last announcement, every cached span of the key is
    /// evicted first — the stale-splice rule.
    pub fn begin_stream(&self, key: &str, identity: StreamIdentity) {
        let mut inner = self.lock();
        let stale = inner
            .streams
            .get(key)
            .is_some_and(|(old, _)| old.stale_against(&identity));
        if stale {
            let evicted: u64 = inner.streams[key]
                .1
                .iter()
                .map(|s| s.data.len() as u64)
                .sum();
            let (old, spans) = inner.streams.get_mut(key).expect("just checked");
            spans.clear();
            *old = identity;
            inner.bytes = inner.bytes.saturating_sub(evicted);
        } else if !inner.streams.contains_key(key) {
            inner.streams.insert(key.to_owned(), (identity, Vec::new()));
        }
    }

    /// Stores a span (already aligned by the fetcher; unaligned starts are
    /// accepted and stored as-is — the alignment contract is the caller's).
    pub fn put(&self, key: &str, start: u64, data: Vec<u8>) {
        let budget = match self.budget {
            Some(budget) => budget,
            None => {
                // Unlimited: store and go (NoOpCacheEvictor).
                let mut inner = self.lock();
                store_span(&mut inner, key, start, data, 0);
                return;
            }
        };
        let mut inner = self.lock();
        store_span(&mut inner, key, start, data, 0);
        // LRU eviction to fit the budget (LeastRecentlyUsedCacheEvictor:
        // least recently used spans go first, the just-stored one last).
        while inner.bytes > budget {
            let evicted = evict_least_recently_used(&mut inner, key);
            if !evicted {
                break;
            }
        }
    }

    /// Serves the longest contiguous run of cached bytes starting at
    /// `start`, up to `max_len`: `None` when `start` itself isn't covered.
    ///
    /// This is the upstream reader's shape (`Media3MediaDataSource`
    /// served partial reads — `return if (total == 0) -1 else total`):
    /// a straddling read gets what the cache holds now and comes back
    /// for the rest after the next chunk fill.
    pub fn get(&self, key: &str, start: u64, max_len: usize) -> Option<Vec<u8>> {
        let mut inner = self.lock();
        let use_counter = inner.use_counter + 1;
        inner.use_counter = use_counter;
        let (_, spans) = inner.streams.get_mut(key)?;
        let end = start + max_len as u64;
        let mut whole: Vec<u8> = Vec::new();
        let mut cursor = start;
        while cursor < end {
            let mut found = false;
            for span in spans.iter_mut() {
                if span.start <= cursor && cursor < span.start + span.data.len() as u64 {
                    span.last_use = use_counter;
                    let offset = (cursor - span.start) as usize;
                    let take = ((span.data.len() as u64 - (cursor - span.start)).min(end - cursor))
                        as usize;
                    whole.extend_from_slice(&span.data[offset..offset + take]);
                    cursor += take as u64;
                    found = true;
                    break;
                }
            }
            if !found {
                break;
            }
        }
        if whole.is_empty() { None } else { Some(whole) }
    }

    /// Whether the key holds a full-span cover of `[start, start+len)`.
    pub fn is_cached(&self, key: &str, start: u64, len: usize) -> bool {
        let mut inner = self.lock();
        let (_, spans) = match inner.streams.get_mut(key) {
            Some(stream) => stream,
            None => return false,
        };
        let end = start + len as u64;
        let mut cursor = start;
        'outer: while cursor < end {
            for span in spans.iter() {
                if span.start <= cursor && cursor < span.start + span.data.len() as u64 {
                    cursor = (cursor + (span.data.len() as u64 - (cursor - span.start))).min(end);
                    continue 'outer;
                }
            }
            return false;
        }
        true
    }

    /// Total bytes held.
    pub fn bytes_used(&self) -> u64 {
        self.lock().bytes
    }

    /// Drops every span of a key (`playerCache.removeResource` — the
    /// offline-cache wipe path, MusicService.kt:3531).
    pub fn remove(&self, key: &str) {
        let mut inner = self.lock();
        let evicted: u64 = inner
            .streams
            .get(key)
            .map(|(_, spans)| spans.iter().map(|s| s.data.len() as u64).sum())
            .unwrap_or(0);
        if let Some((_, spans)) = inner.streams.get_mut(key) {
            spans.clear();
        }
        inner.bytes = inner.bytes.saturating_sub(evicted);
    }

    /// Drops everything (`clearCache`'s player-cache half,
    /// MusicService.kt:3553-3555).
    pub fn clear(&self) {
        let mut inner = self.lock();
        inner.streams.clear();
        inner.bytes = 0;
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        // A poisoned cache lock means a panic mid-write; treat the cache as
        // recoverable (stale data at worst — it is a cache).
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

fn store_span(inner: &mut Inner, key: &str, start: u64, data: Vec<u8>, _reserved: u64) {
    let use_counter = inner.use_counter + 1;
    inner.use_counter = use_counter;
    let bytes = data.len() as u64;
    if let Some((_, spans)) = inner.streams.get_mut(key) {
        // Replace a same-start span (a refetch of the same chunk).
        if let Some(existing) = spans.iter_mut().find(|s| s.start == start) {
            inner.bytes = inner.bytes.saturating_sub(existing.data.len() as u64);
            existing.data = data;
            existing.last_use = use_counter;
            inner.bytes += bytes;
            return;
        }
        spans.push(Span {
            data,
            start,
            last_use: use_counter,
        });
        inner.bytes += bytes;
    }
}

fn evict_least_recently_used(inner: &mut Inner, keep_key: &str) -> bool {
    // Find the least recently used span across all keys; never evict the
    // key that just stored (upstream evicts on insert, the new span
    // survives).
    let mut victim_key: Option<String> = None;
    let mut victim_use = u64::MAX;
    for key in inner.streams.keys() {
        if key == keep_key {
            continue;
        }
        if let Some(oldest) = inner.streams[key].1.iter().map(|s| s.last_use).min()
            && oldest < victim_use
        {
            victim_use = oldest;
            victim_key = Some(key.clone());
        }
    }
    let Some(key) = victim_key else {
        return false;
    };
    let Some((_, spans)) = inner.streams.get_mut(&key) else {
        return false;
    };
    if spans.is_empty() {
        return false;
    }
    let oldest_idx = spans
        .iter()
        .enumerate()
        .min_by_key(|(_, s)| s.last_use)
        .map(|(i, _)| i)
        .unwrap_or(0);
    let span = spans.swap_remove(oldest_idx);
    inner.bytes = inner.bytes.saturating_sub(span.data.len() as u64);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(itag: u32, len: u64) -> StreamIdentity {
        StreamIdentity {
            itag,
            content_length: Some(len),
        }
    }

    #[test]
    fn stores_and_serves_aligned_spans() {
        let cache = ByteSpanCache::with_budget(None);
        cache.begin_stream("song", identity(140, 1024 * 1024));
        cache.put("song", 0, vec![1u8; SUBRANGE_BYTES as usize]);
        cache.put("song", SUBRANGE_BYTES, vec![2u8; 4096]);
        // Full first chunk.
        let got = cache.get("song", 0, SUBRANGE_BYTES as usize).unwrap();
        assert_eq!(got.len(), SUBRANGE_BYTES as usize);
        assert!(got.iter().all(|b| *b == 1));
        // Straddling read across both spans.
        let straddle = cache.get("song", SUBRANGE_BYTES - 4, 8).unwrap();
        assert_eq!(&straddle[..4], &[1, 1, 1, 1]);
        assert_eq!(&straddle[4..], &[2, 2, 2, 2]);
        // Partial miss: past the stored bytes.
        assert!(cache.get("song", SUBRANGE_BYTES * 2, 16).is_none());
        assert!(cache.is_cached("song", 0, SUBRANGE_BYTES as usize));
        // Fully covered through both spans (span1 runs to +4096)…
        assert!(cache.is_cached("song", 0, SUBRANGE_BYTES as usize + 16));
        // …but past the stored tail of the second span: not covered.
        assert!(!cache.is_cached("song", SUBRANGE_BYTES + 4096, 16));
    }

    #[test]
    fn stale_splice_evicts_the_old_stream() {
        // The Phase 06 rule: itag or content-length change → evict.
        let cache = ByteSpanCache::with_budget(None);
        cache.begin_stream("song", identity(140, 1024));
        cache.put("song", 0, vec![9u8; 1024]);
        assert!(cache.get("song", 0, 1024).is_some());

        // Same itag, same length: a url refresh keeps every cached byte.
        cache.begin_stream("song", identity(140, 1024));
        assert!(cache.get("song", 0, 1024).is_some());

        // Different itag: everything goes.
        cache.begin_stream("song", identity(251, 1024));
        assert!(cache.get("song", 0, 1024).is_none());
        assert_eq!(cache.bytes_used(), 0);

        // Same itag, different length: everything goes too.
        cache.begin_stream("song", identity(251, 2048));
        cache.put("song", 0, vec![7u8; 512]);
        cache.begin_stream("song", identity(251, 4096));
        assert!(cache.get("song", 0, 512).is_none());
    }

    #[test]
    fn lru_eviction_respects_the_budget() {
        // 4 KB budget, 2 KB spans: the third store evicts the least
        // recently used.
        let cache = ByteSpanCache::with_budget(Some(4 * 1024));
        cache.begin_stream("a", identity(1, 1));
        cache.begin_stream("b", identity(1, 1));
        cache.begin_stream("c", identity(1, 1));
        cache.put("a", 0, vec![1u8; 2048]);
        cache.put("b", 0, vec![2u8; 2048]);
        assert_eq!(cache.bytes_used(), 4096);
        // Touch "a" so "b" is the least recently used.
        let _ = cache.get("a", 0, 16);
        cache.put("c", 0, vec![3u8; 2048]);
        assert_eq!(cache.bytes_used(), 4096);
        assert!(cache.get("a", 0, 2048).is_some(), "recently used survives");
        assert!(
            cache.get("b", 0, 2048).is_none(),
            "least recently used evicted"
        );
        assert!(cache.get("c", 0, 2048).is_some());
    }

    #[test]
    fn unlimited_budget_never_evicts() {
        let cache = ByteSpanCache::with_budget(None);
        for i in 0..64 {
            let key = format!("s{i}");
            cache.begin_stream(&key, identity(1, 1));
            cache.put(&key, 0, vec![i as u8; 1024]);
        }
        assert_eq!(cache.bytes_used(), 64 * 1024);
        assert!(cache.get("s0", 0, 1024).is_some());
    }

    #[test]
    fn remove_and_clear() {
        let cache = ByteSpanCache::new();
        cache.begin_stream("a", identity(1, 1));
        cache.begin_stream("b", identity(1, 1));
        cache.put("a", 0, vec![1u8; 100]);
        cache.put("b", 0, vec![1u8; 100]);
        cache.remove("a");
        assert!(cache.get("a", 0, 100).is_none());
        assert!(cache.get("b", 0, 100).is_some());
        cache.clear();
        assert!(cache.get("b", 0, 100).is_none());
        assert_eq!(cache.bytes_used(), 0);
    }

    #[test]
    fn refetch_replaces_same_chunk() {
        let cache = ByteSpanCache::with_budget(None);
        cache.begin_stream("a", identity(1, 1));
        cache.put("a", 0, vec![1u8; 100]);
        cache.put("a", 0, vec![2u8; 100]);
        assert_eq!(cache.bytes_used(), 100);
        assert_eq!(cache.get("a", 0, 100).unwrap(), vec![2u8; 100]);
    }
}
