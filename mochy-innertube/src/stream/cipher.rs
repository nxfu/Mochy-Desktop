//! The signature cipher — the port of `utils/cipher/` (4 files).
//!
//! - [`PlayerJsFetcher`]: fetch the current player script
//!   (`iframe_api` → player hash →
//!   `/s/player/{hash}/player_ias.vflset/en_GB/base.js`), cached on
//!   disk for 6 hours (PlayerJsFetcher.kt:14-193);
//! - [`extract_sig_function_info`] / [`extract_n_function_info`]:
//!   the regex catalogs that name the deobfuscation functions inside
//!   the player script (FunctionNameExtractor.kt:10-30);
//! - [`CipherRuntime`]: the CipherWebView port — evaluate the player
//!   script in the QuickJS engine with the two functions exported to
//!   globals (the `})(_yt_player);` splice, CipherWebView.kt:65-84),
//!   then call them (`deobfuscateSig`/`transformN`,
//!   CipherWebView.kt:92-131);
//! - [`Cipher`]: the CipherDeobfuscator port — the cached runtime,
//!   the signature-cipher URL parser and the retry-with-fresh-JS
//!   flow (CipherDeobfuscator.kt:24-100).
//!
//! The signature timestamp (`signatureTimestamp:(\d+)` inside the
//! player script) is exposed here too; the NewPipe-shaped STS entry
//! point lives in [`super::newpipe`] (rustypipe's deobfuscator uses
//! the same regex over the same file).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use regex::Regex;

use super::jsengine::{Engine, JsError};
use crate::error::InnertubeError;

/// `https://www.youtube.com/iframe_api` (PlayerJsFetcher.kt:16).
const IFRAME_API_URL: &str = "https://www.youtube.com/iframe_api";
/// The player-script URL template (PlayerJsFetcher.kt:17).
const PLAYER_JS_URL_TEMPLATE: &str =
    "https://www.youtube.com/s/player/{}/player_ias.vflset/en_GB/base.js";
/// How long a downloaded player script is reused before it is
/// re-fetched (PlayerJsFetcher.kt:19 — 6 hours).
pub const CACHE_TTL: Duration = Duration::from_secs(6 * 60 * 60);
/// The plain browser UA the fetcher identifies with
/// (PlayerJsFetcher.kt:165/183).
const FETCH_UA: &str = "Mozilla/5.0";

/// `CipherCacheStatus` (PlayerJsFetcher.kt:76-88): a snapshot of the
/// on-disk player-script cache for the diagnostics surface.
#[derive(Debug, Clone, PartialEq)]
pub struct CipherCacheStatus {
    /// YouTube's identifier for the cached player script.
    pub player_hash: Option<String>,
    /// When the script was downloaded (epoch ms), if anything is
    /// cached.
    pub last_updated_ms: Option<u64>,
    /// Size of the cached script.
    pub size_bytes: u64,
}

impl CipherCacheStatus {
    /// Milliseconds until the cache goes stale; zero once expired.
    pub fn remaining_ms(&self, now_ms: u64) -> u64 {
        self.last_updated_ms
            .map(|updated| (updated + CACHE_TTL.as_millis() as u64).saturating_sub(now_ms))
            .unwrap_or(0)
    }

    pub fn is_expired(&self, now_ms: u64) -> bool {
        self.player_hash.is_none()
            || self.last_updated_ms.is_none()
            || self.remaining_ms(now_ms) == 0
    }
}

/// The player-script fetcher + disk cache
/// (PlayerJsFetcher.kt:14-193).
pub struct PlayerJsFetcher {
    cache_dir: PathBuf,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

impl PlayerJsFetcher {
    /// A fetcher caching under `cache_dir` (upstream:
    /// `filesDir/cipher_cache`).
    pub fn new(cache_dir: impl Into<PathBuf>) -> Self {
        PlayerJsFetcher {
            cache_dir: cache_dir.into(),
        }
    }

    fn hash_file(&self) -> PathBuf {
        self.cache_dir.join("current_hash.txt")
    }

    fn cache_file(&self, hash: &str) -> PathBuf {
        self.cache_dir.join(format!("player_{hash}.js"))
    }

