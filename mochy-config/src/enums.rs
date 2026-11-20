//! Registry enum types — one Rust enum per Kotlin enum whose variants are
//! stored in the Android DataStore as `Enum.name` strings (read back through
//! `toEnum(default)`, utils/ContextExt `toEnum`: unknown stored values fall
//! back to the default, exactly like [`RegistryEnum::from_name_or_default`]).
//!
//! Every list mirrors the pinned upstream declaration order
//! (constants/PreferenceKeys.kt and the component files that own the
//! seek-bar / button / swipe / larp enums).

/// A macro-generated registry enum: `VARIANTS` in declaration order, matching
/// on the exact Kotlin variant name.
macro_rules! registry_enum {
    ($(#[$doc:meta])* $name:ident { $($variant:ident),+ $(,)? }) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        #[allow(non_camel_case_types)] // Kotlin Enum.name parity (e.g. CREATE_DATE)
        pub enum $name { $($variant),+ }

        impl $name {
            /// Kotlin variant names in declaration order.
            pub const VARIANTS: &'static [&'static str] = &[$(stringify!($variant)),+];

            /// The stored (Kotlin `Enum.name`) form.
            pub const fn variant_name(self) -> &'static str {
                match self { $( $name::$variant => stringify!($variant) ),+ }
            }

            /// Parse a stored value; `None` when the name is not a variant
            /// (upstream `toEnum` would fall back to the default).
            pub fn from_name(name: &str) -> Option<Self> {
                match name { $( stringify!($variant) => Some($name::$variant) ),+, _ => None }
            }

            /// Parse a stored value with an upstream-style default fallback.
            pub fn from_name_or_default(name: &str, default: Self) -> Self {
                Self::from_name(name).unwrap_or(default)
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.variant_name())
            }
        }
    };
}

// --- constants/PreferenceKeys.kt enums ---------------------------------------

registry_enum! {
    /// PreferenceKeys.kt:54 (`DarkModeKey` is stored as this enum's names).
    DarkMode { AUTO, ON, OFF }
}

registry_enum! {
    /// ui/screens/Screens.kt:50 (`DefaultOpenTabKey`).
    NavigationTab { HOME, SEARCH }
}

registry_enum! {
    /// PreferenceKeys.kt:591 (`GridItemsSizeKey`).
    GridItemSize { BIG, SMALL }
}

registry_enum! {
    /// PreferenceKeys.kt:96 (`SliderStyleKey`; dead key, PORT_SPEC §4).
    SliderStyle { DEFAULT, WAVY, SLIM }
}

registry_enum! {
    /// ui/component/SongSwipeBox.kt:29 (`SwipeRightActionKey` /
    /// `SwipeLeftActionKey`). Variant names are UpperCamelCase upstream.
    SongSwipeAction { Library, Queue, Download, PlayNext, AddToPlaylist, Details }
}

registry_enum! {
    /// PreferenceKeys.kt:57 (`HomeStyleKey`).
    HomeStyle { MOCHYY, LISTEN_NOW, HOP_IN }
}

registry_enum! {
    /// PreferenceKeys.kt:66 (`PlaylistStyleKey`).
    PlaylistStyle { FLOW, CLASSIC, MINIMAL }
}

registry_enum! {
    /// ui/player/LarpMode.kt:18 (`LarpLevelKey`).
    LarpLevel { SKIBIDI, SIXTY_SEVEN, BRAINROT, MAXXING }
}

registry_enum! {
    /// utils/AppProxy.kt (`ProxyTypeKey` stores `Proxy.Type.name`).
    ProxyType { HTTP, SOCKS }
}

registry_enum! {
    /// PreferenceKeys.kt:471 (`VideoQualityKey`).
    VideoQuality { AUTO, Q2160, Q1440, Q1080, Q720, Q480, Q360, Q240, Q144 }
}

registry_enum! {
    /// PreferenceKeys.kt:174 (`AudioQualityKey`).
    AudioQuality { AUTO, LOW, MEDIUM, HIGH, LOSSLESS, HI_RES_LOSSLESS, DOLBY_ATMOS }
}

registry_enum! {
    /// providers/innertube/models (`IpVersionKey`).
    IpVersion { AUTO, IPV4, IPV6 }
}

registry_enum! {
    /// PreferenceKeys.kt:169 (`StreamBackendKey`).
    StreamBackend { INNERTUBEX, CLASSIC }
}

registry_enum! {
    /// PreferenceKeys.kt:196 (`DownloadQualityKey`).
    DownloadQuality { HIGH, MEDIUM, LOW }
}

