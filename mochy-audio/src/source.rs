//! The ranged reader over the byte cache — how the decoder sees a song.
//!
//! Port of `StreamPcmDecoder.kt`'s `Media3MediaDataSource` (upstream pin
//! `bde01cb4`, lines 119-206): reading carries on from where it stopped
//! across chunk boundaries, the true length comes from the response's
//! Content-Range (or from the cache when it served the whole remainder),
//! and a 416 past a known position means the end of the file, not an
//! error.
//!
//! The desktop side of the fetch is abstracted behind [`RangeFetcher`] —
//! the network leg is the app's to provide (the mochy crate bridges
//! mochy-net's async client onto this sync trait on the decode thread);
//! tests provide fixtures. The fetcher answers ranges the way the
//! upstream `DataSource` did: one open per chunk, the size from
//! Content-Range.

use std::io;
use std::sync::Arc;

use crate::bytecache::{ByteSpanCache, StreamIdentity, SUBRANGE_BYTES};

/// What one ranged fetch learned — the `DataSource.open` +
/// Content-Range pair the upstream reader consumed.
#[derive(Debug, Clone)]
pub struct FetchedRange {
    /// The bytes for `[start, start+len)`.
    pub data: Vec<u8>,
    /// The total length of the stream when the response said so
    /// (`totalFromContentRange`), else `None`.
    pub total_len: Option<u64>,
}

/// The sync range-fetch seam the decode thread drives.
///
/// `fetch(url, headers, start, end_inclusive)` returns the bytes for the
/// inclusive range plus any total-length knowledge. A 416 at a positive
/// offset should surface as [`RangeFetchError::EndPastPosition`] — the
/// upstream reader turned exactly that into "the file ends here"
/// (`Media3MediaDataSource.reopen`, 416 handling at lines 181-189).
pub trait RangeFetcher: Send + Sync {
    fn fetch(
        &self,
        url: &str,
        headers: &[(String, String)],
        start: u64,
        end_inclusive: u64,
    ) -> Result<FetchedRange, RangeFetchError>;
}

#[derive(thiserror::Error, Debug, Clone)]
pub enum RangeFetchError {
    /// HTTP 416 with `offset > 0`: the stream ends at `at` — upstream
    /// records `length = offset` and stops reopening
    /// (`StreamPcmDecoder.kt:182-188`).
    #[error("the stream ends at byte {at}")]
    EndPastPosition { at: u64 },
    /// Any transport failure.
    #[error("fetch failed: {0}")]
    Io(String),
}

/// A seekable byte reader over one resolved stream, backed by the byte
/// cache and a [`RangeFetcher`] — symphonia's `MediaSource`.
///
/// Reads are served from cached 512 KB subranges; misses fetch the
/// covering chunk, store it, and serve. Reads that straddle a chunk
/// boundary fetch both — the upstream player read in whole chunks
/// (`CHUNK_LENGTH`), the demuxer reads in small probes.
pub struct RangedMediaSource {
    fetcher: Arc<dyn RangeFetcher>,
    cache: Arc<ByteSpanCache>,
    url: String,
    headers: Vec<(String, String)>,
    key: String,
    position: u64,
    length: Option<u64>,
    ended: bool,
}

impl RangedMediaSource {
    /// Opens a stream for reading. `identity` seeds the cache's
    /// stale-splice bookkeeping (Phase 06's rule: itag or content-length
    /// change → evict).
    pub fn new(
        fetcher: Arc<dyn RangeFetcher>,
        cache: Arc<ByteSpanCache>,
        url: impl Into<String>,
        headers: Vec<(String, String)>,
        key: impl Into<String>,
        identity: StreamIdentity,
    ) -> RangedMediaSource {
        let key = key.into();
        let length = identity.content_length;
        cache.begin_stream(&key, identity);
        RangedMediaSource {
            fetcher,
            cache,
            url: url.into(),
            headers,
            key: key.into(),
            position: 0,
            length,
            ended: false,
        }
    }

    /// The stream's total length once known (Content-Range or the
    /// identity); the demuxer asks through `byte_len`.
    fn ensure_length(&mut self) -> io::Result<Option<u64>> {
        if self.length.is_none() && !self.ended {
            // Probing byte 0 discovers the length the way `getSize` did
            // (`reopen(0L)` in `getSize`, StreamPcmDecoder.kt:165-167).
            self.fill_chunk(0)?;
        }
        Ok(self.length)
    }