    /// `getPlayerJs` (PlayerJsFetcher.kt:33-67): the cached script
    /// while fresh, else a fresh download. Returns `(script, hash)`.
    pub async fn get_player_js(
        &self,
        transport: &crate::transport::InnerTubeTransport,
        force_refresh: bool,
    ) -> Result<Option<(String, String)>, InnertubeError> {
        let _ = std::fs::create_dir_all(&self.cache_dir);
        if !force_refresh && let Some(cached) = self.read_from_cache() {
            tracing::debug!(target: "mochy_cipher", hash = %cached.1, "using cached player js");
            return Ok(Some(cached));
        }
        let Some(hash) = self.fetch_player_hash(transport).await? else {
            tracing::error!(target: "mochy_cipher", "could not extract the player hash from iframe_api");
            return Ok(None);
        };
        let Some(player_js) = self.download_player_js(transport, &hash).await? else {
            return Ok(None);
        };
        self.write_to_cache(&hash, &player_js);
        Ok(Some((player_js, hash)))
    }

    /// `readCacheStatus` (PlayerJsFetcher.kt:91-106).
    pub fn read_cache_status(&self) -> CipherCacheStatus {
        let text = match std::fs::read_to_string(self.hash_file()) {
            Ok(text) => text,
            Err(_) => {
                return CipherCacheStatus {
                    player_hash: None,
                    last_updated_ms: None,
                    size_bytes: 0,
                };
            }
        };
        let mut lines = text.split('\n');
        let hash = lines.next().filter(|line| !line.is_empty());
        let timestamp = lines
            .next()
            .and_then(|line| line.trim().parse::<u64>().ok());
        let size_bytes = hash
            .and_then(|hash| std::fs::metadata(self.cache_file(hash)).ok())
            .map(|meta| meta.len())
            .unwrap_or(0);
        CipherCacheStatus {
            player_hash: hash.map(str::to_owned),
            last_updated_ms: timestamp,
            size_bytes,
        }
    }

