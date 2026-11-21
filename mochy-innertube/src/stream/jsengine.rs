//! The JS engine — one QuickJS-NG instance on one dedicated thread.
//!
//! Upstream ran JavaScript in **three Android WebViews**:
//! `cipher/CipherWebView.kt` (signature + n-transform over the player
//! script), `sabr/EjsNTransformSolver.kt` (the yt-dlp EJS bundle over
//! meriyah + astring) and `potoken/PoTokenWebView.kt` (the BotGuard
//! client from `assets/po_token.html`). A desktop process needs no
//! browser for any of that: the scripts are plain JS, so the port runs
//! them all in a single [`rquickjs`] (QuickJS-NG) context instead.
//!
//! QuickJS contexts are single-threaded, so the engine owns one OS
//! thread and every job is funneled to it through a channel — the same
//! one-WebView-one-main-thread discipline upstream had, without the
//! browser. Callers stay async: a job is submitted and awaited through
//! a oneshot.
//!
//! The engine installs a tiny environment shim before any upstream
//! script runs ([`SHIM`]): a `console`, `window` (an alias of
//! `globalThis`, which the BotGuard client and the player script both
//! expect) and cooperative `setTimeout`/`setInterval`/`clearTimeout`/
//! `clearInterval`/`performance.now` implementations. QuickJS has no
//! event loop of its own, so timers live in a queue that
//! [`Engine::pump`] drains between job pumps — a job that waits on a
//! promise or a timer makes progress as long as its driver keeps
//! pumping, which is exactly the WebView contract upstream relied on.

use std::time::{Duration, Instant};

use rquickjs::{Context, Runtime};
use thiserror::Error;

/// A JS-side failure surfaced through `rquickjs` (exception text or an
/// engine-level error).
#[derive(Debug, Error)]
pub enum JsError {
    #[error("JS: {0}")]
    Script(String),
    #[error("js engine channel closed")]
    Closed,
    #[error("js engine timed out after {0:?}")]
    Timeout(Duration),
}

impl From<rquickjs::Error> for JsError {
    fn from(error: rquickjs::Error) -> Self {
        JsError::Script(error.to_string())
    }
}

/// The pre-script environment shim.
///
/// - `console.log/warn/error` route to `tracing` (the WebChromeClient
///   `onConsoleMessage` port; "Uncaught" is what upstream treated as
///   fatal, so it goes to `error`).
/// - `window` is `globalThis` (the player script and the BotGuard
///   client address `window[...]`).
/// - Timers are cooperative: they enqueue into `__timers` and only fire
///   when the host pumps ([`Engine::pump`]). `performance.now` uses
///   `Date.now`, matching the WebView closely enough for BotGuard's
///   pacing.
const SHIM: &str = r#"
var __timers = [];
var __timerSeq = 0;
globalThis.window = globalThis;
globalThis.console = {
    log: function () { __console('debug', arguments); },
    debug: function () { __console('debug', arguments); },
    info: function () { __console('info', arguments); },
    warn: function () { __console('warn', arguments); },
    error: function () { __console('error', arguments); }
};
function __console(level, args) {
    var parts = [];
    for (var i = 0; i < args.length; i++) parts.push(String(args[i]));
    var line = parts.join(' ');
    if (line.indexOf('Uncaught') >= 0) __hostConsole('error', line);
    else __hostConsole(level, line);
}
globalThis.setTimeout = function (fn, ms) {
    __timerSeq += 1;
    __timers.push({ id: __timerSeq, runAt: Date.now() + (Number(ms) || 0), fn: fn, interval: 0 });
    return __timerSeq;
};
globalThis.setInterval = function (fn, ms) {
    __timerSeq += 1;
    var period = Math.max(Number(ms) || 1, 1);
    __timers.push({ id: __timerSeq, runAt: Date.now() + period, fn: fn, interval: period });
    return __timerSeq;
};
globalThis.clearTimeout = globalThis.clearInterval = function (id) {
    for (var i = 0; i < __timers.length; i++) if (__timers[i].id === id) __timers.splice(i, 1);
};
globalThis.performance = { now: function () { return Date.now(); } };
globalThis.__pumpTimers = function () {
    var now = Date.now();
    var due = [];
    for (var i = __timers.length - 1; i >= 0; i--) {
        if (__timers[i].runAt <= now) { due.push(__timers[i]); __timers.splice(i, 1); }
    }
    for (var j = due.length - 1; j >= 0; j--) {
        var timer = due[j];
        try { timer.fn(); } catch (e) { __hostConsole('error', 'timer: ' + e); }
        if (timer.interval > 0) __timers.push({ id: timer.id, runAt: Date.now() + timer.interval, fn: timer.fn, interval: timer.interval });
    }
};
"#;

