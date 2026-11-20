//! # mochy-config — settings registry
//!
//! Settings registry (~400 keys with defaults + search metadata), audio-lab
//! store, atomic persistence, live watchers.
//!
//! Upstream absorption (Android, pin `bde01cb4`):
//! - `constants/PreferenceKeys.kt` (1,399 lines; 398 static key declarations +
//!   the per-style dynamic key factory) — every key name is kept IDENTICAL for
//!   Android import compatibility (see `registry.rs` and `crate::settings!`).
//! - `utils/DataStore.kt` — the `PreferencesCache` read-through snapshot model
//!   becomes [`SettingsStore`]'s lock + snapshot + broadcast design.
//! - `eq/AudioLabStore.kt` — the `audio_lab` SharedPreferences store becomes
//!   the typed [`audio_lab`] section (JSON-blob per key, upstream key names).
//! - 22 SharedPreferences stores (see [`stores`]).
//! - `importer/transfer/TransferArchive.kt` — the typed-JSON settings format
//!   `{ "key": { "t": "b", "v": true } }` and the login-key skip list are
//!   reproduced in [`android_import`] for Android archive import.
//!
//! Layering (PORT_SPEC §3.1): this is a domain crate — no GPUI, no tokio, no
//! platform-specific code in the public API.

mod android_import;
mod audio_lab;
mod enums;
mod macros;
mod registry;
mod store;
mod stores;

pub use android_import::{
    LOGIN_KEYS, LoginKeyBehavior, RawPreferences, encode_preferences_pb, import_archive_prefs_json,
    import_archive_settings_json, parse_preferences_pb,
};
pub use audio_lab::{
    AudioLab, AutoEqCorrection, AutoEqTuning, EQ_BAND_FREQUENCIES, EightDSettings, EqPreset,
    EqSettings, LoudnessSettings, MAX_SPEED, MIN_SPEED, NormalizeSettings, ParametricEqBand,
    PlaybackSettings, ReverbSettings, SPEED_PRESETS, SpatialSettings, StageSettings,
    VolumeSettings,
};
pub use enums::*;
pub use registry::*;
pub use store::{
    ConfigError, PERSISTED_FORMAT, PERSISTED_VERSION, Settings, SettingsStore, WatchError,
    WatchGuard,
};
pub use stores::{PREFS_STORES, PrefsStore, TRANSFER_CARRIED_STORES, store_by_name};

/// The pages of the settings UI. Derived from the upstream settings index
/// (ui/screens/settings/MochySettingsScreen.kt, 100 SettingsEntry rows) and
/// the settings routes registered in `ui/screens/settings/SettingsPane.kt`
/// (`customisation_settings`, `general_settings`, `audio_lab`,
/// `crossmix_settings`, `equalizer`, `audio_quality`, `audio_pass`,
/// `audio_fx`, `modules`, `storage_settings`, `experimental_settings`, ...)
/// plus the pages named in the port spec (Playback, Audio, Now-playing,
/// Customization, Lyrics, Content, Account, Audio-quality, Video, Library,
/// Discord, Scrobble, Proxy, Storage, ...).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SettingsPage {
    /// Appearance / theme keys (`darkMode`, `pureBlack`, blur, ...).
    Appearance,
    /// "App behaviour" — general keys (language, offline, data saver, ...).
    General,
    /// Playback behavior (queue persistence, silence skip, ...).
    Playback,
    /// Transitions: crossfade / crossmix / transport fades / skip blend.
    Transitions,
    /// Audio-quality picks (stream + download quality).
    AudioQuality,
    /// Audio Pass: USB DAC / exclusive output routing.
    AudioPass,
    /// Audio Lab FX: normalization, spatial, speed, volume curve.
    AudioFx,
    /// Now-playing / player surface keys.
    NowPlaying,
    /// Customisation: player styles, gestures, blur, animation.
    Customization,
    /// Lyrics providers, romanization, translation.
    Lyrics,
    /// Content filters and page content toggles.
    Content,
    /// Signed-in account state.
    Account,
    /// Video playback keys.
    Video,
    /// Library sorts, filters and pins.
    Library,
    /// Discord presence (19 presence keys upstream).
    Discord,
    /// Last.fm / Libre.fm scrobbling.
    Scrobble,
    /// Proxy configuration.
    Proxy,
    /// Storage and cache limits.
    Storage,
    /// Experimental settings page.
    Experimental,
    /// Source modules (Monochrome, Amazon Music, instances).
    Modules,
    /// YouTube sync / import & sync.
    Sync,
    /// Internal state markers never shown as settings rows.
    Internal,
}