    /// `invalidateCache` (PlayerJsFetcher.kt:108-118).
    pub fn invalidate_cache(&self) {
        if let Ok(entries) = std::fs::read_dir(&self.cache_dir) {
            for entry in entries.flatten() {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }

    /// `readFromCache` (PlayerJsFetcher.kt:120-147).
    fn read_from_cache(&self) -> Option<(String, String)> {
        let text = std::fs::read_to_string(self.hash_file()).ok()?;
        let mut lines = text.split('\n');
        let hash = lines.next()?.trim();
        let timestamp: u64 = lines.next()?.trim().parse().ok()?;
        if now_ms().saturating_sub(timestamp) > CACHE_TTL.as_millis() as u64 {
            return None;
        }
        let player_js = std::fs::read_to_string(self.cache_file(hash)).ok()?;
        if player_js.is_empty() {
            return None;
        }
        Some((player_js, hash.to_owned()))
    }

    /// `writeToCache` (PlayerJsFetcher.kt:149-160).
    fn write_to_cache(&self, hash: &str, player_js: &str) {
        if let Ok(entries) = std::fs::read_dir(&self.cache_dir) {
            for entry in entries.flatten() {
                if entry.file_name().to_string_lossy().starts_with("player_") {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
        if std::fs::write(self.cache_file(hash), player_js).is_ok() {
            let _ = std::fs::write(self.hash_file(), format!("{hash}\n{}", now_ms()));
        }
    }

    /// `fetchPlayerHash` (PlayerJsFetcher.kt:162-177): the iframe API
    /// names the current player script.
    async fn fetch_player_hash(
        &self,
        transport: &crate::transport::InnerTubeTransport,
    ) -> Result<Option<String>, InnertubeError> {
        let body = transport.raw_text_get(IFRAME_API_URL, FETCH_UA).await?;
        let hash_regex = Regex::new(r"/s/player/([a-zA-Z0-9_-]+)/").unwrap();
        Ok(hash_regex
            .captures(&body)
            .map(|captures| captures[1].to_owned()))
    }

    /// `downloadPlayerJs` (PlayerJsFetcher.kt:179-193).
    async fn download_player_js(
        &self,
        transport: &crate::transport::InnerTubeTransport,
        hash: &str,
    ) -> Result<Option<String>, InnertubeError> {
        let url = PLAYER_JS_URL_TEMPLATE.replace("{}", hash);
        Ok(Some(transport.raw_text_get(&url, FETCH_UA).await?))
    }
}

/// `SigFunctionInfo` (FunctionNameExtractor.kt:32-35).
#[derive(Debug, Clone, PartialEq)]
pub struct SigFunctionInfo {
    pub name: String,
    pub constant_arg: Option<i64>,
}

/// `NFunctionInfo` (FunctionNameExtractor.kt:37-40).
#[derive(Debug, Clone, PartialEq)]
pub struct NFunctionInfo {
    pub name: String,
    pub array_index: Option<usize>,
}

/// The signature-function regex catalog
/// (FunctionNameExtractor.kt:10-19, in order).
const SIG_FUNCTION_PATTERNS: &[&str] = &[
    r"&&\s*\(\s*[a-zA-Z0-9$]+\s*=\s*([a-zA-Z0-9$]+)\s*\(\s*(\d+)\s*,\s*decodeURIComponent\s*\(\s*[a-zA-Z0-9$]+\s*\)",
    r"\b[cs]\s*&&\s*[adf]\.set\([^,]+\s*,\s*encodeURIComponent\(([a-zA-Z0-9$]+)\(",
    r"\b[a-zA-Z0-9]+\s*&&\s*[a-zA-Z0-9]+\.set\([^,]+\s*,\s*encodeURIComponent\(([a-zA-Z0-9$]+)\(",
    r"\bm=([a-zA-Z0-9$]{2,})\(decodeURIComponent\(h\.s\)\)",
    r"\bc\s*&&\s*d\.set\([^,]+\s*,\s*(?:encodeURIComponent\s*\()([a-zA-Z0-9$]+)\(",
    r"\bc\s*&&\s*[a-z]\.set\([^,]+\s*,\s*encodeURIComponent\(([a-zA-Z0-9$]+)\(",
];

/// The n-function regex catalog
/// (FunctionNameExtractor.kt:21-30, in order).
///
/// Pattern 2 upstream asserts the call argument equals the assigned
/// variable through a **backreference** (`\(\1\)`), which the Rust
/// `regex` crate does not support; the port captures both identifiers
/// and compares them in code (same accepted/rejected inputs).
const N_FUNCTION_PATTERNS: &[&str] = &[
    r#"\.get\("n"\)\)&&\(b=([a-zA-Z0-9$]+)(?:\[(\d+)\])?\(([a-zA-Z0-9])\)"#,
    r#"\.get\("n"\)\)\s*&&\s*\(([a-zA-Z0-9$]+)\s*=\s*([a-zA-Z0-9$]+)(?:\[(\d+)\])?\(\s*([a-zA-Z0-9$]+)\s*\)"#,
    r"\(\s*([a-zA-Z0-9$]+)\s*=\s*String\.fromCharCode\(110\)",
    r"([a-zA-Z0-9$]+)\s*=\s*function\([a-zA-Z0-9]\)\s*\{[^}]*?enhanced_except_",
];

/// `extractSigFunctionInfo` (FunctionNameExtractor.kt:42-54). A
/// pattern that cannot compile is skipped, never fatal.
pub fn extract_sig_function_info(player_js: &str) -> Option<SigFunctionInfo> {
    for (index, pattern) in SIG_FUNCTION_PATTERNS.iter().enumerate() {
        let Ok(regex) = Regex::new(pattern) else {
            tracing::warn!(target: "mochy_cipher", pattern = index, "sig pattern does not compile");
            continue;
        };
        if let Some(captures) = regex.captures(player_js) {
            let name = captures.get(1)?.as_str().to_owned();
            let constant_arg = captures.get(2).and_then(|m| m.as_str().parse().ok());
            tracing::debug!(target: "mochy_cipher", pattern = index, %name, ?constant_arg, "sig function found");
            return Some(SigFunctionInfo { name, constant_arg });
        }
    }
    tracing::error!(target: "mochy_cipher", "could not find the signature deobfuscation function");
    None
}

/// `extractNFunctionInfo` (FunctionNameExtractor.kt:56-85): pattern 1
/// names the function in group 1 (array index in group 2), pattern 2
/// in group 2 (array index in group 3 — and the call argument in
/// group 4 must equal the assignee, the backreference port), patterns
/// 3-4 in group 1.
pub fn extract_n_function_info(player_js: &str) -> Option<NFunctionInfo> {
    for (index, pattern) in N_FUNCTION_PATTERNS.iter().enumerate() {
        let Ok(regex) = Regex::new(pattern) else {
            tracing::warn!(target: "mochy_cipher", pattern = index, "n pattern does not compile");
            continue;
        };
        if let Some(captures) = regex.captures(player_js) {
            let (name, array_index) = match index {
                0 => (captures.get(1)?, captures.get(2)),
                1 => {
                    // the backreference port: the assigned identifier
                    // must be what the function is called with
                    let assignee = captures.get(1)?.as_str();
                    let argument = captures.get(4)?.as_str();
                    if assignee != argument {
                        continue;
                    }
                    (captures.get(2)?, captures.get(3))
                }
                _ => (captures.get(1)?, None),
            };
            let name = name.as_str().to_owned();
            let array_index = array_index.and_then(|m| m.as_str().parse().ok());
            tracing::debug!(target: "mochy_cipher", pattern = index, %name, ?array_index, "n function found");
            return Some(NFunctionInfo { name, array_index });
        }
    }
    tracing::error!(target: "mochy_cipher", "could not find the n-transform function");
    None
}

/// The signature timestamp inside the player script — the same
/// `signatureTimestamp[=:](\d+)` rustypipe's deobfuscator extracts
/// over the same file (rustypipe src/deobfuscate.rs `get_sts`; the
/// NewPipe path this replaces reads it the same way).
pub fn signature_timestamp(player_js: &str) -> Option<i32> {
    Regex::new(r"signatureTimestamp[=:](\d+)")
        .ok()?
        .captures(player_js)?
        .get(1)?
        .as_str()
        .parse()
        .ok()
}

/// One loaded player script in the QuickJS engine — the
/// `CipherWebView` port (CipherWebView.kt:19-320).
///
/// The upstream splice exports the two functions by name inside the
/// player IIFE (`})(_yt_player);` → `; window._cipherSigFunc = …;
/// })(_yt_player);`, CipherWebView.kt:79-84); the port writes them to
/// `globalThis.__mochySigFunc` / `globalThis.__mochyNFunc` and then
/// runs the same brute-force discovery over the engine's globals the
/// HTML's `discoverAndInit` performs (CipherWebView.kt:133-186).
pub struct CipherRuntime {
    engine: Arc<Engine>,
    n_function_available: bool,
    #[allow(dead_code)] // surfaced through n_function_available
    discovered_n_function: Option<String>,
}

impl CipherRuntime {
    /// `CipherWebView.create` (CipherWebView.kt:306-318): evaluate the
    /// player script with the exports spliced in, then discover the
    /// n-function.
    pub async fn create(engine: Arc<Engine>, player_js: &str) -> Result<Self, JsError> {
        let sig_info = extract_sig_function_info(player_js);
        let n_info = extract_n_function_info(player_js);
        tracing::debug!(target: "mochy_cipher",
            sig = ?sig_info.as_ref().map(|info| info.name.clone()),
            sig_arg = ?sig_info.as_ref().and_then(|info| info.constant_arg),
            n = ?n_info.as_ref().map(|info| info.name.clone()),
            "creating the cipher runtime over {} chars of player js", player_js.len());

        // the export splice (CipherWebView.kt:65-84)
        let mut exports = String::new();
        if let Some(sig) = &sig_info {
            exports.push_str(&format!(
                " globalThis.__mochySigFunc = typeof {name} !== 'undefined' ? {name} : null;",
                name = sig.name
            ));
        }
        if let Some(n) = &n_info {
            let expression = match n.array_index {
                Some(index) => format!("{}[{}]", n.name, index),
                None => n.name.clone(),
            };
            exports.push_str(&format!(
                " globalThis.__mochyNFunc = typeof {name} !== 'undefined' ? {expression} : null;",
                name = n.name,
            ));
        }
        let mut modified = player_js.to_owned();
        if !exports.is_empty() {
            modified = match player_js.rfind("})(_yt_player);") {
                Some(position) => format!(
                    "{};{}{}",
                    &player_js[..position],
                    exports,
                    &player_js[position..]
                ),
                None => {
                    // a player script without the IIFE tail cannot be
                    // spliced; the functions stay unavailable
                    tracing::warn!(target: "mochy_cipher", "player js has no iife tail to splice exports into");
                    player_js.to_owned()
                }
            };
        }
        engine.eval_sloppy::<()>(modified).await?;

        // the brute-force discovery (CipherWebView.kt:133-186):
        // every one-argument global that transforms the test input
        // and answers with a `_w8_`-carrying string is the n-function
        let discovered: Option<String> = engine
            .eval(
                "(function () {
                     var nFunc = globalThis.__mochyNFunc;
                     var found = '';
                     if (typeof nFunc === 'function') {
                         try {
                             var testResult = nFunc('T2Xw3pWQ_Wk0xbOg');
                             if (typeof testResult === 'string' && testResult !== 'T2Xw3pWQ_Wk0xbOg') {
                                 found = 'injected';
                             } else {
                                 globalThis.__mochyNFunc = null;
                             }
                         } catch (e) {
                             globalThis.__mochyNFunc = null;
                         }
                     }
                     if (!found) {
                         try {
                             var testInput = 'T2Xw3pWQ_Wk0xbOg';
                             var keys = Object.getOwnPropertyNames(globalThis);
                             for (var i = 0; i < keys.length; i++) {
                                 var key = keys[i];
                                 if (key.indexOf('webkit') === 0 || key === '__hostConsole' ||
                                     key === '__mochySigFunc' || key === '__mochyNFunc') continue;
                                 var fn;
                                 try { fn = globalThis[key]; } catch (e) { continue; }
                                 if (typeof fn !== 'function' || fn.length !== 1) continue;
                                 var result;
                                 try { result = fn(testInput); } catch (e) { continue; }
                                 if (typeof result === 'string' && result !== testInput && result.length > 5 &&
                                     result.indexOf('_w8_') >= 0) {
                                     globalThis.__mochyNFunc = fn;
                                     found = key;
                                     break;
                                 }
                             }
                         } catch (e) { /* brute force failed */ }
                     }
                     return found || null;
                 })()",
            )
            .await?;
        let n_function_available: bool = engine
            .eval("typeof globalThis.__mochyNFunc === 'function'")
            .await?;
        if !n_function_available {
            tracing::error!(target: "mochy_cipher", "n-transform function was not discovered at init time");
        }
        Ok(CipherRuntime {
            engine,
            n_function_available,
            discovered_n_function: discovered,
        })
    }

    /// `nFunctionAvailable` (CipherWebView.kt:31).
    pub fn n_function_available(&self) -> bool {
        self.n_function_available
    }

    /// `deobfuscateSignature` (CipherWebView.kt:224-239 +
    /// the HTML's `deobfuscateSig`, CipherWebView.kt:92-113): the
    /// function takes the obfuscated signature, optionally behind a
    /// constant first argument.
    pub async fn deobfuscate_signature(&self, obfuscated_sig: &str) -> Result<String, JsError> {
        let literal = serde_json::to_string(obfuscated_sig).unwrap_or_default();
        let code = format!(
            "(function () {{
                 var func = globalThis.__mochySigFunc;
                 if (typeof func !== 'function') {{
                     throw new Error('sig func not found on the engine (type: ' + typeof func + ')');
                 }}
                 var out = func({literal});
                 if (out === undefined || out === null) {{
                     throw new Error('sig function returned null/undefined');
                 }}
                 return String(out);
             }})()"
        );
        self.engine.eval(code).await
    }

