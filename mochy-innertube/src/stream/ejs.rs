//! The EJS n-solver — the port of `utils/sabr/EjsNTransformSolver.kt`.
//!
//! Upstream loaded three JS files (`meriyah.js`, `astring.js`,
//! `yt.solver.core.js`) into an Android WebView, ran the bundle's `jsc`
//! entry over the current player script, and kept the `n` solver the
//! preprocessed player exports. The desktop port runs the same three
//! files — vendored under `assets/solver/`, see that README for
//! provenance and licenses — inside the shared QuickJS [`Engine`].
//!
//! The upstream flow (EjsNTransformSolver.kt:128-215, the HTML's
//! `initSolver`/`transformN`):
//!
//! ```js
//! var result = jsc({ type: 'player', player: playerCode,
//!                    requests: [], output_preprocessed: true });
//! if (result.type === 'error') { /* solver unavailable */ }
//! var resultObj = { n: null, sig: null };
//! (new Function('_result', result.preprocessed_player))(resultObj);
//! _nSolver = resultObj.n;   // the n-transform function
//! ```
//!
//! The bundle's own `getFromPrepared` runs the whole preprocessed
//! player — with the browser-environment stubs the bundle prepends
//! (`setupNodes`: XMLHttpRequest/location/document/navigator/self/
//! window) — so it executes safely in a bare QuickJS.
//!
//! Errors never propagate to the caller as hard failures
//! (EjsNTransformSolver.kt:55-58 returns the original URL when the
//! transform throws); [`EjsSolver::transform_n_param_in_url`] keeps
//! that contract.

use std::sync::Arc;

use super::jsengine::{Engine, JsError};

/// The vendored bundle files (`app/src/main/assets/solver/` upstream).
const MERIYAH_JS: &str = include_str!("../../assets/solver/meriyah.js");
const ASTRING_JS: &str = include_str!("../../assets/solver/astring.js");
const SOLVER_CORE_JS: &str = include_str!("../../assets/solver/yt.solver.core.js");

/// A compiled EJS solver over one player script.
///
/// The QuickJS state (globals `__ejsN` / `__ejsSig`) lives in the
/// shared engine; this struct is just the compiled marker plus a
/// handle to it, mirroring upstream's single cached `solverWebView`.
#[derive(Debug, Clone)]
pub struct EjsSolver {
    engine: Arc<Engine>,
    n_available: bool,
    sig_available: bool,
}

/// Boot the shared engine's EJS environment: the two dependency UMDs
/// and the bundle itself. Idempotent — the globals are only defined
/// once per engine.
///
/// The current esbuild build of the bundle resolves its two external
/// dependencies through a `__require` shim; QuickJS has no `require`,
/// so a locked-down globals-only stand-in is installed first (see
/// assets/solver/README.md).
async fn boot_bundle(engine: &Arc<Engine>) -> Result<(), JsError> {
    let booted: bool = engine.eval("typeof globalThis.jsc !== 'undefined'").await?;
    if booted {
        return Ok(());
    }
    engine
        .eval::<()>(
            "globalThis.require = function (name) { \
                 if (typeof globalThis[name] !== 'undefined') return globalThis[name]; \
                 throw new Error('require is unavailable in the mochy JS engine: ' + name); \
             };",
        )
        .await?;
    engine.eval::<()>(MERIYAH_JS).await?;
    engine.eval::<()>(ASTRING_JS).await?;
    engine.eval::<()>(SOLVER_CORE_JS).await?;
    // the esbuild IIFE exposes either a function (old builds) or a
    // module namespace object whose default is the entry
    engine
        .eval::<()>("globalThis.__ejsMain = typeof jsc === 'function' ? jsc : jsc.default;")
        .await?;
    Ok(())
}