impl SettingsPage {
    /// All pages in settings-UI order.
    pub const ALL: &'static [SettingsPage] = &[
        SettingsPage::Appearance,
        SettingsPage::General,
        SettingsPage::Playback,
        SettingsPage::Transitions,
        SettingsPage::AudioQuality,
        SettingsPage::AudioPass,
        SettingsPage::AudioFx,
        SettingsPage::NowPlaying,
        SettingsPage::Customization,
        SettingsPage::Lyrics,
        SettingsPage::Content,
        SettingsPage::Account,
        SettingsPage::Video,
        SettingsPage::Library,
        SettingsPage::Discord,
        SettingsPage::Scrobble,
        SettingsPage::Proxy,
        SettingsPage::Storage,
        SettingsPage::Experimental,
        SettingsPage::Modules,
        SettingsPage::Sync,
        SettingsPage::Internal,
    ];

    /// Human page title (English, matching the upstream settings index).
    pub const fn title(self) -> &'static str {
        match self {
            SettingsPage::Appearance => "Appearance",
            SettingsPage::General => "App behaviour",
            SettingsPage::Playback => "Playback",
            SettingsPage::Transitions => "Transitions",
            SettingsPage::AudioQuality => "Audio quality",
            SettingsPage::AudioPass => "Audio Pass",
            SettingsPage::AudioFx => "Audio Lab",
            SettingsPage::NowPlaying => "Now playing",
            SettingsPage::Customization => "Customisation",
            SettingsPage::Lyrics => "Lyrics",
            SettingsPage::Content => "Content",
            SettingsPage::Account => "Account",
            SettingsPage::Video => "Video",
            SettingsPage::Library => "Library",
            SettingsPage::Discord => "Discord",
            SettingsPage::Scrobble => "Scrobbling",
            SettingsPage::Proxy => "Proxy",
            SettingsPage::Storage => "Storage",
            SettingsPage::Experimental => "Experimental",
            SettingsPage::Modules => "Modules",
            SettingsPage::Sync => "Import & sync",
            SettingsPage::Internal => "Internal",
        }
    }
}

/// The primitive kind of a registry value. Mirrors the androidx DataStore
/// key constructors used by `PreferenceKeys.kt`
/// (`booleanPreferencesKey` / `intPreferencesKey` / `floatPreferencesKey` /
/// `longPreferencesKey` / `stringPreferencesKey`), plus the enum form the
/// upstream reads through `toEnum(default)` (stored as the Kotlin enum
/// variant name) and the string-set form the transfer archive can carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SettingKind {
    Bool,
    Int,
    Long,
    Float,
    Str,
    /// Enum-typed key stored as the Kotlin variant name (e.g. `"AUTO"`).
    Enum,
    /// `Set<String>` — only produced by archive import; no static key uses it.
    StringSet,
}

impl SettingKind {
    /// Type name as written in the registry grammar.
    pub const fn as_str(self) -> &'static str {
        match self {
            SettingKind::Bool => "bool",
            SettingKind::Int => "int",
            SettingKind::Long => "long",
            SettingKind::Float => "float",
            SettingKind::Str => "string",
            SettingKind::Enum => "enum",
            SettingKind::StringSet => "string_set",
        }
    }
}

/// A typed setting value. The shape follows the upstream transfer-archive
/// typed-JSON encoding (TransferArchive.kt `typedValue`): booleans, 32-bit
/// ints, 64-bit longs, floats, strings, string sets (and bytes on the wire,
/// which the desktop store keeps opaque).
#[derive(Debug, Clone, PartialEq)]
pub enum SettingValue {
    Bool(bool),
    Int(i32),
    Long(i64),
    Float(f32),
    Str(String),
    StringSet(Vec<String>),
    /// Opaque bytes (transfer archive only).
    Bytes(Vec<u8>),
}

impl SettingValue {
    /// The archive type tag (`"t"` field) for this value
    /// (TransferArchive.kt:160-173).
    pub const fn type_tag(&self) -> &'static str {
        match self {
            SettingValue::Bool(_) => "b",
            SettingValue::Int(_) => "i",
            SettingValue::Long(_) => "l",
            SettingValue::Float(_) => "f",
            SettingValue::Str(_) => "s",
            SettingValue::StringSet(_) => "ss",
            SettingValue::Bytes(_) => "bytes",
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match *self {
            SettingValue::Bool(v) => Some(v),
            _ => None,
        }
    }

