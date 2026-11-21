//! The wry login window — tier 1 + tier 2 of the R3 mitigation,
//! behind the `login-window` feature (the webview backends need
//! system libraries — webkit2gtk on Linux — that the CI matrix does
//! not install; the pure core in this module's siblings compiles and
//! tests everywhere).
//!
//! Port of `YouTubeLoginWebContent`
//! (ui/screens/settings/importsync/YouTubeLoginScreen.kt:141-280):
//!
//! - a desktop-Chrome User-Agent — upstream strips `"; wv"` from the
//!   Android WebView UA because "google refuses sign-in from pages
//!   that announce themselves as an embedded webview"
//!   (YouTubeLoginScreen.kt:238-239); on desktop the equivalent is a
//!   full Chrome UA (blueprint R3: the pattern proven by Electron YTM
//!   apps), per-OS so the OS token matches the machine;
//! - on every page finish whose URL starts with `https://music.youtube.com`,
//!   try to finish: harvest the jar for music.youtube.com (HttpOnly
//!   included — wry `cookies_for_url`), require SAPISID, evaluate
//!   [`READ_CONFIG_JS`] for the session ids — upstream `finishLogin`
//!   (YouTubeLoginScreen.kt:160-205). No SAPISID yet -> keep waiting
//!   (the upstream early return; the next page finish retries);
//! - tier 2 (the loopback-redirect fallback, blueprint R3): the window
//!   loads the login URL with `continue` pointed at a one-shot
//!   loopback listener; when that page finishes, the window loads
//!   `https://music.youtube.com/` so the YouTube bootstrap materializes
//!   the `.youtube.com` cookies, and the tier-1 harvest takes over;
//! - the async completion (vault + settings + accountInfo) is the
//!   CALLER's job: the window only harvests and returns
//!   [`WindowOutcome::Harvested`] credentials, never touching the
//!   vault or the account endpoint itself.
//!
//! The GPUI shell will host this as a child window ("webview children"
//! in the mochy bin's charter); until then it owns a plain tao window.
//! Run it on a thread that may block (the event loop is the window
//! thread) — `mochy_ui_settings::login::login_with_window` wraps it
//! for async callers.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use super::cookies::{HARVEST_URL, JarCookieRecord, cookie_header_for, has_sapisid};
use super::loopback::LoopbackFinishListener;
use super::ytcfg::{READ_CONFIG_JS, parse_ytcfg_result};

/// The login URL — upstream `LOGIN_URL`
/// (YouTubeLoginScreen.kt:66): the ServiceLogin entry with
/// music.youtube.com as the continue target.
pub const LOGIN_URL: &str =
    "https://accounts.google.com/ServiceLogin?continue=https%3A%2F%2Fmusic.youtube.com";

/// The music.youtube.com landing the harvest waits for
/// (YouTubeLoginScreen.kt:260: `url.startsWith("https://music.youtube.com")`).
const MUSIC_ORIGIN: &str = "https://music.youtube.com";
/// Tier-2 landings arrive on the loopback.
const LOOPBACK_PREFIX: &str = "http://127.0.0.1:";

/// The desktop-Chrome UA per OS (R3 mitigation; a modern stable
/// Chrome token — the UA must not announce "embedded").
pub fn desktop_chrome_user_agent() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36"
    }
    #[cfg(target_os = "macos")]
    {
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36"
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36"
    }
}

/// How the window reports what happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowOutcome {
    /// Sign-in finished and the credentials were harvested (the
    /// caller runs [`super::account::complete_login`]).
    Harvested(super::account::LoginCredentials),
    /// The user closed the window without a complete sign-in.
    Cancelled,
}

/// Tier selection for one window run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LoginWindowOptions {
    /// Start on tier 2 (the loopback `continue` target) instead of the
    /// upstream music.youtube.com target. The auto-switch policy
    /// ("sign-in stuck? use the fallback") is the caller's UI decision
    /// — one tier per window run keeps the loop simple.
    pub use_loopback: bool,
}

