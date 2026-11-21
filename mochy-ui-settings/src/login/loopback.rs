//! The loopback-redirect fallback — tier 2 of the R3 mitigation
//! (blueprint ch. 14 R3: "Desktop-Chrome UA in wry (pattern proven by
//! Electron YTM apps); loopback-redirect fallback; manual cookie paste
//! as last resort").
//!
//! When the sign-in flow will not land on music.youtube.com cleanly
//! (consent interstitials, country redirects, the primary detection
//! never firing), the login window reloads accounts.google.com with
//! `continue` pointed at a one-shot listener on `127.0.0.1:<ephemeral>`
//! inside the SAME webview. The redirect to the loopback is a
//! deterministic "the user finished with Google" signal; the listener
//! answers with a small "return to Mochy" page, and the window then
//! materializes the YouTube session by loading
//! `https://music.youtube.com/` (the `.google.com` session cookies
//! live in the webview jar; the music.youtube.com bootstrap sets the
//! `.youtube.com` cookies the harvest needs).
//!
//! Pure std: a `TcpListener` on a dedicated thread, one page served
//! per connection until [`LoopbackFinishListener::close`], a channel
//! that fires the first time the finish URL is requested.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::thread::JoinHandle;
use std::time::Duration;

/// The page the listener serves (kept intentionally tiny; the user
/// sees it for a moment before the window takes over again).
const FINISH_PAGE: &str = "<!doctype html><html><head><meta charset=\"utf-8\"><title>Mochy</title></head>\
<body style=\"background:#131313;color:#fff;font-family:sans-serif;display:flex;align-items:center;\
justify-content:center;height:100vh;margin:0\"><p>Sign-in received — return to Mochy.</p></body></html>";

/// A bound one-shot loopback listener.
pub struct LoopbackFinishListener {
    port: u16,
    hit: Receiver<()>,
    closed: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl LoopbackFinishListener {
    /// Bind to an ephemeral loopback port and start serving.
    pub fn bind() -> std::io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let port = listener.local_addr()?.port();
        let (sender, hit) = mpsc::channel();
        let closed = Arc::new(AtomicBool::new(false));
        let thread_closed = Arc::clone(&closed);
        let thread = std::thread::Builder::new()
            .name("mochy-login-loopback".into())
            .spawn(move || {
                for stream in listener.incoming() {
                    if thread_closed.load(Ordering::Acquire) {
                        break;
                    }
                    match stream {
                        Ok(stream) => {
                            let _ = serve_one(stream);
                            let _ = sender.send(());
                        }
                        Err(_) => break,
                    }
                }
            })?;
        Ok(LoopbackFinishListener {
            port,
            hit,
            closed,
            thread: Some(thread),
        })
    }

    /// The `continue` URL Google redirects to when this listener backs
    /// the fallback.
    pub fn finish_url(&self) -> String {
        format!("http://127.0.0.1:{}/finish", self.port)
    }

    /// Block until the finish URL was requested (the sign-in
    /// completed), or the timeout ran out.
    pub fn wait_for_hit(&self, timeout: Duration) -> Result<(), LoopbackError> {
        match self.hit.recv_timeout(timeout) {
            Ok(()) => Ok(()),
            Err(mpsc::RecvTimeoutError::Timeout) => Err(LoopbackError::Timeout),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(LoopbackError::Closed),
        }
    }
}

impl Drop for LoopbackFinishListener {
    fn drop(&mut self) {
        // unblock the accept loop (a connect-to-self wakes it) and join
        self.closed.store(true, Ordering::Release);
        if let Ok(mut stream) = TcpStream::connect(("127.0.0.1", self.port)) {
            let _ = stream.write_all(b"GET /shutdown HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n");
            let _ = stream.flush();
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn serve_one(mut stream: TcpStream) -> std::io::Result<()> {
    // read the request head (we do not care about its content; the
    // URL path was /finish by construction)
    let mut head = vec![0u8; 1024];
    let _ = stream.read(&mut head);
    let body = FINISH_PAGE.as_bytes();
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(response.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()
}

/// Loopback failures.
#[derive(Debug, thiserror::Error)]
pub enum LoopbackError {
    #[error("the loopback wait timed out")]
    Timeout,
    #[error("the loopback listener went away")]
    Closed,
}

#[allow(clippy::derive_partial_eq_without_eq)]
impl PartialEq for LoopbackError {
    fn eq(&self, other: &Self) -> bool {
        matches!(
            (self, other),
            (LoopbackError::Timeout, LoopbackError::Timeout)
                | (LoopbackError::Closed, LoopbackError::Closed)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    /// The listener serves the finish page and reports the hit. The
    /// client sends a request first (a browser would); the server
    /// reads then answers.
    #[test]
    fn listener_serves_and_signals() {
        let listener = LoopbackFinishListener::bind().unwrap();
        let url = listener.finish_url();
        assert!(url.starts_with("http://127.0.0.1:"));

        // a real loopback request (the browser-equivalent)
        let address = url
            .trim_start_matches("http://")
            .split('/')
            .next()
            .unwrap()
            .to_string();
        let mut stream = TcpStream::connect(address).unwrap();
        stream
            .write_all(b"GET /finish HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n")
            .unwrap();
        stream.flush().unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 200 OK"));
        assert!(response.contains("return to Mochy"));

        listener.wait_for_hit(Duration::from_secs(1)).unwrap();
        drop(listener); // must not hang
    }

    /// Waiting without a hit times out cleanly.
    #[test]
    fn wait_times_out() {
        let listener = LoopbackFinishListener::bind().unwrap();
        assert_eq!(
            listener.wait_for_hit(Duration::from_millis(10)),
            Err(LoopbackError::Timeout)
        );
    }
}
