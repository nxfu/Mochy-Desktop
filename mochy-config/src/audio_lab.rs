//! The audio-lab section — everything `eq/AudioLabStore.kt` persists in the
//! `audio_lab` SharedPreferences store, with identical key names
//! (`eq`, `reverb`, `loudness`, `spatial`, `eight_d`, `playback`, `volume`,
//! `normalize`, `stage`, `presets`, `migrated_legacy_eq`).
//!
//! Upstream stores each value as a JSON blob of the corresponding data class
//! (`Json { ignoreUnknownKeys = true; encodeDefaults = true }`); a parse
//! failure at load falls back to the class default (`AudioLabStore.load` +
//! `?: EqSettings()` etc.). This module reproduces the data classes, their
//! defaults and the load-with-fallback behavior; the DSP parameter plumbing
//! belongs to Phase 19/20 (`mochy-audio`).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::{SettingValue, Settings, SettingsStore};

/// Sonic speed range (AudioLabStore.kt:47-48): 0.1x .. 8x on a log slider.
pub const MIN_SPEED: f32 = 0.1;
pub const MAX_SPEED: f32 = 8.0;

/// Speed presets (ui/screens/audiolab/AudioFxScreen.kt:378-380).
pub const SPEED_PRESETS: &[(&str, f32, bool)] = &[
    // (id, speed, with_reverb)
    ("normal", 1.0, false),
    ("slowed", 0.85, false),
    ("slowed_reverb", 0.85, true),
    ("nightcore", 1.25, false),
];

/// A headphone correction picked from the AutoEq database
/// (AudioLabStore.kt:52-61).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct AutoEqCorrection {
    pub model: String,
    pub source: String,
    #[serde(default)]
    pub rig: Option<String>,
    pub path: String,
    #[serde(default = "default_headphone_form")]
    pub form: String, // HeadphoneForm name; OverEar default
    #[serde(default)]
    pub tuning: AutoEqTuning,
    pub filters: Vec<ParametricEqBand>,
    pub preamp: f64,
}

fn default_headphone_form() -> String {
    "OverEar".to_owned()
}

/// AutoEqTuning (eq/autoeq/AutoEqTuning.kt) — defaults per upstream.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AutoEqTuning {
    #[serde(default = "lf_8000")]
    pub max_frequency: f64,
    #[serde(default = "lf_10")]
    pub min_frequency: f64,
    #[serde(default = "auto_eq_bass")]
    pub bass_boost: f64,
    #[serde(default = "auto_eq_treble")]
    pub treble_boost: f64,
    #[serde(default)]
    pub smooth: bool,
}

fn lf_8000() -> f64 {
    8000.0
}
fn lf_10() -> f64 {
    10.0
}
fn auto_eq_bass() -> f64 {
    6.0
}
fn auto_eq_treble() -> f64 {
    0.0
}

impl Default for AutoEqTuning {
    fn default() -> Self {
        AutoEqTuning {
            max_frequency: lf_8000(),
            min_frequency: lf_10(),
            bass_boost: auto_eq_bass(),
            treble_boost: auto_eq_treble(),
            smooth: false,
        }
    }
}

/// One EQ graph node / band slider (eq/data/ParametricEQ.kt).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ParametricEqBand {
    pub frequency: f64,
    pub gain: f64,
    #[serde(default = "octave_q")]
    pub q: f64,
    #[serde(default = "filter_pk")]
    pub filter_type: String, // FilterType name; PK default
}

fn octave_q() -> f64 {
    1.41
}
fn filter_pk() -> String {
    "PK".to_owned()
}

/// A saved EQ preset (AudioLabStore.kt:64-69).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EqPreset {
    pub id: String,
    pub name: String,
    pub nodes: Vec<ParametricEqBand>,
    #[serde(default)]
    pub preamp: f64,
}

/// The user's EQ curve (AudioLabStore.kt:71-83).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EqSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "flat_nodes")]
    pub nodes: Vec<ParametricEqBand>,
    #[serde(default)]
    pub preamp: f64,
    #[serde(default = "default_true")]
    pub auto_preamp: bool,
    #[serde(default = "default_flat_id")]
    pub preset_id: Option<String>,
    #[serde(default)]
    pub correction: Option<AutoEqCorrection>,
    #[serde(default = "default_true")]
    pub correction_enabled: bool,
}

