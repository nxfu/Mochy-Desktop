//! The `pages/` package — 20 parsers + 2 glue files ported from
//! `providers/innertube/pages/` (pin `bde01cb4`) as **pure functions**
//! DTO → domain: no IO, no clock, no randomness, exhaustively testable
//! over the committed fixture corpus.
//!
//! Skipped upstream files (documented in HANDOFF ## Phase 05):
//! - `LibraryAlbumsPage.kt` — dead upstream (PORT_SPEC §4);
//! - `NewPipe.kt` — not a parser but the NewPipeExtractor last-resort
//!   stream bridge (IO + JS deobfuscation); Phase 08 scope per the
//!   PROGRESS ledger ("NewPipeExtractor-needed parts re-implemented
//!   studying rustypipe").
//!
//! The tiny continuation-page data classes (`ArtistItemsContinuationPage`,
//! `PlaylistContinuationPage`, `LibraryContinuationPage` — 8 lines each
//! upstream) fold into their parent parser's module.

pub mod album;
pub mod artist;
pub mod artist_items;
pub mod browse_result;
pub mod charts;
pub mod explore;
pub mod history;
pub mod home;
pub mod library;
pub mod mood_genres;
pub mod new_release;
pub mod next;
pub mod page_helper;
pub mod playlist;
pub mod related;
pub mod search;
pub mod search_suggestion;
pub mod search_summary;

pub use album::AlbumPage;
pub use artist::{ArtistPage, ArtistSection};
pub use artist_items::{ArtistItemsContinuationPage, ArtistItemsPage};
pub use browse_result::{BrowseItem, BrowseResult};
pub use charts::{ChartSection, ChartType, ChartsPage};
pub use explore::ExplorePage;
pub use history::{HistoryPage, HistorySection};
pub use home::{Chip, HomePage, HomeSection};
pub use library::{LibraryContinuationPage, LibraryPage};
pub use mood_genres::{MoodAndGenres, MoodAndGenresItem};
pub use next::{NextPage, NextResult};
pub use page_helper::{LibraryFeedbackTokens, PageHelper};
pub use playlist::{PlaylistContinuationPage, PlaylistPage};
pub use related::RelatedPage;
pub use search::{SearchPage, SearchResult};
pub use search_suggestion::SearchSuggestionPage;
pub use search_summary::{SearchSummary, SearchSummaryPage};