    /// `transformN` (CipherWebView.kt:255-269 + the HTML's
    /// `transformN`, CipherWebView.kt:115-131).
    pub async fn transform_n(&self, n_value: &str) -> Result<String, JsError> {
        if !self.n_function_available {
            return Err(JsError::Script(
                "n-transform function not discovered".into(),
            ));
        }
        let literal = serde_json::to_string(n_value).unwrap_or_default();
        let code = format!(
            "(function () {{
                 var out = globalThis.__mochyNFunc({literal});
                 if (out === undefined || out === null) {{
                     throw new Error('n-transform returned null/undefined');
                 }}
                 return String(out);
             }})()"
        );
        self.engine.eval(code).await
    }
}

/// The cached cipher — the `CipherDeobfuscator` port
/// (CipherDeobfuscator.kt:11-164): one runtime per player script,
/// rebuilt with fresh JS when a call fails
/// (`deobfuscateStreamUrl`'s retry, CipherDeobfuscator.kt:24-39).
pub struct Cipher {
    fetcher: PlayerJsFetcher,
    runtime: tokio::sync::Mutex<Option<Arc<CipherRuntime>>>,
    engine: Arc<Engine>,
}

impl Cipher {
    pub fn new(fetcher: PlayerJsFetcher, engine: Arc<Engine>) -> Self {
        Cipher {
            fetcher,
            runtime: tokio::sync::Mutex::const_new(None),
            engine,
        }
    }

