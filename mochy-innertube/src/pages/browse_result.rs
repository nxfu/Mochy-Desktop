//! `pages/BrowseResult.kt` (67 lines) — the generic genre/browse
//! drill-down result, one of the two glue files.

use crate::models::ytitem::{self, YTItem};

/// `BrowseResult.kt:8-11`.
#[derive(Debug, Clone, PartialEq)]
pub struct BrowseResult {
    pub title: Option<String>,
    pub items: Vec<BrowseItem>,
}

/// `BrowseResult.Item` (BrowseResult.kt:12-15).
#[derive(Debug, Clone, PartialEq)]
pub struct BrowseItem {
    pub title: Option<String>,
    pub items: Vec<YTItem>,
}

impl BrowseResult {
    /// `filterExplicit` (BrowseResult.kt:17-32) — empty sections drop.
    pub fn filter_explicit(self, enabled: bool) -> Self {
        Self::filtered(self, |items| ytitem::filter_explicit(items, enabled))
    }

    /// `filterVideoSongs` (BrowseResult.kt:34-49).
    pub fn filter_video_songs(self, disable_videos: bool) -> Self {
        Self::filtered(self, |items| {
            ytitem::filter_video_songs(items, disable_videos)
        })
    }

    /// `filterYoutubeShorts` (BrowseResult.kt:51-66).
    pub fn filter_youtube_shorts(self, enabled: bool) -> Self {
        Self::filtered(self, |items| ytitem::filter_youtube_shorts(items, enabled))
    }

    fn filtered(this: Self, filter: impl Fn(&[YTItem]) -> Vec<YTItem>) -> Self {
        BrowseResult {
            title: this.title,
            items: this
                .items
                .into_iter()
                .filter_map(|item| {
                    let items = filter(&item.items);
                    if items.is_empty() {
                        None
                    } else {
                        Some(BrowseItem {
                            title: item.title,
                            items,
                        })
                    }
                })
                .collect(),
        }
    }
}