registry_enum! {
    /// PreferenceKeys.kt:216 (`AudioPassModeKey`); desktop default SONG_RATE
    /// (upstream `AudioPassMode.Default` is SDK-34 SONG_RATE).
    AudioPassMode { SONG_RATE, BIT_PERFECT, DIRECT_USB, EXCLUSIVE_USB, EXCLUSIVE_DAC }
}

registry_enum! {
    /// PreferenceKeys.kt:268 (`CrossmixModeKey`).
    CrossmixMode { AUTO_CROSSFADE, OFF, MANUAL_CROSSFADE, CROSSMIX }
}

registry_enum! {
    /// PreferenceKeys.kt:299 (`CrossmixLengthKey`).
    CrossmixLength { SHORT, MEDIUM, LONG }
}

registry_enum! {
    /// PreferenceKeys.kt:306 (`CrossmixTempoMatchKey`).
    CrossmixTempoMatch { OFF, GENTLE, FULL }
}

registry_enum! {
    /// PreferenceKeys.kt:332 (`TransportEffectKey`).
    TransportEffect { OFF, ECHO, REWIND, TAPE_STOP, FAR_AWAY, REVERB, FILTER }
}

registry_enum! {
    /// PreferenceKeys.kt:524 (`SongSortTypeKey`).
    SongSortType { CREATE_DATE, NAME, ARTIST, PLAY_TIME }
}

registry_enum! {
    /// PreferenceKeys.kt:531 (`PlaylistSongSortTypeKey`).
    PlaylistSongSortType { CUSTOM, CREATE_DATE, NAME, ARTIST, PLAY_TIME }
}

registry_enum! {
    /// PreferenceKeys.kt:540 (`LikedSongSortTypeKey`).
    LikedSongSortType { CREATE_DATE, IMPORT_ORDER, NAME, ARTIST, PLAY_TIME }
}

registry_enum! {
    /// PreferenceKeys.kt:548 (`AutoPlaylistSongSortTypeKey`).
    AutoPlaylistSongSortType { CREATE_DATE, NAME, ARTIST, PLAY_TIME }
}

registry_enum! {
    /// PreferenceKeys.kt:555 (`ArtistSortTypeKey`).
    ArtistSortType { CREATE_DATE, NAME, SONG_COUNT, PLAY_TIME }
}

registry_enum! {
    /// PreferenceKeys.kt:562 (`ArtistSongSortTypeKey`).
    ArtistSongSortType { CREATE_DATE, NAME, PLAY_TIME }
}

registry_enum! {
    /// PreferenceKeys.kt:568 (`AlbumSortTypeKey`).
    AlbumSortType { CREATE_DATE, NAME, ARTIST, YEAR, SONG_COUNT, LENGTH, PLAY_TIME }
}

registry_enum! {
    /// PreferenceKeys.kt:578 (`PlaylistSortTypeKey`; also
    /// `AddToPlaylistSortTypeKey`).
    PlaylistSortType { CREATE_DATE, NAME, SONG_COUNT, LAST_UPDATED }
}

registry_enum! {
    /// PreferenceKeys.kt:585 (`MixSortTypeKey`).
    MixSortType { CREATE_DATE, NAME, LAST_UPDATED }
}

registry_enum! {
    /// PreferenceKeys.kt:506 (`SongFilterKey`).
    SongFilter { LIBRARY, LIKED, DOWNLOADED, UPLOADED }
}

registry_enum! {
    /// PreferenceKeys.kt:513 (`ArtistFilterKey`).
    ArtistFilter { LIBRARY, LIKED }
}

registry_enum! {
    /// PreferenceKeys.kt:518 (`AlbumFilterKey`).
    AlbumFilter { LIBRARY, LIKED, UPLOADED }
}

registry_enum! {
    /// PreferenceKeys.kt:638 (`QuickPicksKey`, stored under "discover").
    QuickPicks { QUICK_PICKS, LAST_LISTEN }
}

registry_enum! {
    /// PreferenceKeys.kt:643 (`PreferredLyricsProviderKey`).
    PreferredLyricsProvider { LRCLIB, KUGOU, BETTER_LYRICS, SIMPMUSIC, YOULYPLUS, PAXSENIX }
}

registry_enum! {
    /// PreferenceKeys.kt:691 (`PlayerStyleKey`), 11 styles.
    PlayerStyle { FLAGSHIP, CLASSIC, TURNTABLE, VIDEO, IPOD, POSTER, TIMELINE, LYRICS, BEDSIDE, BEDSIDE_TILES, MODULE }
}

registry_enum! {
    /// PreferenceKeys.kt:767 (`IpodThemeKey`).
    IpodTheme { SILVER, BLACK, U2, TRUE_DARK }
}

registry_enum! {
    /// PreferenceKeys.kt:772 (`IpodWheelColorKey`).
    IpodWheelColor { THEME, WHITE, BLACK, RED }
}