    /// The fetcher's cache status (diagnostics).
    pub fn cache_status(&self) -> CipherCacheStatus {
        self.fetcher.read_cache_status()
    }

    /// The current player script (cached while fresh) — the
    /// NewPipe-shaped STS lookup reads it without building a cipher
    /// runtime first (NewPipe.kt:102-105 over the same file
    /// EjsNTransformSolver reuses PlayerJsFetcher for).
    pub async fn player_js(
        &self,
        transport: &crate::transport::InnerTubeTransport,
    ) -> Result<Option<String>, InnertubeError> {
        Ok(self
            .fetcher
            .get_player_js(transport, false)
            .await?
            .map(|(player_js, _hash)| player_js))
    }

    /// `getOrCreateWebView` (CipherDeobfuscator.kt:102-142): the
    /// cached runtime, or a fresh one over freshly fetched JS.
    async fn runtime(
        &self,
        transport: &crate::transport::InnerTubeTransport,
        force_refresh: bool,
    ) -> Result<Option<Arc<CipherRuntime>>, InnertubeError> {
        let mut guard = self.runtime.lock().await;
        if !force_refresh && let Some(runtime) = guard.as_ref() {
            return Ok(Some(Arc::clone(runtime)));
        }
        let Some((player_js, _hash)) = self.fetcher.get_player_js(transport, force_refresh).await?
        else {
            return Ok(None);
        };
        // a runtime that cannot even be created must not poison the
        // cache slot: surface the error and leave the slot empty
        match CipherRuntime::create(Arc::clone(&self.engine), &player_js).await {
            Ok(runtime) => {
                let runtime = Arc::new(runtime);
                *guard = Some(Arc::clone(&runtime));
                Ok(Some(runtime))
            }
            Err(error) => {
                tracing::error!(target: "mochy_cipher", error = %error, "cipher runtime creation failed");
                Ok(None)
            }
        }
    }

