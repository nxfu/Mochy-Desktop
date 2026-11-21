//! `pages/PageHelper.kt` (120 lines) — the shared navigation helpers.

use crate::models::endpoint::NavigationEndpoint;
use crate::models::menu::{MenuItem, ToggleMenuServiceRenderer};
use crate::models::music_renderers::FlexColumn;
use crate::models::runs::Run;

const LIBRARY_ADD_ICONS: &[&str] = &["LIBRARY_ADD", "BOOKMARK_BORDER"];
const LIBRARY_SAVED_ICONS: &[&str] = &["LIBRARY_SAVED", "BOOKMARK", "LIBRARY_REMOVE"];

/// `PageHelper.LibraryFeedbackTokens` (PageHelper.kt:13-16).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LibraryFeedbackTokens {
    pub add_token: Option<String>,
    pub remove_token: Option<String>,
}

/// The module facade — every helper is a free function here (upstream's
/// `object PageHelper`).
pub struct PageHelper;

impl PageHelper {
    /// `isLibraryIcon` (PageHelper.kt:18-23).
    pub fn is_library_icon(icon_type: Option<&str>) -> bool {
        let Some(icon_type) = icon_type else {
            return false;
        };
        if icon_type == "KEEP" || icon_type == "KEEP_OFF" {
            return false;
        }
        LIBRARY_ADD_ICONS.contains(&icon_type)
            || LIBRARY_SAVED_ICONS.contains(&icon_type)
            || icon_type.starts_with("LIBRARY_")
    }

    /// `isAddLibraryIcon` (PageHelper.kt:25-27).
    pub fn is_add_library_icon(icon_type: Option<&str>) -> bool {
        icon_type.is_some_and(|icon| LIBRARY_ADD_ICONS.contains(&icon))
    }

    /// `isSavedLibraryIcon` (PageHelper.kt:29-31).
    pub fn is_saved_library_icon(icon_type: Option<&str>) -> bool {
        icon_type.is_some_and(|icon| LIBRARY_SAVED_ICONS.contains(&icon))
    }

    /// `extractRuns` (PageHelper.kt:33-50) — the runs whose navigation
    /// endpoint is of `typeLike` (a musicVideoType or a pageType,
    /// matched by substring).
    pub fn extract_runs(columns: &[FlexColumn], type_like: &str) -> Vec<Run> {
        let mut filtered = Vec::new();
        for column in columns {
            let Some(runs) = column
                .music_responsive_list_item_flex_column_renderer
                .text
                .as_ref()
                .and_then(|text| text.runs.as_ref())
            else {
                continue;
            };
            for run in runs {
                if let Some(type_str) = run_type(run.navigation_endpoint.as_ref())
                    && type_str.contains(type_like)
                {
                    filtered.push(run.clone());
                }
            }
        }
        filtered
    }

    /// `extractLibraryTokensFromMenuItems` (PageHelper.kt:52-86).
    pub fn extract_library_tokens_from_menu_items(
        items: Option<&[MenuItem]>,
    ) -> LibraryFeedbackTokens {
        let Some(items) = items else {
            return LibraryFeedbackTokens::default();
        };

        let mut tokens = LibraryFeedbackTokens::default();
        for item in items {
            let Some(toggle) = item.toggle_menu_service_item_renderer.as_ref() else {
                continue;
            };
            let icon_type = toggle.default_icon.icon_type.as_str();

            if icon_type == "KEEP" || icon_type == "KEEP_OFF" {
                continue;
            }
            if !Self::is_library_icon(Some(icon_type)) {
                continue;
            }

            let default_token = toggle
                .default_service_endpoint
                .feedback_endpoint
                .as_ref()
                .map(|endpoint| endpoint.feedback_token.clone());
            let toggled_token = toggle
                .toggled_service_endpoint
                .as_ref()
                .and_then(|endpoint| endpoint.feedback_endpoint.as_ref())
                .map(|endpoint| endpoint.feedback_token.clone());

            if Self::is_add_library_icon(Some(icon_type)) {
                // add icon: the default action adds, the toggled one removes
                if tokens.add_token.is_none() {
                    tokens.add_token = default_token;
                }
                if tokens.remove_token.is_none() {
                    tokens.remove_token = toggled_token;
                }
            } else if Self::is_saved_library_icon(Some(icon_type)) {
                // saved icon: the default action removes, the toggled one adds
                if tokens.remove_token.is_none() {
                    tokens.remove_token = default_token;
                }
                if tokens.add_token.is_none() {
                    tokens.add_token = toggled_token;
                }
            }
        }
        tokens
    }

