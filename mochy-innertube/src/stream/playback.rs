//! Playback data, the stream engine and the resolution entry points —
//! the `playerResponseForPlayback` half of `YTPlayerUtils.kt`
//! (lines 188-347 + 800-806) plus the stream-backend selection
//! (`backend`, preWarm, InnerTubeX).
//!
//! The entry orchestration mirrors upstream exactly:
//!
//! 1. resolve (the walk) → **or stand-in** (YouTube refused the
//!    upload: the artist's other upload of the song, ≤ 2 tried);
//! 2. `preferStudio`: a music video plays its studio upload's sound
//!    instead — no skits, no sound effects;
//! 3. a guest failure that is neither rate-limit, bot-check nor
//!    premium-only rotates the guest session and retries once
//!    (BotDetectionMitigator), because a burned visitor id is the
//!    usual reason a guest walk fails;
//! 4. the upload-refusal wrapper is unwrapped for the caller — it
//!    only ever carried the video details the stand-in lookup needed.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::models::endpoint::MUSIC_VIDEO_TYPE_ATV;
use crate::models::response::player::{AudioConfig, Format, PlaybackTracking, VideoDetails};
use crate::transport::InnerTubeTransport;
use crate::youtube::YouTube;

use super::cipher::{Cipher, PlayerJsFetcher};
use super::ejs::EjsSolver;
use super::jsengine::Engine;
use super::potoken::PoTokenGenerator;
use super::ranking::RankingConditions;
use super::song_versions::{self, MAX_STAND_INS};
use super::validate::PlaybackCache;

/// `StreamBackend` (PreferenceKeys.kt:169-172): who finds a song's
/// stream — the InnerTubeX engine with the client walk as its backup,
/// or the walk alone. The app keeps this in step with the setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StreamBackend {
    /// The InnerTubeX-equivalent alternate engine first, the walk as
    /// its backup (upstream default). The engine itself sits behind
    /// the `innertubex` cargo feature — see [`super::innertubex`];
    /// without the feature the walk runs alone, exactly like
    /// upstream's `StreamBackend.CLASSIC`.
    #[default]
    InnertubeX,
    /// The client walk alone.
    Classic,
}

/// `PlaybackData` (YTPlayerUtils.kt:188-201).
#[derive(Debug, Clone)]
pub struct PlaybackData {
    pub audio_config: Option<AudioConfig>,
    pub video_details: Option<VideoDetails>,
    pub playback_tracking: Option<PlaybackTracking>,
    pub format: Format,
    pub stream_url: String,
    pub stream_expires_in_seconds: i32,
    /// The client that found the stream (`streamClient`), "unknown"
    /// when the walk failed to note one.
    pub stream_client: String,
    /// The headers googlevideo expects alongside the url
    /// (`streamHeaders`).
    pub stream_headers: Vec<(String, String)>,
    /// YouTube refused the song, so this is its music video's sound.
    pub from_music_video: bool,
    /// A music video asked for, playing its studio upload's sound
    /// instead.
    pub from_studio: bool,
}

impl PlaybackData {
    /// The walk's success constructor (YTPlayerUtils.kt:790-799) —
    /// upstream passes the same eight values as named arguments.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn from_parts(
        audio_config: Option<AudioConfig>,
        video_details: Option<VideoDetails>,
        playback_tracking: Option<PlaybackTracking>,
        format: Format,
        stream_url: String,
        stream_expires_in_seconds: i32,
        stream_client: &str,
        stream_headers: Vec<(String, String)>,
    ) -> Self {
        PlaybackData {
            audio_config,
            video_details,
            playback_tracking,
            format,
            stream_url,
            stream_expires_in_seconds,
            stream_client: stream_client.to_owned(),
            stream_headers,
            from_music_video: false,
            from_studio: false,
        }
    }
}