    /// `deobfuscateStreamUrl` (CipherDeobfuscator.kt:24-65): parse the
    /// signature-cipher query, deobfuscate `s`, and put it back as the
    /// `sp` parameter. A failure retries once with fresh JS
    /// (`isRetry = true` invalidates both caches) and then gives up
    /// with `None`.
    pub async fn deobfuscate_stream_url(
        &self,
        transport: &crate::transport::InnerTubeTransport,
        signature_cipher: &str,
        video_id: &str,
    ) -> Option<String> {
        match self
            .deobfuscate_internal(transport, signature_cipher, video_id, false)
            .await
        {
            Ok(url) => Some(url),
            Err(error) => {
                tracing::warn!(target: "mochy_cipher", error = %error, "cipher deobfuscation failed, retrying with fresh js");
                self.fetcher.invalidate_cache();
                self.runtime.lock().await.take();
                match self
                    .deobfuscate_internal(transport, signature_cipher, video_id, true)
                    .await
                {
                    Ok(url) => Some(url),
                    Err(retry_error) => {
                        tracing::error!(target: "mochy_cipher", error = %retry_error, "cipher deobfuscation retry also failed");
                        None
                    }
                }
            }
        }
    }

    async fn deobfuscate_internal(
        &self,
        transport: &crate::transport::InnerTubeTransport,
        signature_cipher: &str,
        video_id: &str,
        force_refresh: bool,
    ) -> Result<String, String> {
        // `deobfuscateInternal` (CipherDeobfuscator.kt:41-65)
        let params = parse_query_params(signature_cipher);
        let obfuscated_sig = params.get("s").ok_or_else(|| "no s parameter".to_owned())?;
        let sig_param = params.get("sp").map(String::as_str).unwrap_or("signature");
        let base_url = params
            .get("url")
            .ok_or_else(|| "no url parameter".to_owned())?;
        let Some(runtime) = self
            .runtime(transport, force_refresh)
            .await
            .map_err(|error| error.to_string())?
        else {
            return Err("no cipher runtime".into());
        };
        let deobfuscated = runtime
            .deobfuscate_signature(obfuscated_sig)
            .await
            .map_err(|error| error.to_string())?;
        let separator = if base_url.contains('?') { "&" } else { "?" };
        tracing::debug!(target: "mochy_cipher", video_id, "cipher deobfuscation succeeded");
        Ok(format!(
            "{base_url}{separator}{sig_param}={}",
            super::urlutil::uri_encode(&deobfuscated)
        ))
    }