    /// Fetches the 512 KB chunk containing `offset`, stores it, learns the
    /// length.
    fn fill_chunk(&mut self, offset: u64) -> io::Result<()> {
        if let Some(len) = self.length {
            if offset >= len {
                return Ok(());
            }
        }
        let chunk_start = (offset / SUBRANGE_BYTES) * SUBRANGE_BYTES;
        let chunk_end_inclusive = chunk_start + SUBRANGE_BYTES - 1;
        let capped_end = match self.length {
            Some(len) => chunk_end_inclusive.min(len.saturating_sub(1)),
            None => chunk_end_inclusive,
        };
        match self
            .fetcher
            .fetch(&self.url, &self.headers, chunk_start, capped_end)
        {
            Ok(fetched) => {
                if let Some(total) = fetched.total_len {
                    self.length = Some(total);
                }
                self.cache
                    .put(&self.key, chunk_start, fetched.data);
                Ok(())
            }
            Err(RangeFetchError::EndPastPosition { at }) => {
                // "asked past the end of the stream" — the length is where
                // it refused (upstream: `length = offset`, `ended = true`).
                self.ended = true;
                self.length = Some(at);
                Ok(())
            }
            Err(RangeFetchError::Io(why)) => Err(io::Error::new(io::ErrorKind::Other, why)),
        }
    }
}

impl io::Read for RangedMediaSource {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        // Past the end: EOF (StreamPcmDecoder.kt:141-142).
        if let Some(len) = self.length {
            if self.position >= len {
                return Ok(0);
            }
        }
        // Serve what the cache covers run by run — the upstream loop's
        // "reading carries on from where it stopped until the request is
        // satisfied", re-checking the learned length each pass
        // (StreamPcmDecoder.kt:143-161).
        let mut filled = 0usize;
        while filled < buf.len() {
            let cursor = self.position + filled as u64;
            if let Some(len) = self.length {
                if cursor >= len {
                    break;
                }
            }
            let want = buf.len() - filled;
            if let Some(run) = self.cache.get(&self.key, cursor, want) {
                buf[filled..filled + run.len()].copy_from_slice(&run);
                filled += run.len();
                continue;
            }
            // Miss: fetch the covering 512 KB chunk and retry.
            self.fill_chunk(cursor)?;
            if let Some(run) = self.cache.get(&self.key, cursor, want) {
                buf[filled..filled + run.len()].copy_from_slice(&run);
                filled += run.len();
                continue;
            }
            // Still nothing: either the fill learned the stream ends here,
            // or the fetcher went quiet at the tail — serve what we have.
            if self.length.is_some_and(|len| cursor >= len) {
                break;
            }
            if filled > 0 {
                break;
            }
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "range fetch returned nothing",
            ));
        }
        self.position += filled as u64;
        Ok(filled)
    }
}

impl io::Seek for RangedMediaSource {
    fn seek(&mut self, pos: io::SeekFrom) -> io::Result<u64> {
        let (base, offset) = match pos {
            io::SeekFrom::Start(at) => {
                self.position = at;
                return Ok(at);
            }
            io::SeekFrom::Current(delta) => (self.position as i64, delta),
            io::SeekFrom::End(delta) => {
                let len = self
                    .ensure_length()?
                    .ok_or_else(|| io::Error::new(io::ErrorKind::Unsupported, "unknown length"))?;
                (len as i64, delta)
            }
        };
        let target = base + offset;
        if target < 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "seek before start",
            ));
        }
        self.position = target as u64;
        Ok(self.position)
    }
}

impl symphonia::core::io::MediaSource for RangedMediaSource {
    fn is_seekable(&self) -> bool {
        true
    }

    fn byte_len(&self) -> Option<u64> {
        self.length
    }
}

/// A [`RangeFetcher`] over an in-memory buffer — the decode tests' (and
/// the offline harness's) stand-in for the network.
#[derive(Debug, Clone)]
pub struct MemoryFetcher {
    pub bytes: Arc<Vec<u8>>,
    /// Failures the fixture wants to inject, by chunk start.
    pub fail_at: Vec<(u64, RangeFetchError)>,
    /// When set, fetches omit the total length — fixtures exercising the
    /// learn-the-length paths.
    pub hide_total_len: bool,
}