/// Percent-encode a continue target into the ServiceLogin URL
/// (upstream encodes music.youtube.com once; tier 2 swaps the target).
fn login_url_with_continue(continue_url: &str) -> String {
    let encoded: String = continue_url
        .bytes()
        .flat_map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                vec![byte as char]
            }
            _ => format!("%{byte:02X}")
                .into_bytes()
                .into_iter()
                .map(|b| b as char)
                .collect(),
        })
        .collect();
    format!("https://accounts.google.com/ServiceLogin?continue={encoded}")
}

/// Map a wry cookie into the pure jar record.
fn jar_record(cookie: &cookie::Cookie<'_>) -> JarCookieRecord {
    JarCookieRecord {
        name: cookie.name().to_owned(),
        value: cookie.value().to_owned(),
        domain: cookie.domain().unwrap_or_default().to_owned(),
        path: cookie.path().unwrap_or("/").to_owned(),
        expires_at: cookie
            .expires()
            .and_then(|expiration| expiration.datetime())
            .map(|time| time.unix_timestamp()),
        http_only: cookie.http_only().unwrap_or(false),
        secure: cookie.secure().unwrap_or(false),
    }
}

#[derive(Debug)]
enum WindowEvent {
    /// A music.youtube.com page finished: try the harvest.
    TryHarvest,
    /// The loopback finish page was reached: materialize the YouTube
    /// session cookies (tier 2).
    LoopbackHit,
    /// The harvest completed: close.
    Close,
}