    /// `transformNParamInUrl` (CipherDeobfuscator.kt:67-100): find the
    /// `n` parameter, transform it, put it back. No `n`, no runtime or
    /// a thrown transform all return the original URL.
    pub async fn transform_n_param_in_url(
        &self,
        transport: &crate::transport::InnerTubeTransport,
        url: &str,
    ) -> String {
        let Some((n_value, range)) = super::ejs::find_n_param(url) else {
            tracing::debug!(target: "mochy_cipher", "no 'n' parameter found, skipping transform");
            return url.to_owned();
        };
        let decoded = super::urlutil::uri_decode(&n_value);
        let runtime = match self.runtime(transport, false).await {
            Ok(Some(runtime)) => runtime,
            _ => return url.to_owned(),
        };
        if !runtime.n_function_available() {
            tracing::error!(target: "mochy_cipher", "n-transform function was not discovered at init time");
            return url.to_owned();
        }
        match runtime.transform_n(&decoded).await {
            Ok(transformed) => {
                let encoded = super::urlutil::uri_encode(&transformed);
                let mut out = url[..range.start].to_owned();
                out.push_str(&encoded);
                out.push_str(&url[range.end..]);
                out
            }
            Err(error) => {
                tracing::error!(target: "mochy_cipher", error = %error, "n-transform failed, returning the original url");
                url.to_owned()
            }
        }
    }
}

