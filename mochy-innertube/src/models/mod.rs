//! The DTO layer — the port of `providers/innertube/models/`
//! (pin `bde01cb4`), minus the dead comment stack
//! (`models/comment/` — implemented upstream, unwired; PORT_SPEC §4) and
//! `AddItemYouTubePlaylistResponse` (no call sites).
//!
//! Two kinds of types live here, exactly like upstream:
//! - **wire DTOs** (`Runs`, `Menu`, `MusicResponsiveListItemRenderer`, the
//!   `response/` envelopes …) — serde mirrors of the kotlinx-serialization
//!   data classes. Kotlin non-null fields without defaults stay required,
//!   nullable fields become `Option`; serde ignores unknown keys exactly
//!   like upstream's `Json { ignoreUnknownKeys = true }`
//!   (InnerTube.kt:80-84);
//! - **domain models** ([`YTItem`] and its four variants, [`AccountInfo`],
//!   [`MediaInfo`] …) — what the `pages/` parsers produce and the app
//!   consumes.
//!
//! ## Documented decode deviation
//!
//! kotlinx requires a *key* for every constructor parameter without a
//! default (a nullable type only allows the value `null`, not a missing
//! key), so a malformed response fails the whole page upstream. serde's
//! `Option<T>` accepts a missing key as `None`, so the port instead lets
//! the parser drop that item. For well-formed YouTube responses the two
//! are identical; for malformed ones the port degrades instead of
//! erroring — recorded in HANDOFF ## Phase 05.

pub mod account;
pub mod common;
pub mod endpoint;
pub mod grid;
pub mod menu;
pub mod music_headers;
pub mod music_renderers;
pub mod playlist_panel;
pub mod response;
pub mod runs;
pub mod section_list;
pub mod tabs;
pub mod thumbnails;
pub mod two_column;
pub mod youtube_data_page;
pub mod ytitem;

pub use account::{
    AccountInfo, CaptionCue, MediaInfo, ReturnYouTubeDislikeResponse, SearchSuggestions,
    VideoCaptions,
};
pub use common::{
    Badges, Button, Continuation, ContinuationItemRenderer, Icon, ResponseContext,
    SubscriptionButton,
};
pub use endpoint::{
    BrowseEndpoint, FeedbackEndpoint, NavigationEndpoint, QueueAddEndpoint, SearchEndpoint,
    ShareEntityEndpoint, SubscribeEndpoint, ToggledServiceEndpoint, UrlEndpoint, WatchEndpoint,
};
pub use grid::GridRenderer;
pub use menu::Menu;
pub use music_headers::{
    MusicDetailHeaderRenderer, MusicEditablePlaylistDetailHeaderRenderer,
    MusicMultiRowImageItemRenderer, MusicNavigationButtonRenderer, MusicPlaylistEditHeaderRenderer,
    MusicResponsiveHeaderRenderer,
};
pub use music_renderers::{
    MusicCardShelfRenderer, MusicCarouselShelfRenderer, MusicDescriptionShelfRenderer,
    MusicPlaylistShelfRenderer, MusicQueueRenderer, MusicResponsiveListItemRenderer,
    MusicShelfRenderer, MusicTwoRowItemRenderer,
};
pub use playlist_panel::{
    AutomixPreviewVideoRenderer, PlaylistPanelRenderer, PlaylistPanelVideoRenderer,
};
pub use response::{
    AccountMenuResponse, BrowseResponse, ContinuationResponse, CreatePlaylistResponse,
    EditPlaylistResponse, FeedbackResponse, GetQueueResponse, GetSearchSuggestionsResponse,
    GetTranscriptResponse, ImageUploadResponse, NextResponse, PlayerResponse, SearchResponse,
};
pub use runs::{Run, Runs};
pub use section_list::{ItemSectionRenderer, SectionListRenderer};
pub use tabs::Tabs;
pub use thumbnails::{Thumbnail, ThumbnailRenderer, Thumbnails};
pub use two_column::TwoColumnBrowseResultsRenderer;
pub use youtube_data_page::YouTubeDataPage;
pub use ytitem::{Album, AlbumItem, Artist, ArtistItem, PlaylistItem, SongItem, YTItem};

/// `YouTubeConstants.kt` — the localized-title fallbacks of the search
/// summary page.
pub mod constants {
    pub const DEFAULT_TOP_RESULT: &str = "Top result";
    pub const DEFAULT_OTHER_RESULTS: &str = "Other";
}