/// The walk's error taxonomy — the four upstream exceptions plus the
/// transport failures that can happen before any client is asked.
#[derive(Debug, thiserror::Error)]
pub enum StreamError {
    /// `YouTubeRateLimitedException` (YTPlayerUtils.kt:1228-1233):
    /// YouTube answered 429 — nothing else is asked, every client
    /// goes to the same host.
    #[error("YouTube is limiting requests from this network. Try again in a few minutes.")]
    RateLimited,
    /// `YouTubeBotCheckException` (YTPlayerUtils.kt:1235-1237): the
    /// main client answered "sign in to confirm you're not a bot"
    /// twice with a fresh visitor id in between, and no other client
    /// served — it's this network YouTube doubts.
    #[error("YouTube wants a bot check from this network ({0})")]
    BotCheck(String),
    /// `YouTubePremiumOnlyException` (YTPlayerUtils.kt:1239-1241).
    #[error("This song is only for YouTube Music Premium members ({0})")]
    PremiumOnly(String),
    /// `UploadRefusedException` (YTPlayerUtils.kt:1200-1203): no
    /// client got as far as a stream, so another upload of the song
    /// is worth a try — the wrapper carries the video details the
    /// stand-in lookup needs; callers see the wrapped failure
    /// (`unwrapRefusal`). The details are boxed: `VideoDetails` is
    /// 192 bytes and inline it pushed this error past the size where
    /// passing it around stops being free.
    #[error("YouTube refused this upload")]
    UploadRefused {
        failure: Box<StreamError>,
        video_details: Option<Box<VideoDetails>>,
    },
    /// `PlaybackException(ERROR_CODE_REMOTE_ERROR)` — the last client
    /// answered, but not with a playable status.
    #[error("{0}")]
    Unplayable(String),
    /// A plain `Exception(summary)` — every client's reason, joined.
    #[error("{0}")]
    Other(String),
    /// A transport-level failure outside the per-client loop.
    #[error("{0}")]
    Net(#[from] crate::error::InnertubeError),
}

impl StreamError {
    /// `unwrapRefusal` (YTPlayerUtils.kt:317-319): the refusal only
    /// carried the song's details to the stand-in lookup; callers get
    /// the error it wraps.
    pub fn unwrap_refusal(self) -> Self {
        match self {
            StreamError::UploadRefused { failure, .. } => *failure,
            other => other,
        }
    }
}

/// `BOT_CHECK_REST_MS` (YTPlayerUtils.kt:140): after a double bot
/// check the main client is left alone for half an hour — it's the
/// network youtube doubts, and asking for every song keeps it
/// doubting.
pub(crate) const BOT_CHECK_REST: Duration = Duration::from_millis(30 * 60_000);

/// The stream engine: the process-lifetime state the walk needs —
/// the QuickJS engine (EJS solver, cipher runtime, BotGuard VM), the
/// PO-token generator, the main-client rest timer, the studio-version
/// memo and the resolved-url cache. Upstream kept all of this in
/// singletons; the port hands it to the caller as one value.
pub struct StreamEngine {
    /// The backend selection (`@Volatile var backend`), kept in step
    /// with the setting by the app shell.
    pub backend: std::sync::atomic::AtomicBool,
    js: Arc<Engine>,
    /// The signature cipher + player-script fetcher.
    pub cipher: Cipher,
    ejs: tokio::sync::Mutex<Option<Arc<EjsSolver>>>,
    /// The BotGuard PO-token generator.
    pub potoken: PoTokenGenerator,
    /// `visionOsRestingUntil`.
    pub(crate) visionos_resting_until: tokio::sync::Mutex<Option<Instant>>,
    /// `studioIds` — music video id → its studio upload's id ("" for
    /// none), looked up once a session.
    studio_ids: tokio::sync::Mutex<HashMap<String, String>>,
    /// The resolved-url cache (403 invalidation target).
    pub cache: tokio::sync::Mutex<PlaybackCache>,
    /// The bot-detection failure counter the playback path reports
    /// into (upstream's BotDetectionMitigator singleton).
    pub mitigator: crate::guest::BotDetectionMitigator,
    /// The alternate InnerTubeX-equivalent engine (behind the
    /// `innertubex` feature; the unimplemented seam until the port
    /// lands).
    #[cfg(feature = "innertubex")]
    alternate: std::sync::Arc<dyn super::innertubex::AlternateStreamEngine>,
}

impl std::fmt::Debug for StreamEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamEngine")
            .field(
                "backend_is_innertubex",
                &self.backend.load(std::sync::atomic::Ordering::Relaxed),
            )
            .finish_non_exhaustive()
    }
}

impl StreamEngine {
    /// Boot the engine with its cipher cache under `cache_dir`
    /// (upstream: `filesDir/cipher_cache`).
    pub fn new(cache_dir: impl Into<PathBuf>) -> Result<Self, super::jsengine::JsError> {
        let js = Arc::new(Engine::new()?);
        let cipher = Cipher::new(PlayerJsFetcher::new(cache_dir), Arc::clone(&js));
        let potoken = PoTokenGenerator::new(Arc::clone(&js));
        Ok(StreamEngine {
            backend: std::sync::atomic::AtomicBool::new(true),
            js,
            cipher,
            ejs: tokio::sync::Mutex::const_new(None),
            potoken,
            visionos_resting_until: tokio::sync::Mutex::const_new(None),
            studio_ids: tokio::sync::Mutex::new(HashMap::new()),
            cache: tokio::sync::Mutex::new(PlaybackCache::new()),
            mitigator: crate::guest::BotDetectionMitigator::new(),
            #[cfg(feature = "innertubex")]
            alternate: std::sync::Arc::new(super::innertubex::UnimplementedEngine),
        })
    }

