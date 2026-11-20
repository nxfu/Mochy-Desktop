//! The settings store: load-on-start, atomic persistence, live watchers.
//!
//! Upstream model (utils/DataStore.kt): one `settings` DataStore whose
//! `PreferencesCache` keeps a warm snapshot for readers and pushes every edit
//! through an IO-dispatched write; Compose `rememberPreference` re-reads from
//! the snapshot. The desktop port keeps the same shape without Android
//! coroutine machinery: a [`SettingsStore`] with an inner lock, a snapshot
//! clone for readers, and change broadcast (per-key, per-page and global
//! watchers) that later phases subscribe to.
//!
//! Persistence is a single atomic JSON file: write temp + fsync + rename
//! (crash-safe; a torn write can never replace the previous settings).

use std::collections::HashMap;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex, RwLock};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::warn;

use crate::{SettingId, SettingValue, SettingsPage};

/// Format tag of the desktop settings file (like the transfer archive's
/// `"mochy-transfer"` manifest tag, TransferArchive.kt:51).
pub const PERSISTED_FORMAT: &str = "mochy-desktop-settings";
/// Bump when the file layout changes incompatibly.
pub const PERSISTED_VERSION: u32 = 1;

/// Errors the settings store can produce.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// The settings file exists but is not readable as our format.
    #[error("settings file is corrupt: {0}")]
    Corrupt(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("serialization failed: {0}")]
    Serialization(String),
}

impl From<serde_json::Error> for ConfigError {
    fn from(e: serde_json::Error) -> Self {
        ConfigError::Serialization(e.to_string())
    }
}

/// A watcher could not be registered (receiver side already gone, etc.).
#[derive(Debug, Error)]
#[error("watcher registration failed")]
pub struct WatchError;

/// One typed-json entry of the persisted file — the same `{"t","v"}` shape the
/// Android transfer archive uses (TransferArchive.kt:160-173), so imports,
/// exports and the desktop file all agree on one encoding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypedEntry {
    /// Type tag: `b`/`i`/`l`/`f`/`d`/`s`/`ss`/`bytes`.
    pub t: String,
    /// The value.
    pub v: serde_json::Value,
}

/// The persisted file body.
#[derive(Debug, Serialize, Deserialize)]
struct PersistedFile {
    format: String,
    version: u32,
    /// `key -> typed entry`, Android key names.
    values: HashMap<String, TypedEntry>,
    /// SharedPreferences-store sections carried over from Android
    /// (`audio_lab`, `song_tuning`, ...) — same typed-entry encoding.
    #[serde(default)]
    prefs: HashMap<String, HashMap<String, TypedEntry>>,
}

/// A consistent snapshot of all settings (cheap to clone: values map only).
///
/// Typed accessors are macro-generated (see `registry.rs`); unknown keys (for
/// example from a newer Android export) are preserved verbatim so a
/// round-trip never loses data.
#[derive(Debug, Clone, Default)]
pub struct Settings {
    /// Android key name -> current value. Keys absent from the map read as
    /// their registry default.
    pub(crate) values: HashMap<String, SettingValue>,
    /// SharedPreferences-store sections, verbatim.
    pub(crate) prefs: HashMap<String, HashMap<String, SettingValue>>,
}

impl Settings {
    /// Build from a raw value map (used by the macro's defaults table and by
    /// the importers).
    pub fn from_values(values: HashMap<String, SettingValue>) -> Settings {
        Settings {
            values,
            prefs: HashMap::new(),
        }
    }

    /// The number of stored values (not the registry size).
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Whether nothing at all is stored (fresh install).
    pub fn is_empty(&self) -> bool {
        self.values.is_empty() && self.prefs.is_empty()
    }

    /// Raw value of a key.
    pub fn raw(&self, key: &str) -> Option<&SettingValue> {
        self.values.get(key)
    }

    /// All stored key/value pairs (Android key names).
    pub fn iter(&self) -> impl Iterator<Item = (&String, &SettingValue)> {
        self.values.iter()
    }

    /// Keys that exist in this snapshot but not in the registry (forward
    /// compatibility check — used by the import tests).
    pub fn unknown_keys(&self) -> Vec<&str> {
        self.values
            .keys()
            .filter(|k| SettingId::from_key_name(k).is_none())
            .map(String::as_str)
            .collect()
    }

