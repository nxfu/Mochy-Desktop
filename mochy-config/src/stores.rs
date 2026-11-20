//! The 22 Android SharedPreferences stores, mapped to registry sections.
//!
//! Upstream (pin bde01cb4) every `context.getSharedPreferences(name, ...)`:
//!
//! | store | owner |
//! |---|---|
//! | `audio_lab` | eq/AudioLabStore.kt:166 |
//! | `nanosonic_eq_profiles` | eq/data/EQProfileRepository.kt:33 |
//! | `song_tuning` | playback/SongTuning.kt:20 |
//! | `song_eq` | playback/SongTuning.kt:25 |
//! | `home_feed_cache` | viewmodels/HomeFeedCache.kt:14 |
//! | `listening_signals` | viewmodels/HomeRecommender.kt:389 |
//! | `source_modules` | modules/ModuleRegistry.kt:125 + SourceModules.kt:36 |
//! | `module_browser` | modules/browser/ModuleFeed.kt:61 |
//! | `module_looks` | modules/ModuleLooks.kt:47 |
//! | `app_icon` | utils/AppIcons.kt:136 |
//! | `not_recommended` | utils/NotRecommended.kt:11 |
//! | `update_settings` | core/updater/mochyupdater.kt |
//! | `settings` (updater) | core/updater/UpdateChannel.kt:18 |
//! | `tv_profiles` | link/tv/TvProfiles.kt:44 |
//! | `tv_control` | link/tv/TvControl.kt:25 |
//! | `mochy_link` | link/LinkWire.kt:88 |
//! | `mochy_link_tvs` | link/phone/PairedDevices.kt:51 |
//! | `playlist_added_here` | importer/PlaylistAddedHere.kt:11 |
//! | `recent_searches` | ui/screens/search/RecentSearches.kt:66 |
//! | `update_notifications` | ui/screens/update/UpdateNotifications.kt:29 |
//! | `transfer_state` | importer/transfer/TransferExport.kt:229 |
//! | `mochy_settings` | the "display" archive name (TransferArchive.kt:68) |
//!
//! (Plus the per-module dynamic stores `module_<id>` — ModuleRegistry.kt:546
//! — which the module system owns and which cannot be static rows.)
//!
//! The transfer archive carries 6 of these (TransferArchive.kt:58-69
//! `PrefsToCarry`): `audio_lab`, `nanosonic_eq_profiles`,
//! `listening_signals`, `not_recommended`, `source_modules` and `display`.

/// A named SharedPreferences store mapped as a config section.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PrefsStore {
    /// The Android store (file) name — identical for import compatibility.
    pub name: &'static str,
    /// What the store holds (upstream owner file).
    pub owner: &'static str,
    /// Whether an Android transfer archive carries this store.
    pub carried_by_archive: bool,
    /// The name the store travels under inside a transfer archive zip
    /// (`settings/prefs/<name>.json`); usually the same.
    pub archive_name: &'static str,
}