/// The console bridge function injected from Rust
/// (`__hostConsole(level, line)`).
fn host_console(level: rquickjs::function::Rest<String>) {
    let mut parts = level.0.into_iter();
    let level = parts.next().unwrap_or_else(|| "debug".into());
    let line = parts.next().unwrap_or_default();
    match level.as_str() {
        "error" => tracing::error!(target: "mochy_js", "{line}"),
        "warn" => tracing::warn!(target: "mochy_js", "{line}"),
        "info" => tracing::info!(target: "mochy_js", "{line}"),
        _ => tracing::debug!(target: "mochy_js", "{line}"),
    }
}

/// The engine thread's OS stack, in bytes.
///
/// The JS engine runs player-scale JavaScript — the vendored EJS bundle
/// parses a full 2.7 MB YouTube player with meriyah (itself JavaScript),
/// the signature cipher and the BotGuard client — and QuickJS-NG
/// recurses in C for every JS-to-JS call (`JS_CallInternal`), so the
/// interpreter's C frame rides the stack once per JS call level.
///
/// Measured on Linux/GCC debug builds (2026-11-21 audit, see HANDOFF
/// "Phase 04–06 Audit and Corrective Work"): the EJS pipeline peaks at
/// ~250 KiB of C stack for the delta-trimmed challenge fixture and
/// ~500 KiB for the real full player — leaving only a ~2× margin under
/// QuickJS's own 1 MiB default budget. Debug-profile MSVC frames for
/// `JS_CallInternal` are several times larger than GCC's (every local
/// of the one-function interpreter gets its own unshared slot under
/// `/Od`), which pushed the same recursion past the 1 MiB budget on
/// Windows CI and failed the recorded-challenge test with a swallowed
/// `RangeError: Maximum call stack size exceeded`. macOS secondary
/// threads default to a 512 KiB stack — thinner than the real player's
/// measured need — so an explicit reservation is required there too.
///
/// The reservation is virtual address space, committed on touch; one
/// engine per process (plus a handful in tests) makes the cost
/// irrelevant. It must stay comfortably larger than
/// [`ENGINE_JS_STACK_BUDGET`] so QuickJS's own limit fires (a clean,
/// catchable `RangeError`) long before the real stack bottom — a JS
/// budget larger than the thread stack turns the guard into a hard
/// `SIGSEGV`/`0xC00000FD` instead (verified experimentally during the
/// audit).
const ENGINE_THREAD_STACK: usize = 32 * 1024 * 1024;

/// The QuickJS recursion budget exposed to scripts, in bytes
/// ([`Runtime::set_max_stack_size`]).
///
/// 16 MiB is ~8× the MSVC-debug worst-case estimate for the real
/// player pipeline (~2 MiB) and ~64× the trimmed-fixture need measured
/// on Linux; release-profile frames are smaller still. QuickJS's
/// default (1 MiB) was tuned for embedding qjs-sized snippets, not for
/// a JavaScript-implemented parser eating hundreds of interpreter
/// frames per nest level — upstream's Android WebViews ran the same
/// bundle on V8, whose frames are compact enough that its ~1 MiB
/// default sufficed; the port compensates for QuickJS's fatter frames.
const ENGINE_JS_STACK_BUDGET: usize = 16 * 1024 * 1024;

/// One unit of JS work for the engine thread.
type Job = Box<dyn FnOnce(&Runtime, &Context) + Send + 'static>;

/// The engine handle — the `StreamEngine` keeps it in an `Arc`.
pub struct Engine {
    tx: std::sync::mpsc::Sender<Job>,
}