registry_enum! {
    /// PreferenceKeys.kt:786 (`BedsideClockFaceKey`).
    BedsideClockFace { BIG, STACKED, ANALOG, COVER }
}

registry_enum! {
    /// PreferenceKeys.kt:791 (`BedsideNightTintKey`).
    BedsideNightTint { OFF, ORANGE, COVER }
}

registry_enum! {
    /// PreferenceKeys.kt:815 (`PlayerEndTimestampKey`).
    PlayerEndTimestamp { END_TIME, TIME_LEFT }
}

registry_enum! {
    /// PreferenceKeys.kt:826 (`PlayerBottomBarStyleKey`).
    PlayerBottomBarStyle { STACK, VOLUME }
}

registry_enum! {
    /// PreferenceKeys.kt:842 (`FlagshipTransportLayoutKey`).
    FlagshipTransportLayout { THREE, TEN_SECONDS, QUEUE_REPEAT }
}

registry_enum! {
    /// PreferenceKeys.kt:860 (`DiscLabelContentKey`).
    DiscLabelContent { CUSTOM, MUSIC_NAME, ARTIST_NAME, ALBUM_NAME, MUSIC_AND_ARTIST, NONE }
}

registry_enum! {
    /// ui/component/SeekBarStyles.kt:37 (`SeekBarStyleKey`).
    SeekBarStyle { DEFAULT, M3_EXPRESSIVE, M3_EXPRESSIVE_LINE, PLAIN }
}

registry_enum! {
    /// ui/component/ButtonStyles.kt:57 (`ButtonStyleKey`).
    ButtonStyle { DEFAULT, M3_EXPRESSIVE, M3_EXPRESSIVE_PLAIN, M3_TONAL, LEGACY }
}

registry_enum! {
    /// PreferenceKeys.kt:672 (`PlayerCoverStyleKey`).
    PlayerCoverStyle { EDGE_TO_EDGE, SQUARED, DISC_SMALL_FULL, DISC_SMALL_LABEL, DISC_BIG_FULL, DISC_BIG_LABEL, DISC_ALBUM, TURNTABLE }
}

registry_enum! {
    /// PreferenceKeys.kt:896 (`PlayerTitleSlotKey` / `PlayerSubtitleSlotKey`).
    PlayerTextContent { MUSIC_NAME, ALBUM_NAME, ARTIST_NAME, COMBINED }
}

registry_enum! {
    /// PreferenceKeys.kt:658 (`PlayerBackgroundStyleKey`,
    /// `MiniPlayerBackgroundStyleKey`, `FlagshipBackgroundStyleKey`).
    PlayerBackgroundStyle { COVER_GRADIENT, SOLID, DARK_GRADIENT, APPLE_MUSIC, AMBIENT }
}

registry_enum! {
    /// ui/screens/Screens.kt:58 (`LyricsTextPositionKey`).
    LyricsPosition { LEFT, CENTER, RIGHT }
}

registry_enum! {
    /// PreferenceKeys.kt:970 (`LyricsMotionStyleKey`).
    LyricsMotionStyle { MOTION, LEGACY, BORING, PLAIN }
}

registry_enum! {
    /// PreferenceKeys.kt:993 (`LyricsAnimationStyleKey`; legacy renderers).
    LyricsAnimationStyle { NONE, FADE, GLOW, SLIDE, KARAOKE, APPLE, APPLE_V2, MOCHY_1, LYRICS_V2, METRO_LYRICS }
}

registry_enum! {
    /// PreferenceKeys.kt:1039 (`SearchSourceKey`).
    SearchSource { LOCAL, ONLINE }
}

registry_enum! {
    /// PreferenceKeys.kt:1025 (`CanvasOrderKey`).
    CanvasOrder { APPLE_TIDAL_SPOTIFY, APPLE_SPOTIFY_TIDAL, TIDAL_APPLE_SPOTIFY, TIDAL_SPOTIFY_APPLE, SPOTIFY_APPLE_TIDAL, SPOTIFY_TIDAL_APPLE }
}

registry_enum! {
    /// constants/HapticSensitivity.kt (`MusicHapticsSensitivityKey`).
    HapticSensitivity { LOW, MEDIUM, HIGH }
}

registry_enum! {
    /// constants/HapticFocus.kt (`HapticFocusKey`).
    HapticFocus { BALANCE, VOCAL, VIBE, BASS }
}

registry_enum! {
    /// PreferenceKeys.kt:1347 (`BlurQualityKey`). Desktop default is HIGH —
    /// upstream picks HIGH on strong SOCs (GlassKit.kt `DefaultBlurQuality`)
    /// and a desktop GPU is always in that class.
    BlurQuality { LOW, HIGH }
}