impl Default for EqSettings {
    fn default() -> Self {
        EqSettings {
            enabled: false,
            nodes: flat_nodes(),
            preamp: 0.0,
            auto_preamp: true,
            preset_id: default_flat_id(),
            correction: None,
            correction_enabled: true,
        }
    }
}

fn default_true() -> bool {
    true
}
fn default_flat_id() -> Option<String> {
    Some("flat".to_owned())
}

/// EqPresets.Frequencies (AudioLabStore.kt:431).
pub const EQ_BAND_FREQUENCIES: [f64; 10] = [
    31.0, 62.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0,
];

/// EqPresets.flatNodes() (AudioLabStore.kt:442).
pub fn flat_nodes() -> Vec<ParametricEqBand> {
    EQ_BAND_FREQUENCIES
        .iter()
        .map(|&f| ParametricEqBand {
            frequency: f,
            gain: 0.0,
            q: octave_q(),
            filter_type: filter_pk(),
        })
        .collect()
}

/// Loudness (AudioLabStore.kt:85-92).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LoudnessSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "lf4")]
    pub boost_db: f32,
    #[serde(default = "lf03")]
    pub compression: f32,
}

fn lf4() -> f32 {
    4.0
}
fn lf03() -> f32 {
    0.3
}

impl Default for LoudnessSettings {
    fn default() -> Self {
        LoudnessSettings {
            enabled: false,
            boost_db: lf4(),
            compression: lf03(),
        }
    }
}

/// Spatial audio (AudioLabStore.kt:94-108).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SpatialSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "spatial_crossfeed")]
    pub mode: String, // SpatialMode name
    #[serde(default = "lf700")]
    pub cutoff_hz: f32,
    #[serde(default = "lf45")]
    pub feed_db: f32,
    #[serde(default = "lf30")]
    pub speaker_angle: f32,
    #[serde(default = "lf1")]
    pub amount: f32,
    #[serde(default)]
    pub surround: bool,
    #[serde(default)]
    pub mono: bool,
}

fn spatial_crossfeed() -> String {
    "Crossfeed".to_owned()
}
fn lf700() -> f32 {
    700.0
}
fn lf45() -> f32 {
    4.5
}
fn lf30() -> f32 {
    30.0
}
fn lf1() -> f32 {
    1.0
}

impl Default for SpatialSettings {
    fn default() -> Self {
        SpatialSettings {
            enabled: false,
            mode: spatial_crossfeed(),
            cutoff_hz: lf700(),
            feed_db: lf45(),
            speaker_angle: lf30(),
            amount: lf1(),
            surround: false,
            mono: false,
        }
    }
}

/// ReplayGain normalization (AudioLabStore.kt:110-117).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NormalizeSettings {
    #[serde(default = "rg_track")]
    pub replay_gain_mode: String, // ReplayGainMode name
    #[serde(default = "lf3")]
    pub preamp_db: f32,
}

fn rg_track() -> String {
    "Track".to_owned()
}
fn lf3() -> f32 {
    3.0
}

impl Default for NormalizeSettings {
    fn default() -> Self {
        NormalizeSettings {
            replay_gain_mode: rg_track(),
            preamp_db: lf3(),
        }
    }
}

/// Playback speed/pitch (AudioLabStore.kt:119-131). `pitch` is derived:
/// `2^(semitones/12)` when pitch is preserved, else it follows speed.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlaybackSettings {
    #[serde(default = "lf1")]
    pub speed: f32,
    #[serde(default = "default_true")]
    pub preserve_pitch: bool,
    #[serde(default)]
    pub pitch_semitones: f32,
}

impl Default for PlaybackSettings {
    fn default() -> Self {
        PlaybackSettings {
            speed: 1.0,
            preserve_pitch: true,
            pitch_semitones: 0.0,
        }
    }
}