impl Engine {
    /// Boot the engine thread and its QuickJS context with the shim.
    pub fn new() -> Result<Self, JsError> {
        let (tx, rx) = std::sync::mpsc::channel::<Job>();
        let (boot_tx, boot_rx) = std::sync::mpsc::channel::<Result<(), JsError>>();
        std::thread::Builder::new()
            .name("mochy-js-engine".to_owned())
            .stack_size(ENGINE_THREAD_STACK)
            .spawn(move || {
                let runtime = match Runtime::new() {
                    Ok(runtime) => runtime,
                    Err(error) => {
                        let _ = boot_tx.send(Err(error.into()));
                        return;
                    }
                };
                // Raise QuickJS's internal recursion budget before any
                // script runs (the default 1 MiB starves meriyah on
                // debug-profile frames; see ENGINE_JS_STACK_BUDGET).
                runtime.set_max_stack_size(ENGINE_JS_STACK_BUDGET);
                let context = match Context::full(&runtime) {
                    Ok(context) => context,
                    Err(error) => {
                        let _ = boot_tx.send(Err(error.into()));
                        return;
                    }
                };
                let readied = context.with(|ctx| -> Result<(), JsError> {
                    let console = rquickjs::Function::new(ctx.clone(), host_console)?;
                    ctx.globals().set("__hostConsole", console)?;
                    ctx.eval::<(), _>(SHIM)?;
                    Ok(())
                });
                let _ = boot_tx.send(readied);
                drop(boot_tx);
                while let Ok(job) = rx.recv() {
                    job(&runtime, &context);
                }
            })
            .map_err(|error| JsError::Script(format!("engine thread: {error}")))?;
        match boot_rx.recv_timeout(Duration::from_secs(30)) {
            Ok(Ok(())) => Ok(Engine { tx }),
            Ok(Err(error)) => Err(error),
            Err(_) => Err(JsError::Timeout(Duration::from_secs(30))),
        }
    }

    /// Submit `work` to the engine thread and await its result. The
    /// closure receives the runtime (for job draining inside
    /// [`Engine::wait_for`]) and the live context.
    pub async fn drive<T, F>(&self, work: F) -> Result<T, JsError>
    where
        T: Send + 'static,
        F: FnOnce(&Runtime, &Context) -> Result<T, JsError> + Send + 'static,
    {
        let (done_tx, done_rx) = tokio::sync::oneshot::channel();
        let job: Job = Box::new(move |runtime: &Runtime, context: &Context| {
            let _ = done_tx.send(work(runtime, context));
        });
        self.tx.send(job).map_err(|_| JsError::Closed)?;
        done_rx.await.map_err(|_| JsError::Closed)?
    }