impl EjsSolver {
    /// Compile the solver for `player_js`
    /// (EjsNTransformSolver.kt:62-85 `getOrCreateSolver` +
    /// the HTML's `initSolver`).
    ///
    /// Returns the solver even when the bundle could not extract the
    /// functions (upstream keeps a dead webview and answers "not
    /// available" on every later call); check [`n_available`].
    pub async fn compile(engine: Arc<Engine>, player_js: &str) -> Result<Self, JsError> {
        boot_bundle(&engine).await?;
        let player_literal = serde_json::to_string(player_js).unwrap_or_default();
        let code = format!(
            "(function () {{
                 globalThis.__ejsN = null; globalThis.__ejsSig = null; globalThis.__ejsError = null;
                 try {{
                     var result = globalThis.__ejsMain({{
                         type: 'player',
                         player: {player_literal},
                         requests: [],
                         output_preprocessed: true,
                     }});
                     if (result.type === 'error') {{
                         globalThis.__ejsError = String(result.error);
                         return false;
                     }}
                 }} catch (e) {{
                     // the bundle throws ('unexpected structure', parser
                     // errors, ...) instead of returning type 'error'
                     globalThis.__ejsError = String(e);
                     return false;
                 }}
                 var resultObj = {{ n: null, sig: null }};
                 try {{
                     (new Function('_result', result.preprocessed_player))(resultObj);
                 }} catch (e) {{
                     globalThis.__ejsError = 'preprocessed player failed: ' + e;
                     return false;
                 }}
                 globalThis.__ejsN = (typeof resultObj.n === 'function') ? resultObj.n : null;
                 globalThis.__ejsSig = (typeof resultObj.sig === 'function') ? resultObj.sig : null;
                 return true;
             }})()"
        );
        let compiled: bool = engine.eval(code).await?;
        let availability: String = engine
            .eval(
                "[typeof globalThis.__ejsN === 'function', typeof globalThis.__ejsSig === 'function'].join(',')",
            )
            .await?;
        let mut fields = availability.split(',');
        let n_available = fields.next() == Some("true");
        let sig_available = fields.next() == Some("true");
        if !compiled {
            let error: String = engine
                .eval("globalThis.__ejsError || 'EJS preprocessing failed'")
                .await
                .unwrap_or_else(|_| "EJS preprocessing failed".into());
            tracing::warn!(target: "mochy_ejs", error, "EJS solver could not extract the n function");
        }
        Ok(EjsSolver {
            engine,
            n_available,
            sig_available,
        })
    }

    /// Whether the n-transform function was extracted
    /// (`SolverWebView.nFunctionAvailable`).
    pub fn n_available(&self) -> bool {
        self.n_available
    }

    /// Whether the signature function was extracted. Upstream's
    /// EjsNTransformSolver only ever wires `n` (the signature path is
    /// `cipher/`); the bundle hands both over, so it is kept available
    /// for the NewPipe-equivalent fallback.
    pub fn sig_available(&self) -> bool {
        self.sig_available
    }

    /// Run the n-transform on one value (the HTML's `transformN` +
    /// `onNResult`). Errors surface as `Err` for the caller's
    /// fall-back-to-original-url handling.
    pub async fn transform_n(&self, n_value: &str) -> Result<String, JsError> {
        if !self.n_available {
            return Err(JsError::Script("EJS n solver not available".into()));
        }
        let literal = serde_json::to_string(n_value).unwrap_or_default();
        let code = format!(
            "(function () {{
                 var out = globalThis.__ejsN({literal});
                 return (typeof out === 'string') ? out : String(out);
             }})()"
        );
        self.engine.eval(code).await
    }

    /// Run the signature deobfuscation on one value (the bundle's
    /// `sig` solver; unused by upstream's walk, available to the
    /// NewPipe-equivalent path).
    pub async fn transform_sig(&self, sig_value: &str) -> Result<String, JsError> {
        if !self.sig_available {
            return Err(JsError::Script("EJS sig solver not available".into()));
        }
        let literal = serde_json::to_string(sig_value).unwrap_or_default();
        let code = format!(
            "(function () {{
                 var out = globalThis.__ejsSig({literal});
                 return (typeof out === 'string') ? out : String(out);
             }})()"
        );
        self.engine.eval(code).await
    }

    /// `transformNParamInUrl` (EjsNTransformSolver.kt:28-60): find the
    /// `n` parameter, solve it, and put the transformed value back —
    /// URL-decoded on the way in, URL-encoded on the way out, exactly
    /// like the upstream `Uri.decode`/`Uri.encode` pair. A URL with no
    /// `n`, or a solver that is unavailable or throws, comes back
    /// unchanged.
    pub async fn transform_n_param_in_url(&self, url: &str) -> String {
        let Some((n_value, range)) = find_n_param(url) else {
            tracing::debug!(target: "mochy_ejs", "no 'n' parameter in url");
            return url.to_owned();
        };
        let decoded = super::urlutil::uri_decode(&n_value);
        match self.transform_n(&decoded).await {
            Ok(transformed) => {
                let encoded = super::urlutil::uri_encode(&transformed);
                let mut out = url[..range.start].to_owned();
                out.push_str(&encoded);
                out.push_str(&url[range.end..]);
                out
            }
            Err(error) => {
                tracing::error!(target: "mochy_ejs", error = %error, "SABR n-transform failed");
                url.to_owned()
            }
        }
    }
}

