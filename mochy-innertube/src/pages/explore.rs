//! `pages/ExplorePage.kt` (10 lines).

use crate::models::ytitem::AlbumItem;
use crate::pages::mood_genres::MoodAndGenresItem;

/// `ExplorePage.kt:7-10`.
#[derive(Debug, Clone, PartialEq)]
pub struct ExplorePage {
    pub new_release_albums: Vec<AlbumItem>,
    pub mood_and_genres: Vec<MoodAndGenresItem>,
}
