//! # Stream resolution — the port of `utils/YTPlayerUtils.kt` (1,241
//! lines) + `utils/potoken/` + `utils/cipher/` + `utils/sabr/` +
//! the stream-facing parts of `utils/SongVersions.kt`,
//! `utils/BotDetectionMitigator.kt` and `utils/DataSaver.kt`.
//!
//! Milestone M1, part 3 (Phase 06, retitled by its task prompt — the
//! ledger's original Phase 06 items landed with Phase 05).
//!
//! The InnerTubeX-equivalent alternate backend seam lives behind the
//! `innertubex` cargo feature (upstream `utils/InnerTubeXStreams.kt`
//! with `StreamBackend.INNERTUBEX`); SABR itself stays out of scope,
//! per decision Q7 (R1-fallback).

pub mod cipher;
pub mod ejs;
#[cfg(feature = "innertubex")]
pub mod innertubex;
pub mod jsengine;
pub mod newpipe;
pub mod playback;
pub mod potoken;
pub mod ranking;
pub mod song_versions;
pub mod urlutil;
pub mod validate;
pub mod walk;

/// The walk-order client keys — the registry's `roles` re-expressed
/// for the walk (`client.rs` keeps the single source of truth).
pub(crate) mod walk_keys {
    pub(crate) const MAIN_CLIENT_KEY: crate::client::ClientKey = crate::client::roles::MAIN_CLIENT;
    pub(crate) const METADATA_CLIENT_KEY: crate::client::ClientKey =
        crate::client::roles::METADATA_CLIENT;
    pub(crate) const ACCOUNT_CLIENT_KEYS: &[crate::client::ClientKey] =
        crate::client::roles::ACCOUNT_CLIENTS;
    pub(crate) const STREAM_FALLBACK_KEYS: &[crate::client::ClientKey] =
        crate::client::roles::STREAM_FALLBACK_CLIENTS;
}

/// Whether `client` is one of the account's clients
/// (`this in ACCOUNT_CLIENTS`, YTPlayerUtils.kt:146): the registry's
/// singletons are compared by their unique client names.
pub(crate) fn is_account_client(client: &crate::client::YouTubeClient) -> bool {
    walk_keys::ACCOUNT_CLIENT_KEYS
        .iter()
        .any(|key| client.client_name == key.name())
}

/// `innerTubeXFirst` (YTPlayerUtils.kt:321-347): the alternate
/// engine's answer, or `None` so the walk takes over. Without the
/// `innertubex` feature the walk always serves — upstream's
/// `StreamBackend.CLASSIC` shape.
pub(crate) async fn innertube_x_first(
    youtube: &crate::youtube::YouTube,
    engine: &playback::StreamEngine,
    request: &playback::PlaybackRequest,
    is_logged_in: bool,
) -> Option<playback::PlaybackData> {
    // upstream threads the login state through to the engine's own
    // account handling; the seam keeps the parameter so the future
    // engine port finds it where upstream had it
    let _ = is_logged_in;
    #[cfg(feature = "innertubex")]
    {
        let alternate = engine.alternate_engine();
        let hints = innertubex::ContentHints::for_request(request);
        let quality = innertubex::engine_quality(&request.conditions);
        // a stuck lookup never holds a song past the give-up budget
        let stream = tokio::time::timeout(
            std::time::Duration::from_millis(innertubex::GIVE_UP_MS),
            alternate.resolve(youtube, request, &hints, quality),
        )
        .await
        .ok()
        .flatten()?;
        stream.to_playback_data()
    }
    #[cfg(not(feature = "innertubex"))]
    {
        let _ = (youtube, engine, request);
        None
    }
}

pub use playback::{
    PlaybackData, PlaybackRequest, StreamBackend, StreamEngine, StreamError, resolve_playback,
};
pub use ranking::{AudioQuality, RankingConditions, select_audio_format_candidates};
pub use validate::{PlaybackCache, ProbeOutcome, validate_stream_url};