/// Find the first `n=` query parameter: `[?&]n=([^&]+)`
/// (EjsNTransformSolver.kt:29). Returns the raw (still percent-coded)
/// value and its byte range inside `url`.
pub(crate) fn find_n_param(url: &str) -> Option<(String, std::ops::Range<usize>)> {
    let bytes = url.as_bytes();
    let mut index = 0;
    while let Some(position) = url[index..].find(['?', '&']) {
        let start = index + position + 1;
        if url[start..].starts_with("n=") {
            let value_start = start + 2;
            let value_end = url[value_start..]
                .find('&')
                .map(|offset| value_start + offset)
                .unwrap_or(url.len());
            return Some((
                url[value_start..value_end].to_owned(),
                value_start..value_end,
            ));
        }
        index = start;
        let _ = bytes;
    }
    None
}

/// One recorded challenge from the fixture corpus
/// (`tests/fixtures/ejs/challenges.json`).
#[cfg(test)]
#[derive(Debug, serde::Deserialize)]
pub(crate) struct RecordedChallenge {
    pub input: String,
    pub expected: String,
}

/// The fixture corpus shape (see `tests/fixtures/ejs/challenges.json`).
#[cfg(test)]
#[derive(Debug, serde::Deserialize)]
pub(crate) struct ChallengeFixture {
    /// Provenance record (player hash, URL, trimming oracle) — kept
    /// for the fixture-shape test, which asserts it is present.
    #[serde(rename = "_provenance")]
    #[allow(dead_code)]
    pub provenance: serde_json::Value,
    pub n: Vec<RecordedChallenge>,
    pub sig: Vec<RecordedChallenge>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_n_param_like_the_kotlin_regex() {
        let url = "https://x/v?a=1&n=abc_d-e&b=2";
        let (value, range) = find_n_param(url).expect("n present");
        assert_eq!(value, "abc_d-e");
        assert_eq!(&url[range], "abc_d-e");
        // the first query param, not a later one
        let (value, _) = find_n_param("https://x/video?n=first&nn=no").unwrap();
        assert_eq!(value, "first");
        // none
        assert!(find_n_param("https://x/video?id=a").is_none());
        assert!(find_n_param("https://x/video?nn=not_the_n").is_none());
    }

    #[tokio::test]
    async fn solves_the_recorded_n_challenge_offline() {
        let engine = Arc::new(Engine::new().unwrap());
        let player = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/ejs/player_631d3938_trimmed.js"
        ))
        .expect("recorded player fixture");
        let challenges: ChallengeFixture = serde_json::from_str(
            &std::fs::read_to_string(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/ejs/challenges.json"
            ))
            .expect("challenges fixture"),
        )
        .expect("challenges parse");
        let solver = EjsSolver::compile(engine, &player).await.unwrap();
        assert!(solver.n_available(), "the n solver must be extracted");
        assert!(solver.sig_available(), "the sig solver must be extracted");
        for challenge in &challenges.n {
            let solved = solver.transform_n(&challenge.input).await.unwrap();
            assert_eq!(solved, challenge.expected, "recorded n vector");
        }
        for challenge in &challenges.sig {
            let solved = solver.transform_sig(&challenge.input).await.unwrap();
            assert_eq!(solved, challenge.expected, "recorded sig vector");
        }
    }

    #[tokio::test]
    async fn unavailable_solver_leaves_the_url_untouched() {
        let engine = Arc::new(Engine::new().unwrap());
        // a player with no recognizable structure: preprocessing fails,
        // the solver reports unavailable and the URL passes through
        let solver = EjsSolver::compile(engine, "var nothing = 1;")
            .await
            .unwrap();
        assert!(!solver.n_available());
        let url = "https://example.com/videoplayback?n=abc&itag=140";
        assert_eq!(solver.transform_n_param_in_url(url).await, url);
    }

    #[tokio::test]
    async fn bundle_boot_is_idempotent() {
        let engine = Arc::new(Engine::new().unwrap());
        boot_bundle(&engine).await.unwrap();
        boot_bundle(&engine).await.unwrap();
        let is_function: bool = engine
            .eval("typeof globalThis.__ejsMain === 'function'")
            .await
            .unwrap();
        assert!(is_function);
        let raw: String = engine
            .eval("JSON.stringify((function () { try { return globalThis.__ejsMain({type: 'player', player: 'var x = 1;', requests: [], output_preprocessed: false}); } catch (e) { return {type: 'error', error: String(e)}; } })())")
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert!(
            json["type"] == "result" || json["type"] == "error",
            "jsc entry callable: {json}"
        );
    }

    #[test]
    fn challenge_fixture_shape() {
        // guards the fixture format the corpus is written in
        let raw = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/ejs/challenges.json"
        ))
        .unwrap();
        let value: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert!(value["_provenance"]["player_hash"].is_string());
        assert!(!value["n"].as_array().unwrap().is_empty());
        assert!(!value["sig"].as_array().unwrap().is_empty());
    }
}