registry_enum! {
    /// PreferenceKeys.kt:1352 (`ChromeGlassStyleKey`).
    ChromeGlassStyle { FROSTED, MODULE }
}

registry_enum! {
    /// PreferenceKeys.kt:652 (`PlayerButtonsStyleKey`).
    PlayerButtonsStyle { DEFAULT, PRIMARY, TERTIARY }
}

impl PlayerStyle {
    /// The styles a stored name maps onto when it was saved by an older
    /// version (PreferenceKeys.kt:734 `PlayerStyle.fromStored`).
    pub fn from_stored(name: &str) -> Option<PlayerStyle> {
        match name {
            "SIMPLE" | "SIMPLE_COMPACT" => Some(PlayerStyle::TIMELINE),
            "LYRICS_ABSTRACT" => Some(PlayerStyle::LYRICS),
            "DEFAULT" => Some(PlayerStyle::FLAGSHIP),
            _ => PlayerStyle::from_name(name),
        }
    }
}

/// Trait implemented by every [`registry_enum!`] type so the `settings!`
/// macro can generate enum accessors uniformly.
pub trait RegistryEnum: Copy {
    /// Kotlin variant names in declaration order.
    const VARIANTS: &'static [&'static str];
    /// The stored (Kotlin `Enum.name`) form.
    fn variant_name(self) -> &'static str;
    /// Parse a stored value; `None` when the name is not a variant.
    fn from_name(name: &str) -> Option<Self>
    where
        Self: Sized;
}

macro_rules! impl_registry_enum_trait {
    ($($name:ident),+ $(,)?) => { $(
        impl RegistryEnum for $name {
            const VARIANTS: &'static [&'static str] = <$name>::VARIANTS;
            fn variant_name(self) -> &'static str { <$name>::variant_name(self) }
            fn from_name(value: &str) -> Option<Self> { <$name>::from_name(value) }
        }
    )+ };
}

impl_registry_enum_trait!(
    DarkMode,
    NavigationTab,
    GridItemSize,
    SliderStyle,
    SongSwipeAction,
    HomeStyle,
    PlaylistStyle,
    LarpLevel,
    ProxyType,
    VideoQuality,
    AudioQuality,
    IpVersion,
    StreamBackend,
    DownloadQuality,
    AudioPassMode,
    CrossmixMode,
    CrossmixLength,
    CrossmixTempoMatch,
    TransportEffect,
    SongSortType,
    PlaylistSongSortType,
    LikedSongSortType,
    AutoPlaylistSongSortType,
    ArtistSortType,
    ArtistSongSortType,
    AlbumSortType,
    PlaylistSortType,
    MixSortType,
    SongFilter,
    ArtistFilter,
    AlbumFilter,
    QuickPicks,
    PreferredLyricsProvider,
    PlayerStyle,
    IpodTheme,
    IpodWheelColor,
    BedsideClockFace,
    BedsideNightTint,
    PlayerEndTimestamp,
    PlayerBottomBarStyle,
    FlagshipTransportLayout,
    DiscLabelContent,
    SeekBarStyle,
    ButtonStyle,
    PlayerCoverStyle,
    PlayerTextContent,
    PlayerBackgroundStyle,
    LyricsPosition,
    LyricsMotionStyle,
    LyricsAnimationStyle,
    SearchSource,
    CanvasOrder,
    HapticSensitivity,
    HapticFocus,
    BlurQuality,
    ChromeGlassStyle,
    PlayerButtonsStyle,
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enum_names_round_trip() {
        for name in PlayerStyle::VARIANTS {
            let parsed = PlayerStyle::from_name(name).expect(name);
            assert_eq!(parsed.variant_name(), *name);
        }
    }

    #[test]
    fn unknown_name_falls_back() {
        assert_eq!(
            AudioQuality::from_name_or_default("bogus", AudioQuality::HIGH),
            AudioQuality::HIGH
        );
        assert_eq!(
            AudioQuality::from_name("HI_RES_LOSSLESS"),
            Some(AudioQuality::HI_RES_LOSSLESS)
        );
    }

    #[test]
    fn legacy_player_style_names_map() {
        // PreferenceKeys.kt:734
        assert_eq!(
            PlayerStyle::from_stored("SIMPLE"),
            Some(PlayerStyle::TIMELINE)
        );
        assert_eq!(
            PlayerStyle::from_stored("DEFAULT"),
            Some(PlayerStyle::FLAGSHIP)
        );
        assert_eq!(
            PlayerStyle::from_stored("LYRICS_ABSTRACT"),
            Some(PlayerStyle::LYRICS)
        );
        assert_eq!(PlayerStyle::from_stored("IPOD"), Some(PlayerStyle::IPOD));
    }
}