/// All 22 static stores (see the module docs). Ordered as listed upstream by
/// subsystem: audio lab, playback, home, modules, updater, link, importer,
/// search, notifications.
pub const PREFS_STORES: &[PrefsStore] = &[
    PrefsStore {
        name: "audio_lab",
        owner: "eq/AudioLabStore.kt",
        carried_by_archive: true,
        archive_name: "audio_lab",
    },
    PrefsStore {
        name: "nanosonic_eq_profiles",
        owner: "eq/data/EQProfileRepository.kt",
        carried_by_archive: true,
        archive_name: "nanosonic_eq_profiles",
    },
    PrefsStore {
        name: "song_tuning",
        owner: "playback/SongTuning.kt",
        carried_by_archive: false,
        archive_name: "song_tuning",
    },
    PrefsStore {
        name: "song_eq",
        owner: "playback/SongTuning.kt",
        carried_by_archive: false,
        archive_name: "song_eq",
    },
    PrefsStore {
        name: "home_feed_cache",
        owner: "viewmodels/HomeFeedCache.kt",
        carried_by_archive: false,
        archive_name: "home_feed_cache",
    },
    PrefsStore {
        name: "listening_signals",
        owner: "viewmodels/HomeRecommender.kt",
        carried_by_archive: true,
        archive_name: "listening_signals",
    },
    PrefsStore {
        name: "source_modules",
        owner: "modules/ModuleRegistry.kt",
        carried_by_archive: true,
        archive_name: "source_modules",
    },
    PrefsStore {
        name: "module_browser",
        owner: "modules/browser/ModuleFeed.kt",
        carried_by_archive: false,
        archive_name: "module_browser",
    },
    PrefsStore {
        name: "module_looks",
        owner: "modules/ModuleLooks.kt",
        carried_by_archive: false,
        archive_name: "module_looks",
    },
    PrefsStore {
        name: "app_icon",
        owner: "utils/AppIcons.kt",
        carried_by_archive: false,
        archive_name: "app_icon",
    },
    PrefsStore {
        name: "not_recommended",
        owner: "utils/NotRecommended.kt",
        carried_by_archive: true,
        archive_name: "not_recommended",
    },
    PrefsStore {
        name: "update_settings",
        owner: "core/updater/mochyupdater.kt",
        carried_by_archive: false,
        archive_name: "update_settings",
    },
    PrefsStore {
        name: "settings",
        owner: "core/updater/UpdateChannel.kt (updater channel, not the DataStore)",
        carried_by_archive: false,
        archive_name: "settings",
    },
    PrefsStore {
        name: "tv_profiles",
        owner: "link/tv/TvProfiles.kt",
        carried_by_archive: false,
        archive_name: "tv_profiles",
    },
    PrefsStore {
        name: "tv_control",
        owner: "link/tv/TvControl.kt",
        carried_by_archive: false,
        archive_name: "tv_control",
    },
    PrefsStore {
        name: "mochy_link",
        owner: "link/LinkWire.kt",
        carried_by_archive: false,
        archive_name: "mochy_link",
    },
    PrefsStore {
        name: "mochy_link_tvs",
        owner: "link/phone/PairedDevices.kt",
        carried_by_archive: false,
        archive_name: "mochy_link_tvs",
    },
    PrefsStore {
        name: "playlist_added_here",
        owner: "importer/PlaylistAddedHere.kt",
        carried_by_archive: false,
        archive_name: "playlist_added_here",
    },
    PrefsStore {
        name: "recent_searches",
        owner: "ui/screens/search/RecentSearches.kt",
        carried_by_archive: false,
        archive_name: "recent_searches",
    },
    PrefsStore {
        name: "update_notifications",
        owner: "ui/screens/update/UpdateNotifications.kt",
        carried_by_archive: false,
        archive_name: "update_notifications",
    },
    PrefsStore {
        name: "transfer_state",
        owner: "importer/transfer/TransferExport.kt",
        carried_by_archive: false,
        archive_name: "transfer_state",
    },
    PrefsStore {
        name: "mochy_settings",
        owner: "archive \"display\" store (TransferArchive.kt:68)",
        carried_by_archive: true,
        archive_name: "display",
    },
];

/// The 6 stores a transfer archive carries (TransferArchive.kt:58-69).
pub const TRANSFER_CARRIED_STORES: &[&str] = &[
    "audio_lab",
    "nanosonic_eq_profiles",
    "listening_signals",
    "not_recommended",
    "source_modules",
    "mochy_settings",
];

/// Resolve a store by its Android file name.
pub fn store_by_name(name: &str) -> Option<&'static PrefsStore> {
    PREFS_STORES.iter().find(|store| store.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exactly_22_stores() {
        assert_eq!(
            PREFS_STORES.len(),
            22,
            "verified correction: 22 SharedPreferences stores"
        );
        let mut names: Vec<_> = PREFS_STORES.iter().map(|s| s.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), 22, "store names must be unique");
    }

    #[test]
    fn archive_carries_six_stores() {
        assert_eq!(TRANSFER_CARRIED_STORES.len(), 6);
        for name in TRANSFER_CARRIED_STORES {
            let store = store_by_name(name).expect(name);
            assert!(store.carried_by_archive);
        }
        // the "display" archive name maps onto the mochy_settings store
        assert_eq!(
            store_by_name("mochy_settings").unwrap().archive_name,
            "display"
        );
    }
}
