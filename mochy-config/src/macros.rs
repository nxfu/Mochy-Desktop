//! The `settings!` macro (blueprint ch. 9.1 sketch): one row per Android
//! preference, generating —
//!
//! * the exhaustive [`crate::SettingId`] enum with Android key names,
//! * a defaults table, page metadata, titles + keywords (search index),
//! * typed accessors on [`crate::Settings`] and broadcast setters on
//!   [`crate::SettingsStore`],
//! * the JSON/TOML serializer input (`key -> SettingValue`).
//!
//! Row grammar (all fields required; the committed `registry.rs` is generated
//! so the strict grammar is never hand-written):
//!
//! ```ignore
//! settings! {
//!     PersistentQueue: "persistentQueue" => bool(true),
//!         page: Playback, title: "Persistent queue",
//!         keywords: "queue persist restore", hidden: false;
//!     AudioQuality: "audioQuality" => enum(AudioQuality::AUTO),
//!         page: AudioQuality, title: "Audio quality",
//!         keywords: "bitrate quality", hidden: false;
//! }
//! ```
//!
//! Key names are IDENTICAL to upstream `constants/PreferenceKeys.kt` so an
//! Android export imports 1:1. Values: `bool(..)`, `int(..)`, `long(..)`,
//! `float(..)`, `string("..")`, `enum(EnumTy::VARIANT)`.
//!
//! Setter names concatenate `set_` + row name through the `paste` crate
//! (`set_PersistentQueue`), mirroring the Kotlin `PersistentQueueKey`
//! read/write sites.

/// Expand the settings-registry rows. Not part of the public API surface —
/// used once by `registry.rs`.
#[macro_export]
macro_rules! settings {
    (
        $(
            $name:ident : $key:literal => $kind:ident ( $($v:tt)* ),
            page: $page:ident, title: $title:literal,
            keywords: $kw:literal, hidden: $hidden:literal ;
        )+
    ) => {
        /// Every static setting key in the registry, in upstream
        /// `PreferenceKeys.kt` declaration order. One variant per key row;
        /// the count must stay >= 395 (guarded by a test in this crate).
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum SettingId {
            $( $name ),+
        }

        impl SettingId {
            /// All ids in registry (upstream declaration) order.
            pub const ALL: &'static [SettingId] = &[ $( SettingId::$name ),+ ];

            /// Number of static keys in the registry.
            pub const COUNT: usize = SettingId::ALL.len();

            /// The Android key name, identical to `PreferenceKeys.kt`
            /// (import compatibility).
            pub const fn key_name(self) -> &'static str {
                match self { $( SettingId::$name => $key, )+ }
            }

            /// The settings page the key belongs to.
            pub const fn page(self) -> $crate::SettingsPage {
                match self { $( SettingId::$name => $crate::SettingsPage::$page, )+ }
            }

            /// English row title (the upstream settings index keeps search
            /// ids English and localizes at display time; we do the same).
            pub const fn title(self) -> &'static str {
                match self { $( SettingId::$name => $title, )+ }
            }

            /// Search keywords (space-separated lowercase).
            pub const fn keywords(self) -> &'static str {
                match self { $( SettingId::$name => $kw, )+ }
            }

            /// `true` for keys that are dead upstream (PORT_SPEC §4) or pure
            /// internal state — never shown as settings-UI rows.
            pub const fn hidden(self) -> bool {
                match self { $( SettingId::$name => $hidden, )+ }
            }

            /// The primitive kind of this key.
            pub const fn kind(self) -> $crate::SettingKind {
                match self {
                    $( SettingId::$name => $crate::__setting_kind!($kind ( $($v)* )), )+
                }
            }

            /// The upstream default (taken from the Android read site).
            pub fn default_value(self) -> $crate::SettingValue {
                match self {
                    $( SettingId::$name => $crate::__setting_default!($kind ( $($v)* )), )+
                }
            }

            /// Resolve a key by its Android key name.
            pub fn from_key_name(name: &str) -> Option<SettingId> {
                match name {
                    $( $key => Some(SettingId::$name), )+
                    _ => None,
                }
            }

            /// The typed accessor name (row ident), for diagnostics.
            pub const fn accessor_name(self) -> &'static str {
                match self { $( SettingId::$name => stringify!($name), )+ }
            }
        }

        impl $crate::Settings {
            /// Fill a fresh [`Settings`] with every registry default
            /// (the defaults table).
            pub fn with_defaults() -> $crate::Settings {
                let mut values = std::collections::HashMap::new();
                $(
                    values.insert(
                        $key.to_owned(),
                        $crate::__setting_default!($kind ( $($v)* )),
                    );
                )+
                $crate::Settings::from_values(values)
            }

            $(
                $crate::__setting_accessors!($name, $key, $kind ( $($v)* ));
            )+
        }

        impl $crate::SettingsStore {
            $(
                $crate::__setting_setter!($name, $key, $kind ( $($v)* ));
            )+
        }
    };
}