impl PlaybackSettings {
    /// The effective pitch ratio (AudioLabStore.kt:127-128).
    pub fn pitch(&self) -> f32 {
        if self.preserve_pitch {
            2.0f32.powf(self.pitch_semitones / 12.0)
        } else {
            self.speed
        }
    }

    /// Neither speed nor pitch altered (AudioLabStore.kt:130).
    pub fn is_normal(&self) -> bool {
        self.speed == 1.0 && self.pitch() == 1.0
    }

    /// Clamp speed into the sonic range (AudioLabStore.kt:270).
    pub fn clamped(mut self) -> Self {
        self.speed = self.speed.clamp(MIN_SPEED, MAX_SPEED);
        self
    }
}

/// Exponential volume curve (AudioLabStore.kt:133-140): exponent 1-5, limit.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VolumeSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "lf2")]
    pub exponent: f32,
    #[serde(default = "lf1")]
    pub limit: f32,
}

fn lf2() -> f32 {
    2.0
}

impl Default for VolumeSettings {
    fn default() -> Self {
        VolumeSettings {
            enabled: false,
            exponent: lf2(),
            limit: lf1(),
        }
    }
}

/// 8D rotation (AudioLabStore.kt:142-156), normalized on load
/// (rotation 0.05-0.5 Hz, depth 0-1, reverb 0-1).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EightDSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "lf015")]
    pub rotation_speed_hz: f32,
    #[serde(default = "lf085")]
    pub depth: f32,
    #[serde(default = "lf04")]
    pub reverb_level: f32,
}

fn lf015() -> f32 {
    0.15
}
fn lf085() -> f32 {
    0.85
}
fn lf04() -> f32 {
    0.4
}

impl Default for EightDSettings {
    fn default() -> Self {
        EightDSettings {
            enabled: false,
            rotation_speed_hz: lf015(),
            depth: lf085(),
            reverb_level: lf04(),
        }
    }
}

impl EightDSettings {
    /// AudioLabStore.kt:149-153 `normalized()`.
    pub fn normalized(mut self) -> Self {
        self.rotation_speed_hz = self.rotation_speed_hz.clamp(0.05, 0.5);
        self.depth = self.depth.clamp(0.0, 1.0);
        self.reverb_level = self.reverb_level.clamp(0.0, 1.0);
        self
    }
}

/// Reverb (eq/ReverbSettings.kt:7-18).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReverbSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "lf05")]
    pub mix: f32,
    #[serde(default = "lf055")]
    pub room_size: f32,
    #[serde(default = "lf045")]
    pub damping: f32,
    #[serde(default = "lf06")]
    pub wet: f32,
    #[serde(default = "lf1")]
    pub dry: f32,
    #[serde(default = "lf1")]
    pub width: f32,
    #[serde(default = "lf12")]
    pub pre_delay_ms: f32,
    #[serde(default = "lf180")]
    pub low_cut_hz: f32,
    #[serde(default)]
    pub preset_id: Option<String>,
}

fn lf05() -> f32 {
    0.5
}
fn lf055() -> f32 {
    0.55
}
fn lf045() -> f32 {
    0.45
}
fn lf06() -> f32 {
    0.6
}
fn lf12() -> f32 {
    12.0
}
fn lf180() -> f32 {
    180.0
}

impl Default for ReverbSettings {
    fn default() -> Self {
        ReverbSettings {
            enabled: false,
            mix: lf05(),
            room_size: lf055(),
            damping: lf045(),
            wet: lf06(),
            dry: lf1(),
            width: lf1(),
            pre_delay_ms: lf12(),
            low_cut_hz: lf180(),
            preset_id: None,
        }
    }
}

/// Stage (3D Sims room) settings (eq/stage/StageSettings.kt:45-58). The
/// object graph is opaque JSON on the config side; Phase 20 owns the model.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StageSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub preset_id: Option<String>,
    /// Upstream `StageObject` list — kept as raw JSON values until Phase 20.
    #[serde(default)]
    pub objects: Vec<serde_json::Value>,
    #[serde(default)]
    pub listener: Option<serde_json::Value>,
    #[serde(default = "lf05")]
    pub room_size: f32,
    #[serde(default)]
    pub walls: Option<serde_json::Value>,
    #[serde(default = "lf1")]
    pub room_amount: f32,
    #[serde(default)]
    pub level_db: f32,
    #[serde(default = "stage_stereo")]
    pub on_speakers: String, // StageSpeakers name
    #[serde(default)]
    pub output_kinds: HashMap<String, bool>,
}

