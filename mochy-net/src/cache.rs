//! Response caching.
//!
//! Two layers, both absorbing the upstream 50 MB okhttp `Cache`
//! (InnerTube.kt:111-116, `java.io.tmpdir/http_cache`):
//!
//! - [`ResponseCache`]: an in-memory `moka` (TinyLFU) cache of response
//!   bodies, **byte-range aware as a stub**: a cached full body serves any
//!   `Range` request by slicing; a range that falls outside a cached body
//!   is a miss. Phase 08 completes the ranged semantics (206 re-validation
//!   and coalescing) on top of this seam.
//! - [`DiskCache`]: the 50 MB LRU disk cache reserved for innertube GETs —
//!   files named after a stable hash of `(method, url)` with the full key
//!   embedded in the file header (so a reopened cache can serve gets),
//!   size accounting and least-recently-used eviction (upstream let okhttp
//!   do this; the desktop port owns it because reqwest has no disk cache).

use std::collections::HashMap;
use std::fs;
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::SystemTime;

use thiserror::Error;

/// Upstream size of the innertube HTTP cache (InnerTube.kt:114).
pub const INNERTUBE_DISK_CACHE_BYTES: u64 = 50 * 1024 * 1024;

/// A (possibly partial) cached body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ByteRange {
    /// First byte of `data` within the full body.
    pub start: u64,
    /// The bytes (empty for a 0-length body).
    pub data: Vec<u8>,
    /// Total length of the full body when known (`None` for a partial
    /// response without a total).
    pub total_len: Option<u64>,
}

impl ByteRange {
    /// A full-body entry.
    pub fn full(data: Vec<u8>) -> ByteRange {
        ByteRange {
            start: 0,
            total_len: Some(data.len() as u64),
            data,
        }
    }

    /// Serve `range` (start, end inclusive) out of this entry, if covered.
    pub fn slice(&self, start: u64, end_inclusive: u64) -> Option<&[u8]> {
        let end = end_inclusive.saturating_add(1);
        if start >= self.start && end <= self.start + self.data.len() as u64 && start < end {
            let from = (start - self.start) as usize;
            let to = (end - self.start) as usize;
            Some(&self.data[from..to])
        } else {
            None
        }
    }
}

/// In-memory response cache (moka/TinyLFU). Byte-range aware stub: full
/// bodies serve ranges by slicing; uncovered ranges miss.
#[derive(Debug, Clone)]
pub struct ResponseCache {
    inner: moka::sync::Cache<String, ByteRange>,
}

impl Default for ResponseCache {
    fn default() -> Self {
        ResponseCache::new(1024)
    }
}

impl ResponseCache {
    /// A cache holding at most `max_entries` bodies.
    pub fn new(max_entries: u64) -> ResponseCache {
        ResponseCache {
            inner: moka::sync::Cache::builder()
                .max_capacity(max_entries)
                .build(),
        }
    }

    fn key(method: &str, url: &str) -> String {
        format!("{method} {url}")
    }

    /// Store a full response body.
    pub fn put(&self, method: &str, url: &str, body: Vec<u8>) {
        self.inner
            .insert(Self::key(method, url), ByteRange::full(body));
    }

    /// Fetch a full body.
    pub fn get(&self, method: &str, url: &str) -> Option<Vec<u8>> {
        self.inner
            .get(&Self::key(method, url))
            .map(|entry| entry.data)
    }

    /// Fetch a byte range `[start, end_inclusive]` from a cached full body
    /// (the Phase 08 ranged-GET seam).
    pub fn get_range(
        &self,
        method: &str,
        url: &str,
        start: u64,
        end_inclusive: u64,
    ) -> Option<Vec<u8>> {
        self.inner
            .get(&Self::key(method, url))
            .and_then(|entry| entry.slice(start, end_inclusive).map(<[u8]>::to_vec))
    }

    /// Number of cached entries.
    pub fn entry_count(&self) -> u64 {
        self.inner.entry_count()
    }
}

/// Disk cache errors.
#[derive(Debug, Error)]
pub enum DiskCacheError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("cache dir {0} is a file")]
    NotADirectory(PathBuf),
    #[error("corrupt cache entry {0}")]
    Corrupt(String),
}

/// File layout of one entry: `u32 key-len (LE) | key bytes | body`.
/// The embedded key lets a reopened cache serve `get`s (the file name alone
/// is only a hash of the key).
#[derive(Debug)]
struct EntryFile {
    key: String,
    body: Vec<u8>,
}