/// Map a row's kind to its [`crate::SettingKind`].
#[doc(hidden)]
#[macro_export]
macro_rules! __setting_kind {
    (bool ($($v:tt)*)) => {
        $crate::SettingKind::Bool
    };
    (int ($($v:tt)*)) => {
        $crate::SettingKind::Int
    };
    (long ($($v:tt)*)) => {
        $crate::SettingKind::Long
    };
    (float ($($v:tt)*)) => {
        $crate::SettingKind::Float
    };
    (string ($($v:tt)*)) => {
        $crate::SettingKind::Str
    };
    (enum ($ty:ident :: $variant:ident)) => {
        $crate::SettingKind::Enum
    };
}

/// Build the default [`crate::SettingValue`] for a row.
#[doc(hidden)]
#[macro_export]
macro_rules! __setting_default {
    (bool ($v:expr)) => {
        $crate::SettingValue::Bool($v)
    };
    (int ($v:expr)) => {
        $crate::SettingValue::Int($v)
    };
    (long ($v:expr)) => {
        $crate::SettingValue::Long($v)
    };
    (float ($v:expr)) => {
        $crate::SettingValue::Float($v)
    };
    (string ($v:expr)) => {
        $crate::SettingValue::Str($v.to_owned())
    };
    (enum ($ty:ident :: $variant:ident)) => {
        $crate::SettingValue::Str(<$ty>::$variant.variant_name().to_owned())
    };
}