    /// `extractFeedbackToken` (PageHelper.kt:88-119).
    pub fn extract_feedback_token(
        menu: Option<&ToggleMenuServiceRenderer>,
        type_: &str,
    ) -> Option<String> {
        let menu = menu?;
        let default_token = menu
            .default_service_endpoint
            .feedback_endpoint
            .as_ref()
            .map(|endpoint| endpoint.feedback_token.clone());
        let toggled_token = menu
            .toggled_service_endpoint
            .as_ref()
            .and_then(|endpoint| endpoint.feedback_endpoint.as_ref())
            .map(|endpoint| endpoint.feedback_token.clone());
        let icon_type = menu.default_icon.icon_type.as_str();

        // the song is not in the library when the menu still offers adding
        let song_not_in_library = LIBRARY_ADD_ICONS.contains(&icon_type);

        match type_ {
            "LIBRARY_ADD" => {
                if song_not_in_library {
                    default_token
                } else {
                    toggled_token
                }
            }
            "LIBRARY_REMOVE" | "LIBRARY_SAVED" => {
                if song_not_in_library {
                    toggled_token
                } else {
                    default_token
                }
            }
            _ => {
                if icon_type == type_ {
                    default_token
                } else {
                    toggled_token
                }
            }
        }
    }
}

/// The `typeStr` chain of `extractRuns` (PageHelper.kt:40-41):
/// musicVideoType first, pageType second.
fn run_type(endpoint: Option<&NavigationEndpoint>) -> Option<&str> {
    let endpoint = endpoint?;
    if let Some(video_type) = endpoint.music_video_type() {
        return Some(video_type);
    }
    endpoint
        .browse_endpoint
        .as_ref()?
        .browse_endpoint_context_supported_configs
        .as_ref()?
        .browse_endpoint_context_music_config
        .page_type
        .as_deref()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::common::Icon;
    use crate::models::endpoint::{
        BrowseEndpoint, BrowseEndpointContextMusicConfig, BrowseEndpointContextSupportedConfigs,
        WatchEndpoint, WatchEndpointMusicConfig, WatchEndpointMusicSupportedConfigs,
    };
    use crate::models::endpoint::{
        DefaultServiceEndpoint, FeedbackEndpoint, ToggledServiceEndpoint,
    };
    use crate::models::menu::{MenuServiceItemRenderer, ToggleMenuServiceRenderer};
    use crate::models::runs::{Run, Runs};

    fn column(texts: &[(&str, Option<NavigationEndpoint>)]) -> FlexColumn {
        FlexColumn {
            music_responsive_list_item_flex_column_renderer:
                crate::models::music_renderers::MusicResponsiveListItemFlexColumnRenderer {
                    text: Some(Runs {
                        runs: Some(
                            texts
                                .iter()
                                .map(|(text, endpoint)| Run {
                                    text: (*text).into(),
                                    navigation_endpoint: endpoint.clone(),
                                })
                                .collect(),
                        ),
                    }),
                },
        }
    }

    fn watch(video_id: &str, music_video_type: &str) -> NavigationEndpoint {
        NavigationEndpoint {
            watch_endpoint: Some(WatchEndpoint {
                video_id: Some(video_id.into()),
                watch_endpoint_music_supported_configs: Some(WatchEndpointMusicSupportedConfigs {
                    watch_endpoint_music_config: WatchEndpointMusicConfig {
                        music_video_type: music_video_type.into(),
                    },
                }),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn artist_browse(browse_id: &str) -> NavigationEndpoint {
        NavigationEndpoint {
            browse_endpoint: Some(BrowseEndpoint {
                browse_id: browse_id.into(),
                browse_endpoint_context_supported_configs: Some(
                    BrowseEndpointContextSupportedConfigs {
                        browse_endpoint_context_music_config: BrowseEndpointContextMusicConfig {
                            page_type: Some("MUSIC_PAGE_TYPE_ARTIST".into()),
                        },
                    },
                ),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    /// PageHelper.kt:33-50.
    #[test]
    fn extract_runs_by_type() {
        let columns = vec![column(&[
            ("Title", None),
            ("Artist A", Some(artist_browse("UCaaa"))),
            ("Song-ish", Some(watch("v1", "MUSIC_VIDEO_TYPE_ATV"))),
        ])];
        let artists = PageHelper::extract_runs(&columns, "MUSIC_PAGE_TYPE_ARTIST");
        assert_eq!(artists.len(), 1);
        assert_eq!(artists[0].text, "Artist A");
        let titles = PageHelper::extract_runs(&columns, "MUSIC_VIDEO");
        assert_eq!(titles.len(), 1);
        assert_eq!(titles[0].text, "Song-ish");
    }

    fn toggle(icon: &str, default_token: &str, toggled_token: &str) -> MenuItem {
        MenuItem {
            toggle_menu_service_item_renderer: Some(ToggleMenuServiceRenderer {
                default_icon: Icon {
                    icon_type: icon.into(),
                },
                default_service_endpoint: DefaultServiceEndpoint {
                    feedback_endpoint: Some(FeedbackEndpoint {
                        feedback_token: default_token.into(),
                    }),
                    subscribe_endpoint: None,
                },
                toggled_service_endpoint: Some(ToggledServiceEndpoint {
                    feedback_endpoint: Some(FeedbackEndpoint {
                        feedback_token: toggled_token.into(),
                    }),
                }),
            }),
            menu_navigation_item_renderer: None,
            menu_service_item_renderer: None,
        }
    }

    /// PageHelper.kt:52-86 — a toggle menu shows ONE library icon family
    /// (the current state); the add icon's toggled token is the remove
    /// action and vice versa.
    #[test]
    fn library_tokens_from_both_icon_families() {
        // unsaved song: the menu offers adding
        let add_items = vec![
            toggle("LIBRARY_ADD", "add-default", "add-toggled"),
            toggle("KEEP", "keep-default", "keep-toggled"),
        ];
        let tokens = PageHelper::extract_library_tokens_from_menu_items(Some(&add_items));
        assert_eq!(tokens.add_token.as_deref(), Some("add-default"));
        assert_eq!(tokens.remove_token.as_deref(), Some("add-toggled"));

        // saved song: the menu offers removing
        let saved_items = vec![toggle("LIBRARY_SAVED", "saved-default", "saved-toggled")];
        let tokens = PageHelper::extract_library_tokens_from_menu_items(Some(&saved_items));
        assert_eq!(tokens.remove_token.as_deref(), Some("saved-default"));
        assert_eq!(tokens.add_token.as_deref(), Some("saved-toggled"));

        assert_eq!(
            PageHelper::extract_library_tokens_from_menu_items(None),
            LibraryFeedbackTokens::default()
        );
    }

    /// PageHelper.kt:88-119.
    #[test]
    fn feedback_token_direction() {
        let add_menu = toggle("LIBRARY_ADD", "default", "toggled");
        let renderer = add_menu.toggle_menu_service_item_renderer.clone().unwrap();
        assert_eq!(
            PageHelper::extract_feedback_token(Some(&renderer), "LIBRARY_ADD"),
            Some("default".into())
        );
        assert_eq!(
            PageHelper::extract_feedback_token(Some(&renderer), "LIBRARY_REMOVE"),
            Some("toggled".into())
        );
        assert_eq!(
            PageHelper::extract_feedback_token(None, "LIBRARY_ADD"),
            None
        );
    }

    /// PageHelper.kt:18-31.
    #[test]
    fn icon_classification() {
        assert!(PageHelper::is_library_icon(Some("LIBRARY_ADD")));
        assert!(PageHelper::is_library_icon(Some("LIBRARY_SOMETHING_NEW")));
        assert!(!PageHelper::is_library_icon(Some("KEEP")));
        assert!(!PageHelper::is_library_icon(Some("SHARE")));
        assert!(!PageHelper::is_library_icon(None));
        let _ = MenuServiceItemRenderer {
            text: Runs::default(),
            icon: Icon {
                icon_type: "X".into(),
            },
            service_endpoint: NavigationEndpoint::default(),
        };
    }
}