fn stage_stereo() -> String {
    "Stereo".to_owned()
}

impl Default for StageSettings {
    fn default() -> Self {
        StageSettings {
            enabled: false,
            preset_id: None,
            objects: Vec::new(),
            listener: None,
            room_size: lf05(),
            walls: None,
            room_amount: lf1(),
            level_db: 0.0,
            on_speakers: stage_stereo(),
            output_kinds: HashMap::new(),
        }
    }
}

/// Everything the audio lab persists (one JSON blob per key, upstream names).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AudioLab {
    pub eq: EqSettings,
    pub reverb: ReverbSettings,
    pub presets: Vec<EqPreset>,
    pub loudness: LoudnessSettings,
    pub spatial: SpatialSettings,
    pub eight_d: EightDSettings,
    pub playback: PlaybackSettings,
    pub volume: VolumeSettings,
    pub normalize: NormalizeSettings,
    pub stage: StageSettings,
    /// The legacy 10-band migration marker (AudioLabStore.kt:406).
    pub migrated_legacy_eq: bool,
}

impl AudioLab {
    /// Read the `audio_lab` section out of a settings snapshot. Each key is
    /// parsed independently; a missing or corrupt blob falls back to its
    /// default — upstream `load(key) ?: Default` (AudioLabStore.kt:398-402).
    pub fn from_settings(settings: &Settings) -> AudioLab {
        let section = settings.prefs_section("audio_lab");
        let get = |key: &str| -> Option<&str> {
            section
                .and_then(|s| s.get(key))
                .and_then(SettingValue::as_str)
        };
        let parse = |key: &str| -> Option<String> { get(key).map(str::to_owned) };
        AudioLab {
            eq: parse("eq").and_then(|j| from_json(&j)).unwrap_or_default(),
            reverb: parse("reverb")
                .and_then(|j| from_json(&j))
                .unwrap_or_default(),
            presets: parse("presets")
                .and_then(|j| from_json(&j))
                .unwrap_or_default(),
            loudness: parse("loudness")
                .and_then(|j| from_json(&j))
                .unwrap_or_default(),
            spatial: parse("spatial")
                .and_then(|j| from_json(&j))
                .unwrap_or_default(),
            eight_d: parse("eight_d")
                .and_then(|j| from_json::<EightDSettings>(&j))
                .map(EightDSettings::normalized)
                .unwrap_or_default(),
            playback: parse("playback")
                .and_then(|j| from_json::<PlaybackSettings>(&j))
                .map(PlaybackSettings::clamped)
                .unwrap_or_default(),
            volume: parse("volume")
                .and_then(|j| from_json(&j))
                .unwrap_or_default(),
            normalize: parse("normalize")
                .and_then(|j| from_json(&j))
                .unwrap_or_default(),
            stage: parse("stage")
                .and_then(|j| from_json(&j))
                .unwrap_or_default(),
            migrated_legacy_eq: section
                .and_then(|s| s.get("migrated_legacy_eq"))
                .and_then(SettingValue::as_bool)
                .unwrap_or(false),
        }
    }

    /// Write the section back (same key names, `encodeDefaults = true`
    /// so defaults round-trip exactly like upstream's serializer).
    pub fn write_to(&self, store: &SettingsStore) -> Result<(), crate::ConfigError> {
        let mut section: HashMap<String, SettingValue> = HashMap::new();
        let mut put = |key: &str, value: String| {
            section.insert(key.to_owned(), SettingValue::Str(value));
        };
        put("eq", to_json(&self.eq));
        put("reverb", to_json(&self.reverb));
        put("presets", to_json(&self.presets));
        put("loudness", to_json(&self.loudness));
        put("spatial", to_json(&self.spatial));
        put("eight_d", to_json(&self.eight_d.clone().normalized()));
        put("playback", to_json(&self.playback.clone().clamped()));
        put("volume", to_json(&self.volume));
        put("normalize", to_json(&self.normalize));
        put("stage", to_json(&self.stage));
        section.insert(
            "migrated_legacy_eq".to_owned(),
            SettingValue::Bool(self.migrated_legacy_eq),
        );
        store.set_prefs_section("audio_lab", section)
    }
}