/// `parseQueryParams` (CipherDeobfuscator.kt:152-163) — the
/// signature-cipher blob is a `&`-joined query with percent escapes.
pub(crate) fn parse_query_params(query: &str) -> std::collections::HashMap<String, String> {
    let mut result = std::collections::HashMap::new();
    for pair in query.split('&') {
        if let Some(index) = pair.find('=')
            && index > 0
        {
            result.insert(
                super::urlutil::uri_decode(&pair[..index]),
                super::urlutil::uri_decode(&pair[index + 1..]),
            );
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal player-shaped script: the IIFE with
    /// `})(_yt_player);` tail, a signature function reached through the
    /// pattern-1 trigger and an n-transform function on the window
    /// (discoverable by the brute force when the pattern catalog
    /// misses it).
    const MINI_PLAYER: &str = r#"var _yt_player={};(function(g){
var window=this;
function Qq(sig){return sig.split("").reverse().join("");}
window.W7=function(n){
    var out="";
    for (var i=0;i<n.length;i++){out+=String.fromCharCode(n.charCodeAt(i)+1);}
    return "_w8_"+out;
};
var b;
b&&(b=Qq(24,decodeURIComponent(b)));
var c;
c&&window.W7&&(c=window.W7(decodeURIComponent(c)));
})(_yt_player);"#;

    #[test]
    fn extracts_the_sig_function_from_the_modern_pattern() {
        let info =
            extract_sig_function_info("xx&&(xx=sigfunc(24,decodeURIComponent(yy)))zz").unwrap();
        assert_eq!(info.name, "sigfunc");
        assert_eq!(info.constant_arg, Some(24));
    }

    #[test]
    fn extracts_the_sig_function_from_the_classic_patterns() {
        // pattern 2: c && f.set("alr", encodeURIComponent(XXX(
        let info =
            extract_sig_function_info(r#"if(c&&f.set("alr",encodeURIComponent(Qq("x")))"#).unwrap();
        assert_eq!(info.name, "Qq");
        assert_eq!(info.constant_arg, None);
        // pattern 5: c && d.set("alr", encodeURIComponent(ZZ(
        let info =
            extract_sig_function_info(r#"c&&d.set("alr",encodeURIComponent(Z9("y")))"#).unwrap();
        assert_eq!(info.name, "Z9");
    }

    #[test]
    fn sig_extraction_fails_on_noise() {
        assert!(extract_sig_function_info("var nothing = 1;").is_none());
    }

    #[test]
    fn extracts_the_n_function_both_shapes() {
        // pattern 1
        let info = extract_n_function_info(r#"xx.get("n"))&&(b=w8func(Y))"#).unwrap();
        assert_eq!(info.name, "w8func");
        assert_eq!(info.array_index, None);
        // pattern 1 with an array index
        let info = extract_n_function_info(r#"xx.get("n"))&&(b=arr[3](Y))"#).unwrap();
        assert_eq!(info.name, "arr");
        assert_eq!(info.array_index, Some(3));
        // pattern 2 — the backreference port: the assignee must be the
        // call argument
        let info = extract_n_function_info(r#"xx.get("n")) && (Q = w9(Q))"#).unwrap();
        assert_eq!(info.name, "w9");
        // ...and a non-matching argument rejects it
        assert!(extract_n_function_info(r#"xx.get("n")) && (Q = w9(Z))"#).is_none());
        // pattern 4
        let info =
            extract_n_function_info("zz=function(a){var b=enhanced_except_1;return a;};").unwrap();
        assert_eq!(info.name, "zz");
    }

    #[test]
    fn reads_the_signature_timestamp() {
        assert_eq!(
            signature_timestamp("var a=1;var b={signatureTimestamp:20774,c:2};"),
            Some(20774)
        );
        assert_eq!(
            signature_timestamp("signatureTimestamp=19988;"),
            Some(19988)
        );
        assert_eq!(signature_timestamp("nothing"), None);
    }

    #[test]
    fn parses_the_signature_cipher_query() {
        let params =
            parse_query_params("s=abc%2Bdef&sp=sig&url=https%3A%2F%2Fexample.com%2Fv%3Fa%3D1");
        assert_eq!(params.get("s").map(String::as_str), Some("abc+def"));
        assert_eq!(params.get("sp").map(String::as_str), Some("sig"));
        assert_eq!(
            params.get("url").map(String::as_str),
            Some("https://example.com/v?a=1")
        );
    }

    #[tokio::test]
    async fn runtime_splices_exports_into_the_player_iife() {
        let engine = Arc::new(Engine::new().unwrap());
        let runtime = CipherRuntime::create(engine, MINI_PLAYER).await.unwrap();
        // the pattern-1 trigger names Qq with constant arg 24: the sig
        // function reverses its second argument
        let info = extract_sig_function_info(MINI_PLAYER).unwrap();
        assert_eq!(info.name, "Qq");
        assert_eq!(info.constant_arg, Some(24));
        assert_eq!(
            runtime.deobfuscate_signature("abc123").await.unwrap(),
            "321cba"
        );
        // no n-pattern matches the mini player: the brute force finds
        // window.W7 by its _w8_-carrying answer
        assert!(runtime.n_function_available());
        let transformed = runtime.transform_n("bbbb").await.unwrap();
        assert!(transformed.starts_with("_w8_"), "{transformed}");
    }

    #[tokio::test]
    async fn runtime_without_the_iife_tail_still_evaluates() {
        let engine = Arc::new(Engine::new().unwrap());
        // no })(_yt_player); tail: no exports, but evaluation succeeds
        let runtime = CipherRuntime::create(engine, "var plain = 1;")
            .await
            .unwrap();
        assert!(!runtime.n_function_available());
        assert!(runtime.deobfuscate_signature("x").await.is_err());
    }
}