    /// The current backend selection.
    pub fn backend(&self) -> StreamBackend {
        if self.backend.load(std::sync::atomic::Ordering::Relaxed) {
            StreamBackend::InnertubeX
        } else {
            StreamBackend::Classic
        }
    }

    /// The alternate engine handle (feature-gated).
    #[cfg(feature = "innertubex")]
    pub(crate) fn alternate_engine(
        &self,
    ) -> std::sync::Arc<dyn super::innertubex::AlternateStreamEngine> {
        std::sync::Arc::clone(&self.alternate)
    }

    /// Set the backend selection (the settings mirror calls this).
    pub fn set_backend(&self, backend: StreamBackend) {
        self.backend.store(
            matches!(backend, StreamBackend::InnertubeX),
            std::sync::atomic::Ordering::Relaxed,
        );
    }

    /// True once the BotGuard engine holds a usable session token
    /// (`hasPoToken`) — surfaced in diagnostics.
    pub fn has_po_token(&self) -> bool {
        self.potoken.has_session_token()
    }

    /// `preWarmPoToken` (YTPlayerUtils.kt:109-114): boot the BotGuard
    /// engine before the first track needs it. Safe to call more than
    /// once.
    pub async fn prewarm_po_token(&self, youtube: &YouTube) {
        let session = youtube.transport().session();
        let session_id = {
            let guard = session
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if guard.cookie.is_some() {
                guard
                    .data_sync_id
                    .clone()
                    .or_else(|| guard.visitor_data.clone())
            } else {
                guard.visitor_data.clone()
            }
        };
        if let Some(session_id) = session_id {
            self.potoken.prewarm(youtube.transport(), &session_id).await;
        }
    }

    /// `preWarmInnerTubeX` (YTPlayerUtils.kt:96-100): compile the
    /// player script before the first song needs it. On the classic
    /// backend there is nothing to do — but the EJS solver benefits
    /// either way, so the port warms it unconditionally when asked.
    pub async fn prewarm(&self, youtube: &YouTube) {
        if let Err(error) = self.ejs_solver(youtube.transport()).await {
            tracing::warn!(target: "mochy_stream", error = %error, "prewarm could not compile the ejs solver; the first song will do it");
        }
    }

    /// The lazily compiled EJS solver (EjsNTransformSolver.kt:62-85
    /// `getOrCreateSolver`): built over the current player script the
    /// first time an n-parameter needs solving. A player-script fetch
    /// failure degrades to "no solver" — upstream's
    /// `PlayerJsFetcher.getPlayerJs()` catches every exception and
    /// answers null, so the walk keeps the untransformed URL (the
    /// 2026-11-21 audit fixed the port from aborting the whole walk
    /// here).
    pub(crate) async fn ejs_solver(
        &self,
        transport: &InnerTubeTransport,
    ) -> Result<Option<Arc<EjsSolver>>, crate::error::InnertubeError> {
        let mut guard = self.ejs.lock().await;
        if let Some(solver) = guard.as_ref() {
            return Ok(Some(Arc::clone(solver)));
        }
        let player_js = match self.cipher.player_js(transport).await {
            Ok(Some(player_js)) => player_js,
            Ok(None) => return Ok(None),
            Err(error) => {
                tracing::warn!(target: "mochy_stream", error = %error, "player js fetch failed; the ejs solver stays unavailable and the url passes through untransformed");
                return Ok(None);
            }
        };
        match EjsSolver::compile(Arc::clone(&self.js), &player_js).await {
            Ok(solver) => {
                let solver = Arc::new(solver);
                *guard = Some(Arc::clone(&solver));
                Ok(Some(solver))
            }
            Err(error) => {
                tracing::error!(target: "mochy_stream", error = %error, "ejs solver creation failed");
                Ok(None)
            }
        }
    }

    /// The studio-version memo: `studioIds[videoId]` — `Some("")`
    /// means "looked, there is none" (YTPlayerUtils.kt:307-311).
    pub(crate) async fn studio_id(&self, video_id: &str) -> Option<String> {
        self.studio_ids
            .lock()
            .await
            .get(video_id)
            .cloned()
            .and_then(|id| (!id.is_empty()).then_some(id))
    }

    pub(crate) async fn remember_studio_id(&self, video_id: &str, studio_id: Option<&str>) {
        self.studio_ids.lock().await.insert(
            video_id.to_owned(),
            studio_id.unwrap_or_default().to_owned(),
        );
    }
}

