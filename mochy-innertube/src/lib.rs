//! # mochy-innertube — 18-client registry, endpoints, pagination, auth
//! headers
//!
//! The YouTube InnerTube client — the most directly portable part of the
//! upstream codebase (no Android imports).
//!
//! Upstream absorption (Android, pin `bde01cb4`): `providers/innertube`
//! (106 files: InnerTube.kt transport, YouTube.kt 1,814-line facade,
//! models/, pages/ 21 parsers + 2 glue) + app `utils/YTPlayerUtils.kt`
//! (1,241 lines) + `potoken/` + `cipher/` + `sabr/`.
//!
//! ## Phase 04 scope (Milestone M1, part 1)
//!
//! - [`client`]: the 18-client registry, verbatim from
//!   `models/YouTubeClient.kt`, with runtime version overrides (risk R1)
//!   and the walk-order roles from `YTPlayerUtils.kt`;
//! - [`transport`]: the raw transport — `InnerTube.kt`'s client shape +
//!   per-request header injection + retry/429/offline + the 50 MB GET
//!   disk cache, built on [`mochy_net`];
//! - [`auth`]: cookie parsing + the origin-bound SAPISIDHASH;
//! - [`session`]: the session cell (`Arc<RwLock<Session>>`) and the
//!   settings mirror (`App.kt` initializeSettings +
//!   observeSettingsChanges semantics, secrets from [`mochy_vault`]);
//! - [`endpoints`]: all 22 endpoints (blueprint ch. 4.1) returning raw
//!   JSON;
//! - [`guest`]: the sw.js_data visitor-data scrape +
//!   `BotDetectionMitigator` semantics;
//! - [`paging`]: the continuation pager — seen-set dedup, the
//!   50-request ceiling, caller-driven;
//! - [`models`]: the DTO layer + domain models (Phase 05);
//! - [`pages`]: the 20 parsers + 2 glue files, pure functions
//!   DTO → domain (Phase 05);
//! - [`youtube`]: the `YouTube.kt` facade over it all (Phase 05);
//! - [`captions`]: the Captions.kt json3 → LRC/cues converters
//!   (Phase 05);
//! - [`utils`]: Utils.kt's parse helpers + the YouTubeUrlParser
//!   (Phase 05).
//!
//! The Phase 05 acceptance — parser tests over the committed fixture
//! corpus, serde round-trips, offline facade wire tests — lives in
//! `tests/parsers.rs` and `tests/facade_wire.rs` (see
//! docs/PROGRESS.md).
//!
//! ## Layering
//!
//! Infrastructure crate like [`mochy_net`] (PORT_SPEC §3.1 rule 1's
//! unavoidable case): the transport is async (tokio) because every
//! upstream endpoint is a `suspend fun`; the domain logic (registry,
//! auth, paging, guest scraping) is sync and exhaustively testable. No
//! GPUI, no platform-specific code.

pub mod auth;
pub mod body;
pub mod captions;
pub mod client;
pub mod context;
pub mod endpoints;
pub mod error;
pub mod guest;
pub mod models;
pub mod pages;
pub mod paging;
pub mod session;
pub mod stream;
pub mod transport;
pub mod utils;
pub mod youtube;

pub use auth::{authorization_header, parse_cookie_string, sapisidhash, sha1_hex};
pub use client::{
    API_URL_YOUTUBE_MUSIC, ClientKey, ClientRegistry, ORIGIN_YOUTUBE, ORIGIN_YOUTUBE_MUSIC,
    REFERER_YOUTUBE_MUSIC, USER_AGENT_WEB, YouTubeClient,
};
pub use context::{Context, ThirdParty, YouTubeLocale};
pub use error::InnertubeError;
pub use guest::{BotDetectionMitigator, extract_visitor_data};
pub use paging::{Admit, ContinuationPager, EmptyPagePolicy, MAX_REQUESTS, StopReason};
pub use session::{Session, SessionMirror, SharedSession, SystemLocale};
pub use transport::{InnerTubeTransport, TransportConfig, default_cache_dir};

#[cfg(test)]
mod tests {
    #[test]
    fn crate_skeleton_presents() {
        assert_eq!(env!("CARGO_PKG_NAME"), "mochy-innertube");
    }
}
