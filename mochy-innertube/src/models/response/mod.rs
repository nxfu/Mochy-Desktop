//! `models/response/` — the response envelopes of the 22 endpoints,
//! minus `AddItemYouTubePlaylistResponse` (dead upstream, PORT_SPEC §4).

pub mod browse;
pub mod misc;
pub mod next;
pub mod player;
pub mod search;

pub use browse::BrowseResponse;
pub use misc::{
    AccountMenuResponse, ContinuationResponse, CreatePlaylistResponse, EditPlaylistResponse,
    FeedbackResponse, GetQueueResponse, GetSearchSuggestionsResponse, GetTranscriptResponse,
    ImageUploadResponse,
};
pub use next::NextResponse;
pub use player::{CaptionTrack, PlayerResponse};
pub use search::SearchResponse;