/// Run the interactive login window (tier 1 or tier 2 per
/// `options`). Blocks on the tao event loop until the user finishes
/// signing in or closes the window.
pub fn run_login_window(options: LoginWindowOptions) -> Result<WindowOutcome, String> {
    // tier 2 binds the loopback and points `continue` at it
    let loopback = if options.use_loopback {
        Some(LoopbackFinishListener::bind().map_err(|error| error.to_string())?)
    } else {
        None
    };
    let start_url = match &loopback {
        Some(listener) => login_url_with_continue(&listener.finish_url()),
        None => LOGIN_URL.to_owned(),
    };

    let event_loop = tao::event_loop::EventLoopBuilder::<WindowEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    let (tx, rx) = mpsc::channel();

    let window = tao::window::WindowBuilder::new()
        .with_title("Sign in to YouTube")
        .with_inner_size(tao::dpi::LogicalSize::new(1024.0, 768.0))
        .build(&event_loop)
        .map_err(|error| error.to_string())?;

    let builder = wry::WebViewBuilder::new()
        .with_url(&start_url)
        .with_user_agent(desktop_chrome_user_agent())
        .with_on_page_load_handler({
            let proxy = proxy.clone();
            move |event, url| {
                if !matches!(event, wry::PageLoadEvent::Finished) {
                    return;
                }
                if url.starts_with(MUSIC_ORIGIN) {
                    let _ = proxy.send_event(WindowEvent::TryHarvest);
                } else if url.starts_with(LOOPBACK_PREFIX) {
                    let _ = proxy.send_event(WindowEvent::LoopbackHit);
                }
            }
        });

    #[cfg(any(
        target_os = "windows",
        target_os = "macos",
        target_os = "ios",
        target_os = "android"
    ))]
    let webview = builder.build(&window).map_err(|error| error.to_string())?;
    #[cfg(not(any(
        target_os = "windows",
        target_os = "macos",
        target_os = "ios",
        target_os = "android"
    )))]
    let webview = {
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;
        let vbox = window.default_vbox().unwrap();
        builder.build_gtk(vbox)
    }
    .map_err(|error| error.to_string())?;

    let webview = Arc::new(webview);
    let harvested = Arc::new(AtomicBool::new(false));
    let mut cancelled = false;

    // `run_return` (not `run`): the login window is an embedded child
    // flow — control must come back to the caller when the loop exits
    // (tao's `run` never returns). Available on Windows/Unix/macOS via
    // `EventLoopExtRunReturn`; iOS is not a port target. The handler
    // needs no `'static` bound, so it captures by reference — the
    // 'static inner callbacks (evaluate_script_with_callback) receive
    // owned clones.
    use tao::platform::run_return::EventLoopExtRunReturn;
    let mut event_loop = event_loop;
    event_loop.run_return(|event, _, control_flow| {
        *control_flow = tao::event_loop::ControlFlow::Wait;
        match event {
            tao::event::Event::UserEvent(WindowEvent::TryHarvest) => {
                if harvested.load(Ordering::SeqCst) {
                    return;
                }
                // finishLogin (YouTubeLoginScreen.kt:160-205): the jar
                // first, the SAPISID guard, then the ytcfg scrape
                let header = match webview.cookies_for_url(HARVEST_URL) {
                    Ok(jar) => {
                        let records: Vec<JarCookieRecord> = jar.iter().map(jar_record).collect();
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|since| since.as_secs() as i64)
                            .unwrap_or(0);
                        cookie_header_for(&records, now)
                    }
                    Err(error) => {
                        tracing::warn!(%error, "cookie jar read failed");
                        return;
                    }
                };
                if !has_sapisid(&header) {
                    // sign-in not complete; the next page finish retries
                    return;
                }
                let proxy = proxy.clone();
                let webview = Arc::clone(&webview);
                let harvested = Arc::clone(&harvested);
                let tx = tx.clone();
                let _ = webview.evaluate_script_with_callback(READ_CONFIG_JS, move |raw| {
                    // fires later on this same event loop thread
                    let ids = parse_ytcfg_result(&raw);
                    let credentials = super::account::LoginCredentials {
                        cookie: header.clone(),
                        visitor_data: ids.visitor_data,
                        data_sync_id: ids.data_sync_id,
                    };
                    harvested.store(true, Ordering::SeqCst);
                    let _ = tx.send(credentials);
                    let _ = proxy.send_event(WindowEvent::Close);
                });
            }
            tao::event::Event::UserEvent(WindowEvent::LoopbackHit) => {
                // the sign-in completed against the loopback; load the
                // music origin so the YouTube bootstrap sets the
                // .youtube.com cookies, then the tier-1 harvest fires
                if let Err(error) = webview.load_url(MUSIC_ORIGIN) {
                    tracing::error!(%error, "could not load music.youtube.com after the loopback hit");
                }
            }
            tao::event::Event::UserEvent(WindowEvent::Close) => {
                *control_flow = tao::event_loop::ControlFlow::Exit;
            }
            tao::event::Event::WindowEvent {
                event: tao::event::WindowEvent::CloseRequested,
                ..
            } => {
                cancelled = true;
                *control_flow = tao::event_loop::ControlFlow::Exit;
            }
            _ => {}
        }
    });

    drop(loopback);

    // the callback sent the credentials before the Close event
    match rx.recv_timeout(Duration::from_secs(1)) {
        Ok(credentials) => Ok(WindowOutcome::Harvested(credentials)),
        Err(_) if cancelled => Ok(WindowOutcome::Cancelled),
        Err(_) => Ok(WindowOutcome::Cancelled),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tier-2 URL builder percent-encodes the continue target the
    /// way the upstream constant encodes music.youtube.com.
    #[test]
    fn loopback_continue_url_is_encoded() {
        let url = login_url_with_continue("http://127.0.0.1:41234/finish");
        assert_eq!(
            url,
            "https://accounts.google.com/ServiceLogin?continue=http%3A%2F%2F127.0.0.1%3A41234%2Ffinish"
        );
        // the upstream constant stays the tier-1 default
        assert_eq!(
            LOGIN_URL,
            "https://accounts.google.com/ServiceLogin?continue=https%3A%2F%2Fmusic.youtube.com"
        );
    }

    /// The UA is a full desktop Chrome per OS (never an embedded one).
    #[test]
    fn user_agent_announces_desktop_chrome() {
        let ua = desktop_chrome_user_agent();
        assert!(ua.contains("Chrome/131"), "a stable Chrome token: {ua}");
        assert!(!ua.contains("wv"), "no embedded-webview marker");
        assert!(!ua.to_lowercase().contains("webview"));
    }
}
