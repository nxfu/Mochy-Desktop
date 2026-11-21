//! `pages/NewReleaseAlbumPage.kt` (44 lines).

use crate::models::runs::{odd_elements, split_by_separator};
use crate::models::ytitem::{AlbumItem, Artist};
use crate::pages::home::explicit_badge;

pub struct NewReleaseAlbumPage;

impl NewReleaseAlbumPage {
    /// `fromMusicTwoRowItemRenderer` (NewReleaseAlbumPage.kt:10-43).
    pub fn from_music_two_row_item_renderer(
        renderer: &crate::models::music_renderers::MusicTwoRowItemRenderer,
    ) -> Option<AlbumItem> {
        Some(AlbumItem {
            browse_id: renderer
                .navigation_endpoint
                .browse_endpoint
                .as_ref()?
                .browse_id
                .clone(),
            playlist_id: renderer
                .thumbnail_overlay
                .as_ref()?
                .music_item_thumbnail_overlay_renderer
                .content
                .music_play_button_renderer
                .play_navigation_endpoint
                .as_ref()?
                .watch_playlist_endpoint
                .as_ref()?
                .playlist_id
                .clone()?,
            title: renderer.title.first()?.text.clone(),
            artists: Some(
                split_by_separator(renderer.subtitle.as_ref()?.runs.as_ref()?)
                    .get(1)
                    .map(|segment| {
                        odd_elements(segment)
                            .iter()
                            .map(|run| Artist {
                                name: run.text.clone(),
                                id: run
                                    .navigation_endpoint
                                    .as_ref()
                                    .and_then(|endpoint| endpoint.browse_endpoint.as_ref())
                                    .map(|browse| browse.browse_id.clone()),
                            })
                            .collect::<Vec<_>>()
                    })?,
            ),
            year: renderer
                .subtitle
                .as_ref()?
                .last()
                .and_then(|run| run.text.parse().ok()),
            thumbnail: renderer
                .thumbnail_renderer
                .music_thumbnail_renderer
                .as_ref()?
                .get_thumbnail_url()?
                .to_owned(),
            explicit: explicit_badge(&renderer.subtitle_badges),
            ..Default::default()
        })
    }
}