    /// A typed view of a SharedPreferences-store section (from an Android
    /// import or the audio-lab editor).
    pub fn prefs_section(&self, store: &str) -> Option<&HashMap<String, SettingValue>> {
        self.prefs.get(store)
    }

    /// Mutable access to a prefs section, creating it if needed.
    pub fn prefs_section_mut(&mut self, store: &str) -> &mut HashMap<String, SettingValue> {
        self.prefs.entry(store.to_owned()).or_default()
    }

    // ---- typed readers used by the macro-generated accessors -------------

    pub(crate) fn bool_value(&self, key: &str) -> Option<bool> {
        self.values.get(key).and_then(SettingValue::as_bool)
    }

    pub(crate) fn i32_value(&self, key: &str) -> Option<i32> {
        self.values.get(key).and_then(SettingValue::as_i32)
    }

    pub(crate) fn i64_value(&self, key: &str) -> Option<i64> {
        self.values.get(key).and_then(SettingValue::as_i64)
    }

    pub(crate) fn f32_value(&self, key: &str) -> Option<f32> {
        self.values.get(key).and_then(SettingValue::as_f32)
    }

    pub(crate) fn str_value(&self, key: &str) -> Option<&str> {
        self.values.get(key).and_then(SettingValue::as_str)
    }

    /// Search the registry: rows whose title or keywords contain every word
    /// of `query` (case-insensitive), optionally restricted to one page.
    /// Mirrors the upstream settings search
    /// (ui/screens/settings/MochySettingsScreen.kt filters the 100-entry
    /// index by title + keywords).
    pub fn search(query: &str, page: Option<SettingsPage>) -> Vec<SettingId> {
        let words: Vec<String> = query
            .split_whitespace()
            .map(str::to_ascii_lowercase)
            .collect();
        if words.is_empty() {
            return SettingId::ALL
                .iter()
                .copied()
                .filter(|id| !id.hidden() && page.is_none_or(|p| id.page() == p))
                .collect();
        }
        SettingId::ALL
            .iter()
            .copied()
            .filter(|id| !id.hidden() && page.is_none_or(|p| id.page() == p))
            .filter(|id| {
                let title = id.title().to_ascii_lowercase();
                let keywords = id.keywords().to_ascii_lowercase();
                words
                    .iter()
                    .all(|w| title.contains(w) || keywords.contains(w))
            })
            .collect()
    }
}

/// Handle returned by [`SettingsStore::watch_key`] / [`watch_page`] /
/// [`watch_all`]; unsubscribes on drop.
#[derive(Debug)]
pub struct WatchGuard {
    store: Option<Arc<Watchers>>,
    slot: WatchSlot,
    id: u64,
}