/// One playback request (the `playerResponseForPlayback` parameters,
/// YTPlayerUtils.kt:232-239).
#[derive(Debug, Clone)]
pub struct PlaybackRequest {
    pub video_id: String,
    pub playlist_id: Option<String>,
    /// The quality + network conditions the ranking resolves against.
    pub conditions: RankingConditions,
    /// A music video plays its song's studio upload when there is
    /// one, without the video's skits and sound effects.
    pub prefer_studio: bool,
}

impl PlaybackRequest {
    pub fn new(video_id: impl Into<String>, conditions: RankingConditions) -> Self {
        PlaybackRequest {
            video_id: video_id.into(),
            playlist_id: None,
            conditions,
            prefer_studio: false,
        }
    }

    pub fn with_playlist(mut self, playlist_id: Option<String>) -> Self {
        self.playlist_id = playlist_id;
        self
    }

    pub fn with_prefer_studio(mut self, prefer: bool) -> Self {
        self.prefer_studio = prefer;
        self
    }

    pub(crate) fn is_uploaded_track(&self) -> bool {
        self.playlist_id
            .as_deref()
            .is_some_and(|playlist| playlist == "MLPT" || playlist.contains("MLPT"))
    }
}

/// `playerResponseForPlayback` (YTPlayerUtils.kt:232-265): the
/// resolution entry point the player calls.
pub async fn resolve_playback(
    youtube: &YouTube,
    engine: &StreamEngine,
    request: &PlaybackRequest,
) -> Result<PlaybackData, StreamError> {
    let mut first_attempt = super::walk::resolve_playback_data(youtube, engine, request).await;
    // orStandIn: YouTube refused the upload, the artist's other
    // upload usually plays
    first_attempt = or_stand_in(youtube, engine, request, first_attempt).await;

    if request.prefer_studio
        && let Ok(data) = &first_attempt
        && let Some(studio) = studio_version_of(youtube, engine, request, data).await
    {
        return Ok(studio);
    }

    // A rate limit or bot check is the network and a premium-only
    // song is the song, so a fresh guest session and a second walk
    // would only play nothing again (YTPlayerUtils.kt:248-264).
    let is_guest = youtube
        .transport()
        .session()
        .read()
        .map(|guard| guard.cookie.is_none())
        .unwrap_or(true);
    let first_failure = match &first_attempt {
        Ok(_) => None,
        Err(error) => Some(match error {
            StreamError::UploadRefused { failure, .. } => failure.as_ref(),
            other => other,
        }),
    };
    if let (true, Some(failure)) = (is_guest, first_failure)
        && !matches!(
            failure,
            StreamError::RateLimited | StreamError::BotCheck(_) | StreamError::PremiumOnly(_)
        )
    {
        tracing::warn!(target: "mochy_stream", "playback failed for guest; rotating the session and retrying");
        // BotDetectionMitigator.rotateGuestSession: the old
        // visitor id stays until a new one arrives
        let _ = youtube.refresh_visitor_data().await;
        let retry = super::walk::resolve_playback_data(youtube, engine, request).await;
        if retry.is_ok() {
            engine.mitigator.notify_playback_success();
        }
        return retry.map_err(StreamError::unwrap_refusal);
    }

    if first_attempt.is_ok() {
        engine.mitigator.notify_playback_success();
    }
    first_attempt.map_err(StreamError::unwrap_refusal)
}

/// `orStandIn` (YTPlayerUtils.kt:267-290): the refusal only carries
/// the song's details to the stand-in lookup, and a stand-in that
/// hits the rate limit propagates it — it's the network again.
async fn or_stand_in(
    youtube: &YouTube,
    engine: &StreamEngine,
    request: &PlaybackRequest,
    attempt: Result<PlaybackData, StreamError>,
) -> Result<PlaybackData, StreamError> {
    let details = match &attempt {
        Err(StreamError::UploadRefused { video_details, .. }) => video_details.clone(),
        _ => return attempt,
    };
    let (Some(title), Some(artist), Some(length)) = (
        details.as_deref().and_then(|details| details.title.clone()),
        details
            .as_deref()
            .and_then(|details| details.author.clone()),
        details
            .as_deref()
            .and_then(|details| details.length_seconds.parse::<i32>().ok()),
    ) else {
        return attempt;
    };

    let stand_ins =
        song_versions::other_uploads(youtube, &request.video_id, &title, &artist, length).await;
    for stand_in in stand_ins.iter().take(MAX_STAND_INS) {
        let stand_in_request = PlaybackRequest {
            video_id: stand_in.id.clone(),
            playlist_id: None,
            conditions: request.conditions,
            prefer_studio: false,
        };
        let attempt = super::walk::resolve_playback_data(youtube, engine, &stand_in_request).await;
        match attempt {
            Err(StreamError::RateLimited) => return Err(StreamError::RateLimited),
            Err(_) => continue,
            Ok(data) => {
                tracing::info!(target: "mochy_stream",
                    from = %request.video_id, to = %stand_in.id,
                    "youtube refused the upload, playing its other upload");
                // the song keeps its own details, and the play isn't
                // reported as the stand-in's
                return Ok(PlaybackData {
                    video_details: details.as_deref().cloned(),
                    playback_tracking: None,
                    from_music_video: stand_in.is_video_song,
                    ..data
                });
            }
        }
    }
    attempt
}

