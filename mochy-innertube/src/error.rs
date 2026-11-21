//! The innertube error taxonomy.
//!
//! Wraps [`mochy_net::NetError`] (offline, 429, IO, HTTP) and adds the
//! endpoint-specific failures: the missing-visitorData mint
//! (InnerTube.kt:199), the sw.js_data scrape (YouTube.kt:1473-1483) and
//! the upload handshake.

use thiserror::Error;

/// Everything that can go wrong below the Phase 05 facade.
#[derive(Debug, Error)]
pub enum InnertubeError {
    /// The shared HTTP layer: offline, rate-limited (the typed 429
    /// short-circuit), IO, HTTP status.
    #[error(transparent)]
    Net(#[from] mochy_net::NetError),

    /// `IllegalStateException("YouTube answered visitor_id without a
    /// visitorData")` (InnerTube.kt:199).
    #[error("visitor_id mint returned no visitorData")]
    VisitorIdMissing,

    /// The sw.js_data payload had no visitor data in its expected slot
    /// (YouTube.kt:1473-1483 raises through the Result chain).
    #[error("sw.js_data carried no visitor data")]
    VisitorDataScrapeFailed,

    /// The resumable upload start returned no `x-guploader-uploadid`
    /// (the uploadId!! requirement, YouTube.kt:1247).
    #[error("playlist_image_upload start returned no upload id")]
    UploadIdMissing,

    /// A session/client lock was poisoned.
    #[error("a session lock was poisoned")]
    LockPoisoned,

    /// The 50 MB GET disk cache could not be opened.
    #[error("disk cache: {0}")]
    Cache(#[from] mochy_net::DiskCacheError),

    /// JSON body parsing of a response failed.
    #[error("invalid JSON response: {0}")]
    Json(#[from] serde_json::Error),

    /// A DTO could not be decoded / a required response piece was absent
    /// (Phase 05 facade; maps kotlinx `MissingFieldException` and the
    /// facade's own `error(...)`/`!!` failures).
    #[error("unexpected response: {0}")]
    Parse(String),

    /// `addSongToLibrary`/`removeSongFromLibrary`: the song was not in
    /// the next response (YouTube.kt:1499-1501).
    #[error("song {0} not found in next response")]
    SongNotFound(String),

    /// The library feedback token was not available (YouTube.kt:1502-1515).
    #[error("{0} token not available")]
    LibraryTokenMissing(&'static str),

    /// No caption track the caption clients could list (YouTube.kt:1439).
    #[error("no captions for {0}")]
    NoCaptions(String),
}

impl InnertubeError {
    /// The 429 short-circuit — callers stop asking anything
    /// (utils/YTPlayerUtils.kt:1229-1233).
    pub fn is_rate_limited(&self) -> bool {
        matches!(
            self,
            InnertubeError::Net(mochy_net::NetError::YouTubeRateLimited)
        )
    }

    /// Offline mode refused the request before it went out.
    pub fn is_offline(&self) -> bool {
        matches!(self, InnertubeError::Net(mochy_net::NetError::Offline))
    }
}
