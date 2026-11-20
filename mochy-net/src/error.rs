//! Error taxonomy for the shared HTTP layer.
//!
//! The load-bearing distinction is upstream's: IO errors retry, HTTP errors
//! surface, and 429 is its own type that callers treat as "stop asking
//! YouTube anything for a while"
//! (utils/YTPlayerUtils.kt:1229-1233 `YouTubeRateLimitedException`).

use thiserror::Error;

/// Errors from the shared HTTP layer.
#[derive(Debug, Error)]
pub enum NetError {
    /// Offline mode is on: the request was not sent
    /// (providers/innertube OfflineModeException.kt — "an IOException so
    /// callers that treat a failed connection as nothing to show carry on").
    #[error("offline mode is on")]
    Offline,

    /// YouTube answered 429: it is refusing this network for a while.
    /// Short-circuits every caller — no retries, no other clients
    /// (utils/YTPlayerUtils.kt:1229-1233).
    #[error("YouTube is limiting requests from this network. Try again in a few minutes.")]
    YouTubeRateLimited,

    /// Transport-level IO failure (connection refused, reset, timeout...).
    /// These are the ONLY errors the retry policy retries
    /// (InnerTube.kt:216 `catch (e: IOException)`).
    #[error("io error: {0}")]
    Io(#[source] Box<dyn std::error::Error + Send + Sync>),

    /// A non-2xx response that is not 429.
    #[error("http {status} from {url}")]
    Http {
        status: reqwest::StatusCode,
        url: String,
    },

    /// Anything else (serde, redirect loops, ...).
    #[error("{0}")]
    Other(String),
}

impl NetError {
    /// Whether the retry policy retries this error.
    pub fn is_retryable(&self) -> bool {
        matches!(self, NetError::Io(_))
    }
}

impl From<reqwest::Error> for NetError {
    fn from(e: reqwest::Error) -> Self {
        if let Some(status) = e.status() {
            if status.as_u16() == 429 {
                return NetError::YouTubeRateLimited;
            }
            if e.is_status() {
                return NetError::Http {
                    status,
                    url: e.url().map(|u| u.to_string()).unwrap_or_default(),
                };
            }
        }
        // transport-level failure — the retryable bucket (upstream retries
        // IOException: connect, timeout, reset, mid-body failures)
        if e.is_connect() || e.is_timeout() || e.is_request() || e.is_body() {
            return NetError::Io(Box::new(e));
        }
        NetError::Other(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_io_is_retryable() {
        let io = NetError::Io(Box::new(std::io::Error::other("reset")));
        assert!(io.is_retryable());
        assert!(!NetError::Offline.is_retryable());
        assert!(!NetError::YouTubeRateLimited.is_retryable());
        assert!(!NetError::Other("x".into()).is_retryable());
        assert!(
            !NetError::Http {
                status: reqwest::StatusCode::INTERNAL_SERVER_ERROR,
                url: String::new()
            }
            .is_retryable()
        );
    }
}