/// `studioVersionOf` (YTPlayerUtils.kt:295-315): the studio upload's
/// stream for a music video, with the video's own details kept; null
/// for a song or a video with no studio version.
async fn studio_version_of(
    youtube: &YouTube,
    engine: &StreamEngine,
    request: &PlaybackRequest,
    data: &PlaybackData,
) -> Option<PlaybackData> {
    let details = data.video_details.as_ref()?;
    let music_video_type = details.music_video_type.as_deref()?;
    if music_video_type == MUSIC_VIDEO_TYPE_ATV {
        return None; // already the studio upload
    }
    let title = details.title.as_deref()?;
    let artist = details
        .author
        .as_deref()?
        .trim_end_matches(" - Topic")
        .to_owned();

    // looked up once a session per video, the answer (or that there's
    // none) kept for every replay and seek after
    let studio_id = match engine.studio_id(&request.video_id).await {
        Some(known) => known,
        None => {
            let found = song_versions::song_version_id(youtube, title, &artist)
                .await
                .filter(|id| *id != request.video_id);
            engine
                .remember_studio_id(&request.video_id, found.as_deref())
                .await;
            found?
        }
    };
    let studio_request = PlaybackRequest {
        video_id: studio_id,
        playlist_id: None,
        conditions: request.conditions,
        prefer_studio: false,
    };
    let studio = super::walk::resolve_playback_data(youtube, engine, &studio_request)
        .await
        .ok()?;
    tracing::info!(target: "mochy_stream",
        from = %request.video_id, to = %studio_request.video_id,
        "a music video, playing its studio version");
    Some(PlaybackData {
        video_details: data.video_details.clone(),
        playback_tracking: None,
        from_studio: true,
        ..studio
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::PlayerResponse;

    #[test]
    fn backend_flags_round_trip() {
        let engine = StreamEngine::new("/tmp/mochy-test-cipher").unwrap();
        assert_eq!(engine.backend(), StreamBackend::InnertubeX);
        engine.set_backend(StreamBackend::Classic);
        assert_eq!(engine.backend(), StreamBackend::Classic);
        engine.set_backend(StreamBackend::InnertubeX);
        assert_eq!(engine.backend(), StreamBackend::InnertubeX);
    }

    #[test]
    fn uploaded_track_detection_follows_the_playlist_id() {
        let request = PlaybackRequest::new(
            "abc",
            RankingConditions {
                quality: crate::stream::ranking::AudioQuality::High,
                network_metered: false,
                data_saver: false,
            },
        )
        .with_playlist(Some("MLPT_7".into()));
        assert!(request.is_uploaded_track());
        let plain = PlaybackRequest::new("abc", request.conditions);
        assert!(!plain.is_uploaded_track());
    }

    #[test]
    fn refusal_unwraps_for_callers() {
        let inner = StreamError::Other("no stream".into());
        let wrapped = StreamError::UploadRefused {
            failure: Box::new(inner),
            video_details: None,
        };
        assert!(matches!(
            wrapped.unwrap_refusal(),
            StreamError::Other(message) if message == "no stream"
        ));
    }

    #[test]
    fn player_response_helpers() {
        let mut response = PlayerResponse::default();
        assert!(!super::super::walk::is_bot_check(&response));
        assert!(!super::super::walk::is_premium_only(&response));
        response.playability_status = Some(crate::models::response::player::PlayabilityStatus {
            status: "LOGIN_REQUIRED".into(),
            reason: Some("Please sign in to confirm you're not a bot".into()),
        });
        assert!(super::super::walk::is_bot_check(&response));
        response.playability_status = Some(crate::models::response::player::PlayabilityStatus {
            status: "LOGIN_REQUIRED".into(),
            reason: Some("This content is only available for Premium members".into()),
        });
        assert!(super::super::walk::is_premium_only(&response));
    }
}