    pub fn as_i32(&self) -> Option<i32> {
        match *self {
            SettingValue::Int(v) => Some(v),
            SettingValue::Long(v) => i32::try_from(v).ok(),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match *self {
            SettingValue::Long(v) => Some(v),
            SettingValue::Int(v) => Some(v as i64),
            _ => None,
        }
    }

    pub fn as_f32(&self) -> Option<f32> {
        match *self {
            SettingValue::Float(v) => Some(v),
            SettingValue::Int(v) => Some(v as f32),
            SettingValue::Long(v) => Some(v as f32),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            SettingValue::Str(v) => Some(v),
            _ => None,
        }
    }
}

impl From<bool> for SettingValue {
    fn from(v: bool) -> Self {
        SettingValue::Bool(v)
    }
}
impl From<i32> for SettingValue {
    fn from(v: i32) -> Self {
        SettingValue::Int(v)
    }
}
impl From<i64> for SettingValue {
    fn from(v: i64) -> Self {
        SettingValue::Long(v)
    }
}
impl From<f32> for SettingValue {
    fn from(v: f32) -> Self {
        SettingValue::Float(v)
    }
}
impl From<&str> for SettingValue {
    fn from(v: &str) -> Self {
        SettingValue::Str(v.to_owned())
    }
}
impl From<String> for SettingValue {
    fn from(v: String) -> Self {
        SettingValue::Str(v)
    }
}
impl From<Vec<String>> for SettingValue {
    fn from(v: Vec<String>) -> Self {
        SettingValue::StringSet(v)
    }
}

/// The per-style dynamic preference-key factory
/// (PreferenceKeys.kt:808, `playerStyleRotatedKey`):
/// `playerStyleRotated_${style.name}` for each `PlayerStyle`.
///
/// Documented, not enumerated: the number of rows depends on the installed
/// player styles, so it cannot be a static registry row. Use
/// [`SettingId::from_key_name`] to resolve a produced key after import.
pub fn per_style_key(style: PlayerStyle, setting: PerStyleSetting) -> String {
    match setting {
        PerStyleSetting::Rotated => format!("playerStyleRotated_{}", style.variant_name()),
    }
}

/// The settings that have one key per player style (upstream: only
/// "rotated", but the factory shape allows more later).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PerStyleSetting {
    /// Whether the style's rotate button turned it sideways.
    Rotated,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn per_style_key_matches_upstream_factory() {
        // upstream: booleanPreferencesKey("playerStyleRotated_${style.name}")
        assert_eq!(
            per_style_key(PlayerStyle::TURNTABLE, PerStyleSetting::Rotated),
            "playerStyleRotated_TURNTABLE"
        );
        assert_eq!(
            per_style_key(PlayerStyle::BEDSIDE_TILES, PerStyleSetting::Rotated),
            "playerStyleRotated_BEDSIDE_TILES"
        );
    }

    /// Defaults audit (2026-11-20, master audit of Phases 01-03): pins the
    /// read-site defaults that were found wrong in the first pass — every
    /// list-sort "descending" toggle defaults TRUE upstream, ytmSync and
    /// showArtistVideo default TRUE, and the theme color default is
    /// `Color(0xFF8E8E8E).toArgb()` (MainActivity.kt:570 + Theme.kt:27).
    #[test]
    fn audited_defaults_match_upstream_read_sites() {
        let s = Settings::with_defaults();
        for id in [
            SettingId::SongSortDescending,
            SettingId::PlaylistSongSortDescending,
            SettingId::LikedSongSortDescending,
            SettingId::ArtistSortDescending,
            SettingId::AlbumSortDescending,
            SettingId::PlaylistSortDescending,
            SettingId::AddToPlaylistSortDescending,
            SettingId::ArtistSongSortDescending,
            SettingId::ShowArtistVideo,
            SettingId::YtmSync,
        ] {
            assert_eq!(
                s.bool_value(id.key_name()),
                Some(true),
                "{} must default true (upstream read sites)",
                id.accessor_name()
            );
        }
        // Theme.kt:27 Color(0xFF8E8E8E).toArgb() as signed i32.
        assert_eq!(s.SelectedThemeColor(), -7_434_610);
        // Dead key with zero upstream readers stays out of the settings UI.
        assert!(SettingId::AutoPlaylistSongSortDescending.hidden());
    }

    #[test]
    fn pages_are_unique_and_titled() {
        let all = SettingsPage::ALL;
        for (i, page) in all.iter().enumerate() {
            assert!(!page.title().is_empty());
            for other in &all[i + 1..] {
                assert_ne!(page, other, "duplicate page");
            }
        }
    }
}