impl RangeFetcher for MemoryFetcher {
    fn fetch(
        &self,
        _url: &str,
        _headers: &[(String, String)],
        start: u64,
        end_inclusive: u64,
    ) -> Result<FetchedRange, RangeFetchError> {
        if let Some((_, failure)) = self.fail_at.iter().find(|(at, _)| *at == start) {
            return Err(failure.clone());
        }
        let end = (end_inclusive + 1).min(self.bytes.len() as u64) as usize;
        let from = start as usize;
        if from >= self.bytes.len() {
            return Err(RangeFetchError::EndPastPosition { at: start });
        }
        Ok(FetchedRange {
            data: self.bytes[from..end].to_vec(),
            total_len: if self.hide_total_len {
                None
            } else {
                Some(self.bytes.len() as u64)
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read as _, Seek as _};

    fn source(bytes: Vec<u8>) -> RangedMediaSource {
        RangedMediaSource::new(
            Arc::new(MemoryFetcher {
                bytes: Arc::new(bytes),
                fail_at: Vec::new(),
                hide_total_len: false,
            }),
            Arc::new(ByteSpanCache::with_budget(None)),
            "https://example.com/song",
            Vec::new(),
            "song",
            StreamIdentity {
                itag: 140,
                content_length: None,
            },
        )
    }

    #[test]
    fn reads_across_chunk_boundaries() {
        // A stream larger than one 512 KB chunk, read in small pieces.
        let mut bytes = Vec::with_capacity(SUBRANGE_BYTES as usize + 1024);
        for i in 0..(SUBRANGE_BYTES + 1024) {
            bytes.push((i % 251) as u8);
        }
        let mut src = source(bytes.clone());
        let mut buf = [0u8; 333]; // deliberately awkward
        let mut at = 0usize;
        loop {
            let n = src.read(&mut buf).unwrap();
            if n == 0 {
                break;
            }
            assert_eq!(&buf[..n], &bytes[at..at + n]);
            at += n;
        }
        assert_eq!(at, bytes.len());
    }

    #[test]
    fn seek_and_read_anywhere() {
        let bytes: Vec<u8> = (0..100_000u32).map(|i| (i % 256) as u8).collect();
        let mut src = source(bytes.clone());
        src.seek(io::SeekFrom::Start(90_000)).unwrap();
        let mut buf = [0u8; 100];
        src.read_exact(&mut buf).unwrap();
        assert_eq!(&buf, &bytes[90_000..90_100]);
        // Seek from the end.
        let end = src.seek(io::SeekFrom::End(-10)).unwrap();
        assert_eq!(end, 99_990);
        // Reads past the end are EOF.
        src.seek(io::SeekFrom::Start(100_000)).unwrap();
        assert_eq!(src.read(&mut buf).unwrap(), 0);
    }

    #[test]
    fn end_past_position_sets_the_length() {
        // The identity didn't know the length and the fetcher won't say:
        // the second chunk 416s and the reader stops there cleanly — the
        // upstream `ended = true; length = offset` path
        // (StreamPcmDecoder.kt:182-188).
        let bytes: Vec<u8> = (0..SUBRANGE_BYTES + 2048)
            .map(|i| (i % 13) as u8)
            .collect();
        let fetcher = MemoryFetcher {
            bytes: Arc::new(bytes.clone()),
            fail_at: vec![(
                SUBRANGE_BYTES,
                RangeFetchError::EndPastPosition {
                    at: SUBRANGE_BYTES,
                },
            )],
            hide_total_len: true,
        };
        let mut src = RangedMediaSource::new(
            Arc::new(fetcher),
            Arc::new(ByteSpanCache::with_budget(None)),
            "u",
            Vec::new(),
            "k",
            StreamIdentity {
                itag: 1,
                content_length: None,
            },
        );
        // Read the whole thing in awkward sizes: the first chunk serves,
        // the second refuses and the length lands at the refusal point.
        let mut buf = [0u8; 4096];
        let mut got = Vec::new();
        loop {
            let n = src.read(&mut buf).unwrap();
            if n == 0 {
                break;
            }
            got.extend_from_slice(&buf[..n]);
        }
        assert_eq!(got.len(), SUBRANGE_BYTES as usize);
        assert_eq!(got, bytes[..SUBRANGE_BYTES as usize]);
    }

    #[test]
    fn cached_bytes_are_never_fetched_twice() {
        // The player cache contract: isCached guard before warmStart.
        let bytes: Vec<u8> = (0..SUBRANGE_BYTES).map(|i| (i % 97) as u8).collect();
        let cache = Arc::new(ByteSpanCache::with_budget(None));
        let identity = StreamIdentity {
            itag: 1,
            content_length: Some(SUBRANGE_BYTES),
        };
        let mut src = RangedMediaSource::new(
            Arc::new(MemoryFetcher {
                bytes: Arc::new(bytes.clone()),
                fail_at: Vec::new(),
                hide_total_len: true,
            }),
            cache.clone(),
            "u",
            Vec::new(),
            "k",
            identity.clone(),
        );
        let mut buf = [0u8; 4096];
        src.read_exact(&mut buf).unwrap();
        assert!(cache.is_cached("k", 0, SUBRANGE_BYTES as usize));
        // A second source over the same cache: zero fetches needed.
        let mut again = RangedMediaSource::new(
            Arc::new(MemoryFetcher {
                bytes: Arc::new(bytes),
                fail_at: vec![(0, RangeFetchError::Io("would fetch".into()))],
                hide_total_len: true,
            }),
            cache,
            "u",
            Vec::new(),
            "k",
            identity,
        );
        again.seek(io::SeekFrom::Start(0)).unwrap();
        again.read_exact(&mut buf).unwrap(); // no error: served from cache
    }
}