fn from_json<T: for<'de> Deserialize<'de>>(text: &str) -> Option<T> {
    serde_json::from_str(text).ok() // upstream: runCatching + log warn + null
}

fn to_json<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).expect("audio-lab types never fail to serialize")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_lab_round_trip_through_store() {
        let store = SettingsStore::with_defaults();
        let mut lab = AudioLab::from_settings(&store.snapshot());
        assert!(!lab.eq.enabled);
        lab.eq.enabled = true;
        lab.eq.nodes[0].gain = 4.5;
        lab.playback.speed = 1.25;
        lab.playback.pitch_semitones = -2.0;
        lab.volume.exponent = 3.0;
        lab.volume.limit = 0.8;
        lab.presets.push(EqPreset {
            id: "user_1".to_owned(),
            name: "mine".to_owned(),
            nodes: flat_nodes(),
            preamp: -1.0,
        });
        lab.write_to(&store).unwrap();

        let back = AudioLab::from_settings(&store.snapshot());
        assert!(back.eq.enabled);
        assert_eq!(back.eq.nodes[0].gain, 4.5);
        assert_eq!(back.playback.speed, 1.25);
        assert_eq!(back.playback.pitch(), 2.0f32.powf(-2.0 / 12.0));
        assert_eq!(back.volume.exponent, 3.0);
        assert_eq!(back.volume.limit, 0.8);
        assert_eq!(back.presets.len(), 1);
        assert_eq!(back.presets[0].name, "mine");
    }

    #[test]
    fn corrupt_blob_falls_back_to_default() {
        let store = SettingsStore::with_defaults();
        let mut section = HashMap::new();
        section.insert("eq".to_owned(), SettingValue::Str("{ not json".to_owned()));
        store.set_prefs_section("audio_lab", section).unwrap();
        let lab = AudioLab::from_settings(&store.snapshot());
        assert_eq!(lab.eq, EqSettings::default());
    }

    #[test]
    fn speed_clamps_to_sonic_range() {
        let s = PlaybackSettings {
            speed: 12.0,
            ..Default::default()
        }
        .clamped();
        assert_eq!(s.speed, MAX_SPEED);
        let s = PlaybackSettings {
            speed: 0.01,
            ..Default::default()
        }
        .clamped();
        assert_eq!(s.speed, MIN_SPEED);
    }

    #[test]
    fn eight_d_normalizes() {
        let s = EightDSettings {
            rotation_speed_hz: 2.0,
            depth: 7.0,
            reverb_level: -1.0,
            ..Default::default()
        }
        .normalized();
        assert_eq!(s.rotation_speed_hz, 0.5);
        assert_eq!(s.depth, 1.0);
        assert_eq!(s.reverb_level, 0.0);
    }

    #[test]
    fn speed_presets_match_upstream() {
        assert!(SPEED_PRESETS.iter().any(|(id, ..)| *id == "slowed_reverb"));
        let (_, speed, _) = SPEED_PRESETS
            .iter()
            .find(|(id, ..)| *id == "nightcore")
            .unwrap();
        assert_eq!(*speed, 1.25);
    }

    #[test]
    fn default_keys_match_upstream_names() {
        // upstream prefs keys: eq, reverb, loudness, spatial, eight_d,
        // playback, volume, normalize, stage, presets, migrated_legacy_eq
        let store = SettingsStore::with_defaults();
        AudioLab::default().write_to(&store).unwrap();
        let snapshot = store.snapshot();
        let section = snapshot.prefs_section("audio_lab").unwrap();
        for key in [
            "eq",
            "reverb",
            "loudness",
            "spatial",
            "eight_d",
            "playback",
            "volume",
            "normalize",
            "stage",
            "presets",
            "migrated_legacy_eq",
        ] {
            assert!(section.contains_key(key), "missing audio_lab key {key}");
        }
    }
}