impl EntryFile {
    fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(4 + self.key.len() + self.body.len());
        out.extend_from_slice(&(self.key.len() as u32).to_le_bytes());
        out.extend_from_slice(self.key.as_bytes());
        out.extend_from_slice(&self.body);
        out
    }

    fn decode(bytes: Vec<u8>) -> Result<EntryFile, DiskCacheError> {
        if bytes.len() < 4 {
            return Err(DiskCacheError::Corrupt("truncated header".into()));
        }
        let key_len = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
        if bytes.len() < 4 + key_len {
            return Err(DiskCacheError::Corrupt("truncated key".into()));
        }
        let key = String::from_utf8_lossy(&bytes[4..4 + key_len]).into_owned();
        let body = bytes[4 + key_len..].to_vec();
        Ok(EntryFile { key, body })
    }
}

/// An LRU disk cache with a byte budget. Keyed by `(method, url)`; eviction
/// removes least-recently-used files until the budget is met. Writes are
/// atomic (temp + rename), mirroring the settings store.
#[derive(Debug)]
pub struct DiskCache {
    dir: PathBuf,
    budget: u64,
    /// cache key -> (file name, bytes, last access).
    index: Mutex<HashMap<String, (String, u64, SystemTime)>>,
}

impl DiskCache {
    /// Open (or create) the cache at `dir` with a byte budget
    /// (upstream: 50 MB). Existing files are re-indexed by mtime.
    pub fn open(dir: impl Into<PathBuf>, budget: u64) -> Result<DiskCache, DiskCacheError> {
        let dir = dir.into();
        if dir.is_file() {
            return Err(DiskCacheError::NotADirectory(dir));
        }
        fs::create_dir_all(&dir)?;
        let mut index = HashMap::new();
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "tmp") {
                let _ = fs::remove_file(&path); // torn write
                continue;
            }
            let meta = entry.metadata()?;
            let accessed = meta
                .accessed()
                .or_else(|_| meta.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            // decode just the key header (the body stays on disk)
            let bytes = fs::read(&path)?;
            let entry_file = EntryFile::decode(bytes)
                .map_err(|e| DiskCacheError::Corrupt(format!("{name}: {e}")))?;
            index.insert(entry_file.key, (name.to_owned(), meta.len(), accessed));
        }
        let cache = DiskCache {
            dir,
            budget,
            index: Mutex::new(index),
        };
        cache.evict_to_budget()?;
        Ok(cache)
    }

    /// The upstream innertube cache: 50 MB.
    pub fn innertube(dir: impl Into<PathBuf>) -> Result<DiskCache, DiskCacheError> {
        DiskCache::open(dir, INNERTUBE_DISK_CACHE_BYTES)
    }

    fn file_name(key: &str) -> String {
        // FxHash-style: any stable hash works for cache keys (not security)
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        key.hash(&mut hasher);
        format!("{:016x}.body", hasher.finish())
    }

    fn key(method: &str, url: &str) -> String {
        format!("{method} {url}")
    }

    /// Store a body. Returns `false` if the body alone exceeds the budget
    /// (never cached, nothing evicted for it).
    pub fn put(&self, method: &str, url: &str, body: &[u8]) -> Result<bool, DiskCacheError> {
        let key = Self::key(method, url);
        let bytes = body.len() as u64;
        if bytes > self.budget {
            return Ok(false);
        }
        let name = Self::file_name(&key);
        let path = self.dir.join(&name);
        let tmp = self.dir.join(format!("{name}.tmp"));
        {
            let mut file = fs::File::create(&tmp)?;
            file.write_all(
                &EntryFile {
                    key: key.clone(),
                    body: body.to_vec(),
                }
                .encode(),
            )?;
            file.sync_all()?;
        }
        fs::rename(&tmp, &path)?;
        let mut index = self.index.lock().expect("disk cache lock poisoned");
        index.insert(key, (name, bytes, SystemTime::now()));
        drop(index);
        self.evict_to_budget()?;
        Ok(true)
    }

    /// Fetch a body (touching its LRU clock).
    pub fn get(&self, method: &str, url: &str) -> Option<Vec<u8>> {
        let key = Self::key(method, url);
        let now = SystemTime::now();
        let name = {
            let mut index = self.index.lock().expect("disk cache lock poisoned");
            let (name, _, accessed) = index.get_mut(&key)?;
            *accessed = now;
            name.clone()
        };
        let bytes = fs::read(self.dir.join(name)).ok()?;
        EntryFile::decode(bytes).ok().map(|entry| entry.body)
    }

    /// Current total size in bytes.
    pub fn size_bytes(&self) -> u64 {
        self.index
            .lock()
            .expect("disk cache lock poisoned")
            .values()
            .map(|(_, bytes, _)| *bytes)
            .sum()
    }

    /// Number of cached entries.
    pub fn len(&self) -> usize {
        self.index.lock().expect("disk cache lock poisoned").len()
    }

    /// Whether the cache holds nothing.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Remove everything.
    pub fn clear(&self) -> Result<(), DiskCacheError> {
        let mut index = self.index.lock().expect("disk cache lock poisoned");
        for (name, _, _) in index.values() {
            let _ = fs::remove_file(self.dir.join(name));
        }
        index.clear();
        Ok(())
    }

    fn evict_to_budget(&self) -> Result<(), DiskCacheError> {
        let mut index = self.index.lock().expect("disk cache lock poisoned");
        while index.values().map(|(_, b, _)| *b).sum::<u64>() > self.budget {
            let evict = index
                .iter()
                .min_by_key(|(_, (_, _, accessed))| *accessed)
                .map(|(k, _)| k.clone());
            let Some(evict) = evict else { break };
            if let Some((name, _, _)) = index.remove(&evict) {
                let _ = fs::remove_file(self.dir.join(name));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mochy-net-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn memory_cache_round_trip() {
        let cache = ResponseCache::new(16);
        cache.put("GET", "https://example.com/a", b"hello world".to_vec());
        assert_eq!(
            cache.get("GET", "https://example.com/a"),
            Some(b"hello world".to_vec())
        );
        assert_eq!(cache.get("GET", "https://example.com/b"), None);
        assert_eq!(
            cache.get("POST", "https://example.com/a"),
            None,
            "methods are part of the key"
        );
    }

    #[test]
    fn memory_cache_serves_ranges_from_full_bodies() {
        let cache = ResponseCache::new(16);
        let body: Vec<u8> = (0..=255u8).collect();
        cache.put("GET", "https://example.com/f", body);
        assert_eq!(
            cache.get_range("GET", "https://example.com/f", 10, 19),
            Some((10..=19u8).collect::<Vec<u8>>())
        );
        assert_eq!(
            cache.get_range("GET", "https://example.com/f", 250, 259),
            None,
            "past the end"
        );
        assert_eq!(
            cache.get_range("GET", "https://example.com/f", 0, 255),
            Some((0..=255u8).collect::<Vec<u8>>())
        );
    }

    #[test]
    fn byte_range_slice_semantics() {
        let entry = ByteRange::full(b"abcdef".to_vec());
        assert_eq!(entry.slice(0, 2), Some(&b"abc"[..]));
        assert_eq!(entry.slice(3, 5), Some(&b"def"[..]));
        assert_eq!(entry.slice(4, 9), None);
        assert_eq!(entry.slice(6, 6), None);
    }

    #[test]
    fn disk_cache_round_trip_and_budget() {
        let dir = temp_dir("disk");
        let cache = DiskCache::open(&dir, 100).unwrap();
        assert!(cache.is_empty());
        assert!(cache.put("GET", "https://a/1", &[7u8; 40]).unwrap());
        assert!(cache.put("GET", "https://a/2", &[7u8; 40]).unwrap());
        assert_eq!(cache.size_bytes(), 80);
        assert_eq!(cache.get("GET", "https://a/1"), Some(vec![7u8; 40]));

        // over budget -> least recently used evicted
        assert!(cache.put("GET", "https://a/3", &[9u8; 40]).unwrap());
        assert_eq!(cache.size_bytes(), 80, "budget respected");
        assert_eq!(cache.len(), 2);
        // a/1 was accessed most recently before the insert -> a/2 evicted
        assert_eq!(cache.get("GET", "https://a/2"), None);
        assert!(cache.get("GET", "https://a/1").is_some());
        assert!(cache.get("GET", "https://a/3").is_some());

        // oversized body refused outright
        assert!(!cache.put("GET", "https://a/huge", &[1u8; 101]).unwrap());
        cache.clear().unwrap();
        assert!(cache.is_empty());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn disk_cache_reindexes_existing_files() {
        let dir = temp_dir("disk2");
        {
            let cache = DiskCache::open(&dir, 1000).unwrap();
            cache.put("GET", "https://a/keep", b"kept").unwrap();
        }
        let cache = DiskCache::open(&dir, 1000).unwrap();
        assert_eq!(cache.get("GET", "https://a/keep"), Some(b"kept".to_vec()));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn innertube_budget_is_50_mb() {
        assert_eq!(INNERTUBE_DISK_CACHE_BYTES, 50 * 1024 * 1024);
    }
}