/// Typed getter + snapshot setter for one row. The expansion wraps itself in
/// `paste!` because the setter name is concatenated.
#[doc(hidden)]
#[macro_export]
macro_rules! __setting_accessors {
    // bool
    ($name:ident, $key:literal, bool ($v:expr)) => {
        ::paste::paste! {
            #[allow(non_snake_case)]
            pub fn $name(&self) -> bool {
                self.bool_value($key).unwrap_or($v)
            }

            #[allow(non_snake_case)]
            pub fn [<set_ $name>](&mut self, value: bool) {
                self.values.insert($key.to_owned(), $crate::SettingValue::Bool(value));
            }
        }
    };
    // int
    ($name:ident, $key:literal, int ($v:expr)) => {
        ::paste::paste! {
            #[allow(non_snake_case)]
            pub fn $name(&self) -> i32 {
                self.i32_value($key).unwrap_or($v)
            }

            #[allow(non_snake_case)]
            pub fn [<set_ $name>](&mut self, value: i32) {
                self.values.insert($key.to_owned(), $crate::SettingValue::Int(value));
            }
        }
    };
    // long
    ($name:ident, $key:literal, long ($v:expr)) => {
        ::paste::paste! {
            #[allow(non_snake_case)]
            pub fn $name(&self) -> i64 {
                self.i64_value($key).unwrap_or($v)
            }

            #[allow(non_snake_case)]
            pub fn [<set_ $name>](&mut self, value: i64) {
                self.values.insert($key.to_owned(), $crate::SettingValue::Long(value));
            }
        }
    };
    // float
    ($name:ident, $key:literal, float ($v:expr)) => {
        ::paste::paste! {
            #[allow(non_snake_case)]
            pub fn $name(&self) -> f32 {
                self.f32_value($key).unwrap_or($v)
            }

            #[allow(non_snake_case)]
            pub fn [<set_ $name>](&mut self, value: f32) {
                self.values.insert($key.to_owned(), $crate::SettingValue::Float(value));
            }
        }
    };
    // string
    ($name:ident, $key:literal, string ($v:expr)) => {
        ::paste::paste! {
            #[allow(non_snake_case)]
            pub fn $name(&self) -> String {
                self.str_value($key).unwrap_or($v).to_owned()
            }

            #[allow(non_snake_case)]
            pub fn [<set_ $name>](&mut self, value: impl Into<String>) {
                self.values.insert($key.to_owned(), $crate::SettingValue::Str(value.into()));
            }
        }
    };
    // enum
    ($name:ident, $key:literal, enum ($ty:ident :: $variant:ident)) => {
        ::paste::paste! {
            #[allow(non_snake_case)]
            pub fn $name(&self) -> $ty {
                self.str_value($key)
                    .and_then(|stored| <$ty>::from_name(stored))
                    .unwrap_or($ty::$variant)
            }

            #[allow(non_snake_case)]
            pub fn [<set_ $name>](&mut self, value: $ty) {
                self.values.insert(
                    $key.to_owned(),
                    $crate::SettingValue::Str(value.variant_name().to_owned()),
                );
            }
        }
    };
}

/// Broadcast setter on [`crate::SettingsStore`] for one row. The expansion
/// wraps itself in `paste!` because the setter name is concatenated.
#[doc(hidden)]
#[macro_export]
macro_rules! __setting_setter {
    // bool / int / long / float
    ($name:ident, $key:literal, bool ($v:expr)) => {
        $crate::__setting_setter_plain!($name, bool);
    };
    ($name:ident, $key:literal, int ($v:expr)) => {
        $crate::__setting_setter_plain!($name, i32);
    };
    ($name:ident, $key:literal, long ($v:expr)) => {
        $crate::__setting_setter_plain!($name, i64);
    };
    ($name:ident, $key:literal, float ($v:expr)) => {
        $crate::__setting_setter_plain!($name, f32);
    };
    // string
    ($name:ident, $key:literal, string ($v:expr)) => {
        ::paste::paste! {
            #[allow(non_snake_case)]
            pub fn [<set_ $name>](&self, value: impl Into<String>) -> Result<(), $crate::ConfigError> {
                let value: String = value.into();
                self.set_by_id(SettingId::$name, $crate::SettingValue::Str(value))
            }
        }
    };
    // enum
    ($name:ident, $key:literal, enum ($ty:ident :: $variant:ident)) => {
        ::paste::paste! {
            #[allow(non_snake_case)]
            pub fn [<set_ $name>](&self, value: $ty) -> Result<(), $crate::ConfigError> {
                self.set_by_id(
                    SettingId::$name,
                    $crate::SettingValue::Str(value.variant_name().to_owned()),
                )
            }
        }
    };
}

/// Store setter for plain (non-string) kinds. The expansion wraps itself in
/// `paste!` because the setter name is concatenated.
#[doc(hidden)]
#[macro_export]
macro_rules! __setting_setter_plain {
    ($name:ident, $ty:ty) => {
        ::paste::paste! {
            #[allow(non_snake_case)]
            pub fn [<set_ $name>](&self, value: $ty) -> Result<(), $crate::ConfigError> {
                self.set_by_id(SettingId::$name, $crate::SettingValue::from(value))
            }
        }
    };
}