impl Drop for WatchGuard {
    fn drop(&mut self) {
        if let Some(watchers) = self.store.take() {
            watchers.remove(&self.slot, self.id);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum WatchSlot {
    Key(SettingId),
    Page(SettingsPage),
    All,
}

/// What a watcher receives.
#[derive(Debug, Clone)]
pub enum WatchEvent {
    /// A single key changed (per-key watchers).
    Changed { id: SettingId, value: SettingValue },
    /// Any key on the watched page changed (per-page watchers).
    PageChanged {
        id: SettingId,
        page: SettingsPage,
        value: SettingValue,
    },
    /// Global stream (per-key granularity).
    AnyChanged { id: SettingId, value: SettingValue },
}

/// A registered watcher: the sender plus the unique id it was created with
/// (`std::sync::mpsc::Sender` has no equality, so removal matches on the id).
#[derive(Debug)]
struct WatchSender {
    id: u64,
    sender: Sender<WatchEvent>,
}

fn next_watcher_id() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

#[derive(Debug, Default)]
struct Watchers {
    key: Mutex<HashMap<SettingId, Vec<WatchSender>>>,
    page: Mutex<HashMap<SettingsPage, Vec<WatchSender>>>,
    all: Mutex<Vec<WatchSender>>,
}

impl Watchers {
    fn remove(&self, slot: &WatchSlot, id: u64) {
        fn drop_dead(list: &mut Vec<WatchSender>, id: u64) {
            list.retain(|w| w.id != id);
        }
        match slot {
            WatchSlot::Key(key) => {
                if let Ok(mut map) = self.key.lock()
                    && let Some(list) = map.get_mut(key)
                {
                    drop_dead(list, id);
                }
            }
            WatchSlot::Page(page) => {
                if let Ok(mut map) = self.page.lock()
                    && let Some(list) = map.get_mut(page)
                {
                    drop_dead(list, id);
                }
            }
            WatchSlot::All => {
                if let Ok(mut list) = self.all.lock() {
                    drop_dead(&mut list, id);
                }
            }
        }
    }

    fn broadcast(&self, id: SettingId, page: SettingsPage, value: SettingValue) {
        let event = WatchEvent::Changed {
            id,
            value: value.clone(),
        };
        if let Ok(map) = self.key.lock()
            && let Some(list) = map.get(&id)
        {
            for watcher in list {
                // a dead receiver is pruned by its WatchGuard; a failed
                // send here only means the receiver was dropped mid-race
                let _ = watcher.sender.send(event.clone());
            }
        }
        let page_event = WatchEvent::PageChanged {
            id,
            page,
            value: value.clone(),
        };
        if let Ok(map) = self.page.lock()
            && let Some(list) = map.get(&page)
        {
            for watcher in list {
                let _ = watcher.sender.send(page_event.clone());
            }
        }
        let any_event = WatchEvent::AnyChanged { id, value };
        if let Ok(list) = self.all.lock() {
            for watcher in list.iter() {
                let _ = watcher.sender.send(any_event.clone());
            }
        }
    }
}

/// The settings store: load-on-start, in-memory snapshot, atomic save,
/// live watchers. Clone shares the same underlying store.
#[derive(Debug, Clone)]
pub struct SettingsStore {
    inner: Arc<StoreInner>,
}

#[derive(Debug)]
struct StoreInner {
    path: PathBuf,
    settings: RwLock<Settings>,
    watchers: Arc<Watchers>,
}

impl SettingsStore {
    /// Load settings from `path`; a missing file yields the defaults table
    /// (first run). A corrupt file logs a warning and falls back to defaults
    /// rather than refusing to start — the same resilience the Android
    /// DataStore offers through its exception surface.
    pub fn load(path: impl Into<PathBuf>) -> SettingsStore {
        let path = path.into();
        let settings = match fs::read(&path) {
            Ok(bytes) => match Self::decode(&bytes) {
                Ok(settings) => settings,
                Err(e) => {
                    warn!(?path, error = %e, "settings file unreadable; using defaults");
                    Settings::with_defaults()
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Settings::with_defaults(),
            Err(e) => {
                warn!(?path, error = %e, "settings file unreadable; using defaults");
                Settings::with_defaults()
            }
        };
        SettingsStore {
            inner: Arc::new(StoreInner {
                path,
                settings: RwLock::new(settings),
                watchers: Arc::new(Watchers::default()),
            }),
        }
    }

    /// A store over pure defaults (tests, previews).
    pub fn with_defaults() -> SettingsStore {
        SettingsStore {
            inner: Arc::new(StoreInner {
                path: PathBuf::new(),
                settings: RwLock::new(Settings::with_defaults()),
                watchers: Arc::new(Watchers::default()),
            }),
        }
    }

    /// The path this store persists to.
    pub fn path(&self) -> &Path {
        &self.inner.path
    }

    /// Replace the whole in-memory settings (importers). Watchers are not
    /// notified — imports are bulk loads before subscribers exist.
    pub(crate) fn replace_settings(&self, settings: Settings) {
        let mut guard = self.inner.settings.write().expect("settings lock poisoned");
        *guard = settings;
    }

    fn decode(bytes: &[u8]) -> Result<Settings, ConfigError> {
        let file: PersistedFile = serde_json::from_slice(bytes)?;
        if file.format != PERSISTED_FORMAT {
            return Err(ConfigError::Corrupt(format!(
                "unknown format {:?} (expected {PERSISTED_FORMAT})",
                file.format
            )));
        }
        if file.version > PERSISTED_VERSION {
            return Err(ConfigError::Corrupt(format!(
                "settings from a newer Mochy (version {} > {})",
                file.version, PERSISTED_VERSION
            )));
        }
        let mut values = HashMap::with_capacity(file.values.len());
        for (key, entry) in file.values {
            if let Some(value) = typed_entry_to_value(&entry) {
                values.insert(key, value);
            }
        }
        let mut prefs = HashMap::with_capacity(file.prefs.len());
        for (store, entries) in file.prefs {
            let mut section = HashMap::with_capacity(entries.len());
            for (key, entry) in entries {
                if let Some(value) = typed_entry_to_value(&entry) {
                    section.insert(key, value);
                }
            }
            prefs.insert(store, section);
        }
        Ok(Settings { values, prefs })
    }

    fn encode(&self, settings: &Settings) -> Result<Vec<u8>, ConfigError> {
        let mut values = HashMap::with_capacity(settings.values.len());
        for (key, value) in &settings.values {
            values.insert(key.clone(), value_to_typed_entry(value));
        }
        let mut prefs = HashMap::with_capacity(settings.prefs.len());
        for (store, section) in &settings.prefs {
            let mut entries = HashMap::with_capacity(section.len());
            for (key, value) in section {
                entries.insert(key.clone(), value_to_typed_entry(value));
            }
            prefs.insert(store.clone(), entries);
        }
        let file = PersistedFile {
            format: PERSISTED_FORMAT.to_owned(),
            version: PERSISTED_VERSION,
            values,
            prefs,
        };
        Ok(serde_json::to_vec_pretty(&file)?)
    }

    /// A consistent snapshot of the current settings.
    pub fn snapshot(&self) -> Settings {
        self.inner
            .settings
            .read()
            .expect("settings lock poisoned")
            .clone()
    }

    /// Read one raw value.
    pub fn get(&self, id: SettingId) -> SettingValue {
        self.snapshot()
            .raw(id.key_name())
            .cloned()
            .unwrap_or_else(|| id.default_value())
    }

    /// Write one registry value: updates memory, broadcasts to the key, page
    /// and global watchers. Persistence is explicit ([`save`]) — later phases
    /// decide the flush policy (upstream writes through on every edit; the
    /// desktop shell debounces instead, matching DataStore's async writes).
    pub fn set_by_id(&self, id: SettingId, value: SettingValue) -> Result<(), ConfigError> {
        let page = {
            let mut guard = self.inner.settings.write().expect("settings lock poisoned");
            guard.values.insert(id.key_name().to_owned(), value.clone());
            id.page()
        };
        self.inner.watchers.broadcast(id, page, value);
        Ok(())
    }

    /// Write a raw (possibly unknown) key. Unknown keys do not broadcast a
    /// typed id; page watchers are not notified either.
    pub fn set_raw(&self, key: &str, value: SettingValue) -> Result<(), ConfigError> {
        let mut guard = self.inner.settings.write().expect("settings lock poisoned");
        guard.values.insert(key.to_owned(), value);
        Ok(())
    }

    /// Replace a whole prefs-store section (audio lab and friends).
    pub fn set_prefs_section(
        &self,
        store: &str,
        section: HashMap<String, SettingValue>,
    ) -> Result<(), ConfigError> {
        let mut guard = self.inner.settings.write().expect("settings lock poisoned");
        guard.prefs.insert(store.to_owned(), section);
        Ok(())
    }

    /// Atomically persist: write `<file>.tmp`, fsync, rename over the target.
    /// The fsync of the parent directory matches the durability contract of
    /// the Android DataStore's atomic rename.
    pub fn save(&self) -> Result<(), ConfigError> {
        if self.inner.path.as_os_str().is_empty() {
            return Ok(()); // defaults-only store (tests): nothing to write
        }
        let bytes = {
            let guard = self.inner.settings.read().expect("settings lock poisoned");
            self.encode(&guard)?
        };
        let path = &self.inner.path;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("tmp");
        {
            let mut file = fs::File::create(&tmp)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
        }
        fs::rename(&tmp, path)?;
        if let Some(parent) = path.parent()
            && let Ok(dir) = fs::File::open(parent)
        {
            let _ = dir.sync_all();
        }
        Ok(())
    }

    /// Watch one key: the receiver yields the new value on every change.
    /// The returned [`WatchGuard`] keeps the subscription alive; dropping it
    /// unsubscribes (senders to a dropped receiver are pruned lazily).
    pub fn watch_key(
        &self,
        id: SettingId,
    ) -> Result<(Receiver<WatchEvent>, WatchGuard), WatchError> {
        let (sender, receiver) = std::sync::mpsc::channel();
        let watcher_id = next_watcher_id();
        self.inner
            .watchers
            .key
            .lock()
            .map_err(|_| WatchError)?
            .entry(id)
            .or_default()
            .push(WatchSender {
                id: watcher_id,
                sender,
            });
        let guard = WatchGuard {
            store: Some(Arc::clone(&self.inner.watchers)),
            slot: WatchSlot::Key(id),
            id: watcher_id,
        };
        Ok((receiver, guard))
    }

    /// Watch every key on one page.
    pub fn watch_page(
        &self,
        page: SettingsPage,
    ) -> Result<(Receiver<WatchEvent>, WatchGuard), WatchError> {
        let (sender, receiver) = std::sync::mpsc::channel();
        let watcher_id = next_watcher_id();
        self.inner
            .watchers
            .page
            .lock()
            .map_err(|_| WatchError)?
            .entry(page)
            .or_default()
            .push(WatchSender {
                id: watcher_id,
                sender,
            });
        let guard = WatchGuard {
            store: Some(Arc::clone(&self.inner.watchers)),
            slot: WatchSlot::Page(page),
            id: watcher_id,
        };
        Ok((receiver, guard))
    }

    /// Watch every change.
    pub fn watch_all(&self) -> Result<(Receiver<WatchEvent>, WatchGuard), WatchError> {
        let (sender, receiver) = std::sync::mpsc::channel();
        let watcher_id = next_watcher_id();
        self.inner
            .watchers
            .all
            .lock()
            .map_err(|_| WatchError)?
            .push(WatchSender {
                id: watcher_id,
                sender,
            });
        let guard = WatchGuard {
            store: Some(Arc::clone(&self.inner.watchers)),
            slot: WatchSlot::All,
            id: watcher_id,
        };
        Ok((receiver, guard))
    }

    /// JSON backup serializer (the persisted file itself).
    pub fn to_json_string(&self) -> Result<String, ConfigError> {
        let guard = self.inner.settings.read().expect("settings lock poisoned");
        let bytes = self.encode(&guard)?;
        String::from_utf8(bytes).map_err(|e| ConfigError::Serialization(e.to_string()))
    }

    /// TOML backup serializer: `[values]` table with native types, plus one
    /// table per prefs store. Human-editable, diff-friendly.
    pub fn to_toml_string(&self) -> Result<String, ConfigError> {
        let settings = self.snapshot();
        let mut table = toml::Table::new();
        let mut values = toml::Table::new();
        for (key, value) in &settings.values {
            if let Some(item) = value_to_toml(value) {
                values.insert(key.clone(), item);
            }
        }
        table.insert("values".to_owned(), values.into());
        let mut prefs = toml::Table::new();
        for (store, section) in &settings.prefs {
            let mut entries = toml::Table::new();
            for (key, value) in section {
                if let Some(item) = value_to_toml(value) {
                    entries.insert(key.clone(), item);
                }
            }
            prefs.insert(store.clone(), entries.into());
        }
        table.insert("prefs".to_owned(), prefs.into());
        toml::to_string_pretty(&table).map_err(|e| ConfigError::Serialization(e.to_string()))
    }

    /// Restore from a TOML backup produced by [`to_toml_string`].
    pub fn import_toml(&self, text: &str) -> Result<(), ConfigError> {
        let table: toml::Table =
            toml::from_str(text).map_err(|e| ConfigError::Serialization(e.to_string()))?;
        let mut values = HashMap::new();
        if let Some(toml::Value::Table(values_table)) = table.get("values") {
            for (key, item) in values_table {
                if let Some(value) = toml_to_value(item) {
                    values.insert(key.clone(), value);
                }
            }
        }
        let mut prefs = HashMap::new();
        if let Some(toml::Value::Table(prefs_table)) = table.get("prefs") {
            for (store, entries) in prefs_table {
                if let toml::Value::Table(entries) = entries {
                    let mut section = HashMap::new();
                    for (key, item) in entries {
                        if let Some(value) = toml_to_value(item) {
                            section.insert(key.clone(), value);
                        }
                    }
                    prefs.insert(store.clone(), section);
                }
            }
        }
        let mut guard = self.inner.settings.write().expect("settings lock poisoned");
        guard.values.extend(values);
        guard.prefs.extend(prefs);
        Ok(())
    }
}

// Watcher slots are shared via Arc<Watchers> held by StoreInner; WatchGuard
// removes its sender on drop so a closed watcher never leaks.

/// Convert a persisted/archive typed entry into a [`SettingValue`]
/// (TransferArchive.kt:120-128 `jsonIntoPreferences` type switch).
pub(crate) fn typed_entry_to_value(entry: &TypedEntry) -> Option<SettingValue> {
    match entry.t.as_str() {
        "b" => entry.v.as_bool().map(SettingValue::Bool),
        "i" => entry
            .v
            .as_i64()
            .and_then(|v| i32::try_from(v).ok())
            .map(SettingValue::Int),
        "l" => entry.v.as_i64().map(SettingValue::Long),
        "f" | "d" => entry.v.as_f64().map(|v| SettingValue::Float(v as f32)),
        "s" => entry.v.as_str().map(|v| SettingValue::Str(v.to_owned())),
        "ss" => entry.v.as_array().map(|items| {
            SettingValue::StringSet(
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(str::to_owned))
                    .collect(),
            )
        }),
        "bytes" => entry
            .v
            .as_str()
            .and_then(base64_decode)
            .map(SettingValue::Bytes),
        _ => None,
    }
}

/// Convert a [`SettingValue`] into the persisted/archive typed entry
/// (TransferArchive.kt:160-173 `typedValue`).
pub(crate) fn value_to_typed_entry(value: &SettingValue) -> TypedEntry {
    let v = match value {
        SettingValue::Bool(v) => serde_json::Value::Bool(*v),
        SettingValue::Int(v) => serde_json::Value::from(*v),
        SettingValue::Long(v) => serde_json::Value::from(*v),
        SettingValue::Float(v) => serde_json::Value::from(*v),
        SettingValue::Str(v) => serde_json::Value::from(v.as_str()),
        SettingValue::StringSet(items) => serde_json::Value::Array(
            items
                .iter()
                .map(|s| serde_json::Value::from(s.as_str()))
                .collect(),
        ),
        SettingValue::Bytes(bytes) => serde_json::Value::String(base64_encode(bytes)),
    };
    TypedEntry {
        t: value.type_tag().to_owned(),
        v,
    }
}

fn value_to_toml(value: &SettingValue) -> Option<toml::Value> {
    match value {
        SettingValue::Bool(v) => Some(toml::Value::Boolean(*v)),
        SettingValue::Int(v) => Some(toml::Value::Integer(*v as i64)),
        SettingValue::Long(v) => Some(toml::Value::Integer(*v)),
        SettingValue::Float(v) => Some(toml::Value::Float(*v as f64)),
        SettingValue::Str(v) => Some(toml::Value::String(v.clone())),
        SettingValue::StringSet(items) => Some(toml::Value::Array(
            items.iter().cloned().map(toml::Value::String).collect(),
        )),
        SettingValue::Bytes(_) => None, // no clean TOML form; JSON keeps it
    }
}

fn toml_to_value(item: &toml::Value) -> Option<SettingValue> {
    match item {
        toml::Value::Boolean(v) => Some(SettingValue::Bool(*v)),
        toml::Value::Integer(v) => i32::try_from(*v).ok().map(SettingValue::Int),
        toml::Value::Float(v) => Some(SettingValue::Float(*v as f32)),
        toml::Value::String(v) => Some(SettingValue::Str(v.clone())),
        toml::Value::Array(items) => {
            let set: Option<Vec<String>> = items
                .iter()
                .map(|i| i.as_str().map(str::to_owned))
                .collect();
            set.map(SettingValue::StringSet)
        }
        toml::Value::Table(_) | toml::Value::Datetime(_) => None,
    }
}

/// Minimal standard base64 (no padding tolerance needed: we encode and decode
/// with the same implementation; kept dependency-free on purpose — the
/// transfer archive's `Base64.NO_WRAP`).
fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

pub(crate) fn base64_decode(text: &str) -> Option<Vec<u8>> {
    fn val(c: u8) -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some(u32::from(c - b'A')),
            b'a'..=b'z' => Some(u32::from(c - b'a') + 26),
            b'0'..=b'9' => Some(u32::from(c - b'0') + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let bytes: Vec<u8> = text.bytes().filter(|&b| b != b'=').collect();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for chunk in bytes.chunks(4) {
        if chunk.len() < 2 {
            return None;
        }
        let mut n = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            n |= val(c)? << (18 - 6 * i);
        }
        out.push((n >> 16) as u8);
        if chunk.len() > 2 {
            out.push((n >> 8) as u8);
        }
        if chunk.len() > 3 {
            out.push(n as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PlayerStyle, SettingId};

    #[test]
    fn round_trip_persistence() {
        let dir = std::env::temp_dir().join(format!("mochy-config-rt-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        let _ = fs::remove_file(&path);

        let store = SettingsStore::load(&path);
        store.set_PersistentQueue(false).unwrap();
        store
            .set_AudioQuality(crate::AudioQuality::HI_RES_LOSSLESS)
            .unwrap();
        store.set_PlayerStyle(PlayerStyle::TURNTABLE).unwrap();
        store
            .set_LyricsProviderOrder("kugou,lrclib".to_owned())
            .unwrap();
        store.save().unwrap();

        let reloaded = SettingsStore::load(&path);
        let snap = reloaded.snapshot();
        assert!(!snap.PersistentQueue());
        assert_eq!(snap.AudioQuality(), crate::AudioQuality::HI_RES_LOSSLESS);
        assert_eq!(snap.PlayerStyle(), PlayerStyle::TURNTABLE);
        assert_eq!(snap.LyricsProviderOrder(), "kugou,lrclib");

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn atomic_save_leaves_no_tmp() {
        let dir = std::env::temp_dir().join(format!("mochy-config-tmp-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        let store = SettingsStore::load(&path);
        store.save().unwrap();
        assert!(path.exists());
        assert!(!dir.join("settings.tmp").exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn watcher_fires_per_key_page_and_all() {
        let store = SettingsStore::with_defaults();
        let (key_rx, _k) = store.watch_key(SettingId::PersistentQueue).unwrap();
        let (page_rx, _p) = store.watch_page(SettingsPage::Playback).unwrap();
        let (all_rx, _a) = store.watch_all().unwrap();

        store.set_PersistentQueue(true).unwrap();

        match key_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap()
        {
            WatchEvent::Changed { id, value } => {
                assert_eq!(id, SettingId::PersistentQueue);
                assert_eq!(value, SettingValue::Bool(true));
            }
            other => panic!("unexpected key event: {other:?}"),
        }
        match page_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap()
        {
            WatchEvent::PageChanged { id, page, .. } => {
                assert_eq!(id, SettingId::PersistentQueue);
                assert_eq!(page, SettingsPage::Playback);
            }
            other => panic!("unexpected page event: {other:?}"),
        }
        assert!(matches!(
            all_rx
                .recv_timeout(std::time::Duration::from_secs(2))
                .unwrap(),
            WatchEvent::AnyChanged {
                id: SettingId::PersistentQueue,
                ..
            }
        ));
    }

    #[test]
    fn toml_backup_round_trip() {
        let store = SettingsStore::with_defaults();
        store
            .set_SongSortType(crate::SongSortType::PLAY_TIME)
            .unwrap();
        store.set_SkipBlendAhead(3).unwrap();
        let text = store.to_toml_string().unwrap();
        assert!(text.contains("songSortType = \"PLAY_TIME\""));

        let restored = SettingsStore::with_defaults();
        restored.import_toml(&text).unwrap();
        let snap = restored.snapshot();
        assert_eq!(snap.SongSortType(), crate::SongSortType::PLAY_TIME);
        assert_eq!(snap.SkipBlendAhead(), 3);
    }

    #[test]
    fn corrupt_file_falls_back_to_defaults() {
        let dir = std::env::temp_dir().join(format!("mochy-config-bad-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        fs::write(&path, b"{ not json").unwrap();
        let store = SettingsStore::load(&path);
        assert!(store.snapshot().PersistentQueue()); // default true
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn search_finds_by_keyword_and_filters_by_page() {
        let hits = Settings::search("crossfade", None);
        assert!(hits.contains(&SettingId::CrossmixMode));
        assert!(hits.contains(&SettingId::CrossfadeDuration));
        let audio_only = Settings::search("crossfade", Some(SettingsPage::Transitions));
        assert!(
            audio_only
                .iter()
                .all(|id| id.page() == SettingsPage::Transitions)
        );
        let none = Settings::search("zzzznope", None);
        assert!(none.is_empty());
    }

    #[test]
    fn base64_round_trip() {
        for data in [&b""[..], b"f", b"fo", b"foo", b"foob", b"fooba", b"foobar"] {
            assert_eq!(base64_decode(&base64_encode(data)).unwrap(), data);
        }
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
    }
}