    /// Evaluate `code` in the engine context (state persists, like one
    /// WebView).
    pub async fn eval<T>(&self, code: impl Into<String> + Send + 'static) -> Result<T, JsError>
    where
        T: for<'js> rquickjs::FromJs<'js> + Send + 'static,
    {
        self.eval_with_strictness(code, true).await
    }

    /// Evaluate `code` as a **classic sloppy script** — the mode the
    /// Android WebViews ran upstream's player scripts in. YouTube's
    /// base.js opens with `(function(g){var window=this;...})`, whose
    /// `this` is `undefined` under strict mode (rquickjs defaults
    /// `eval` to strict), so everything that executes a player script
    /// directly must go through here.
    pub async fn eval_sloppy<T>(
        &self,
        code: impl Into<String> + Send + 'static,
    ) -> Result<T, JsError>
    where
        T: for<'js> rquickjs::FromJs<'js> + Send + 'static,
    {
        self.eval_with_strictness(code, false).await
    }

    async fn eval_with_strictness<T>(
        &self,
        code: impl Into<String> + Send + 'static,
        strict: bool,
    ) -> Result<T, JsError>
    where
        T: for<'js> rquickjs::FromJs<'js> + Send + 'static,
    {
        let code = code.into();
        self.drive(move |_, context| {
            context.with(|ctx| {
                use rquickjs::CatchResultExt;
                let mut options = rquickjs::context::EvalOptions::default();
                options.strict = strict;
                ctx.eval_with_options::<T, _>(code.as_bytes(), options)
                    .catch(&ctx)
                    .map_err(|error| JsError::Script(error.to_string()))
            })
        })
        .await
    }

    /// Pump the context's job queue (promise reactions) and timers
    /// once. Returns whether anything ran.
    #[allow(dead_code)] // wired up by the BotGuard engine (stream/potoken.rs)
    pub(crate) fn pump(context: &Context, runtime: &Runtime) -> Result<bool, JsError> {
        let mut progressed = false;
        loop {
            match runtime.execute_pending_job() {
                Ok(true) => progressed = true,
                Ok(false) => break,
                Err(error) => return Err(JsError::Script(error.to_string())),
            }
        }
        let pumped: bool = context.with(|ctx| ctx.eval("__pumpTimers(), true"))?;
        Ok(progressed || pumped)
    }

    /// Wait until `ready` (polled on the engine thread between pumps)
    /// returns `Some`, the deadline passes, or JS throws. This is the
    /// port of the WebView "continuation resumes from a
    /// JavascriptInterface callback" pattern (e.g.
    /// `EjsBridge.onNResult`).
    #[allow(dead_code)] // wired up by the BotGuard engine (stream/potoken.rs)
    pub(crate) fn wait_for<T, F>(
        runtime: &Runtime,
        context: &Context,
        ready: F,
        timeout: Duration,
    ) -> Result<T, JsError>
    where
        F: Fn(&rquickjs::Ctx<'_>) -> Result<Option<T>, JsError>,
    {
        let deadline = Instant::now() + timeout;
        loop {
            let done = context.with(|ctx| ready(&ctx))?;
            if let Some(value) = done {
                return Ok(value);
            }
            if Instant::now() >= deadline {
                return Err(JsError::Timeout(timeout));
            }
            Self::pump(context, runtime)?;
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("JsEngine")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn evaluates_and_keeps_globals() {
        let engine = Engine::new().unwrap();
        let value: i32 = engine.eval("var __t = 20; __t + 22").await.unwrap();
        assert_eq!(value, 42);
        // state persists across jobs (one context, like one WebView)
        let again: i32 = engine.eval("__t").await.unwrap();
        assert_eq!(again, 20);
    }

    /// Regression (2026-11-21 audit): the engine must budget JS
    /// recursion for player-scale scripts. QuickJS-NG consumes one C
    /// `JS_CallInternal` frame per JS call, and the EJS bundle's
    /// meriyah parse needs ~80 (trimmed fixture) to ~160 (real player)
    /// such frames; debug-profile frames are 3+ KiB each on GCC and
    /// several times that on MSVC. Depth 512 exceeds what QuickJS's
    /// old 1 MiB default budget allowed even on Linux (~326 frames
    /// measured) — the exact condition that failed the recorded
    /// n-challenge test on Windows CI — and must now succeed
    /// everywhere (512 × 4 KiB = 2 MiB ≪ [`ENGINE_JS_STACK_BUDGET`]).
    #[tokio::test]
    async fn js_recursion_budget_covers_player_parser_depths() {
        let engine = Engine::new().unwrap();
        let depth: i32 = engine
            .eval(
                "(function () { \
                     function f(n) { return n <= 0 ? 0 : f(n - 1) + 1; } \
                     return f(512); \
                 })()",
            )
            .await
            .expect("512-deep JS recursion must fit the engine budget");
        assert_eq!(depth, 512);
        // and the budget stays bounded: absurd recursion still fails
        // cleanly instead of exhausting the thread
        let overflowed: Result<i32, _> = engine
            .eval(
                "(function () { \
                     function f(n) { return n <= 0 ? 0 : f(n - 1) + 1; } \
                     return f(1000000); \
                 })()",
            )
            .await;
        assert!(
            overflowed.is_err(),
            "a million frames must still hit the budget ceiling"
        );
    }

    #[tokio::test]
    async fn exceptions_carry_the_message() {
        let engine = Engine::new().unwrap();
        let error: Result<i32, _> = engine.eval("throw new Error('boom')").await;
        assert!(error.unwrap_err().to_string().contains("boom"));
    }

    #[tokio::test]
    async fn window_aliases_global_this() {
        let engine = Engine::new().unwrap();
        let same: bool = engine
            .eval("window.__wv = 7, window.__wv === globalThis.__wv")
            .await
            .unwrap();
        assert!(same);
    }

    #[tokio::test]
    async fn promises_and_timers_drive_to_completion() {
        let engine = Engine::new().unwrap();
        engine
            .eval::<()>(
                "globalThis.__state = 'pending'; \
                 setTimeout(function () { \
                     Promise.resolve(1).then(function (v) { \
                         globalThis.__state = 'done' + v; \
                     }); \
                 }, 5);",
            )
            .await
            .unwrap();
        let state: String = engine
            .drive(|runtime, ctx| {
                Engine::wait_for(
                    runtime,
                    ctx,
                    |ctx| {
                        ctx.eval::<Option<String>, _>(
                            "globalThis.__state === 'done1' ? globalThis.__state : null",
                        )
                        .map_err(|error| JsError::Script(error.to_string()))
                    },
                    Duration::from_secs(5),
                )
            })
            .await
            .unwrap();
        assert_eq!(state, "done1");
    }
}
