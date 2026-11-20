//! # mochy-vault — keyring-backed secret store (12 keys)
//!
//! Upstream absorption (Android, pin `bde01cb4`): the Keystore-sealed secret
//! sub-store — `utils/SecretVault.kt` (AES-GCM sealing under the Android
//! Keystore alias `mochy_secrets`) wrapped by `utils/SecretPreferences.kt`,
//! whose `SecretKeys` list names EXACTLY the 12 preference keys that hold
//! everything "that would let someone else be this user"
//! (SecretPreferences.kt:40-54).
//!
//! Desktop mapping (PORT_SPEC §8): Windows Credential Manager, Linux Secret
//! Service, macOS Keychain — all through the `keyring` crate v4 facade. The
//! OS keychain replaces the Android Keystore; there is no sealed-blob form
//! on disk to mirror, and unsealable values cannot happen (a missing
//! keychain entry reads as absent, like upstream's restore-onto-another-
//! phone path where the sealed value is dropped and the user signs in
//! again).
//!
//! Layering: leaf crate, no GPUI/tokio in the public API; the keyring call
//! is synchronous (Credential Manager / Secret Service / Keychain calls are
//! all sub-millisecond local IPC).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use thiserror::Error;

/// The keyring service name (the Android Keystore alias equivalent —
/// upstream `SecretVault.Alias = "mochy_secrets"`).
pub const SERVICE: &str = "mochy_secrets";

/// The 12 secret keys, exactly as upstream `SecretPreferences.SecretKeys`
/// (utils/SecretPreferences.kt:41-54). The `name()` of each is the Android
/// DataStore key name, identical to `constants/PreferenceKeys.kt`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum VaultKey {
    /// `innerTubeCookie` — the signed-in YouTube cookie.
    InnerTubeCookie,
    /// `visitorData` — the guest session id.
    VisitorData,
    /// `dataSyncId` — the account sync id.
    DataSyncId,
    /// `discordAccessToken`.
    DiscordAccessToken,
    /// `discordRefreshToken`.
    DiscordRefreshToken,
    /// `lastfmSession`.
    LastFmSession,
    /// `librefmSession`.
    LibreFmSession,
    /// `openRouterApiKey` — lyrics translation.
    OpenRouterApiKey,
    /// `deeplApiKey` — lyrics translation.
    DeeplApiKey,
    /// `monochromePlaybackApiToken`.
    MonochromePlaybackApiToken,
    /// `turnstileBypassToken` — Monochrome's Cloudflare bypass.
    TurnstileBypassToken,
    /// `proxyPassword`.
    ProxyPassword,
}

impl VaultKey {
    /// All 12 keys, in upstream declaration order.
    pub const ALL: &'static [VaultKey] = &[
        VaultKey::InnerTubeCookie,
        VaultKey::VisitorData,
        VaultKey::DataSyncId,
        VaultKey::DiscordAccessToken,
        VaultKey::DiscordRefreshToken,
        VaultKey::LastFmSession,
        VaultKey::LibreFmSession,
        VaultKey::OpenRouterApiKey,
        VaultKey::DeeplApiKey,
        VaultKey::MonochromePlaybackApiToken,
        VaultKey::TurnstileBypassToken,
        VaultKey::ProxyPassword,
    ];

    /// The Android preference key name (import compatibility).
    pub const fn name(self) -> &'static str {
        match self {
            VaultKey::InnerTubeCookie => "innerTubeCookie",
            VaultKey::VisitorData => "visitorData",
            VaultKey::DataSyncId => "dataSyncId",
            VaultKey::DiscordAccessToken => "discordAccessToken",
            VaultKey::DiscordRefreshToken => "discordRefreshToken",
            VaultKey::LastFmSession => "lastfmSession",
            VaultKey::LibreFmSession => "librefmSession",
            VaultKey::OpenRouterApiKey => "openRouterApiKey",
            VaultKey::DeeplApiKey => "deeplApiKey",
            VaultKey::MonochromePlaybackApiToken => "monochromePlaybackApiToken",
            VaultKey::TurnstileBypassToken => "turnstileBypassToken",
            VaultKey::ProxyPassword => "proxyPassword",
        }
    }

    /// Resolve by Android key name.
    pub fn from_name(name: &str) -> Option<VaultKey> {
        VaultKey::ALL.iter().copied().find(|key| key.name() == name)
    }
}

/// Vault failures. A missing entry is NOT an error — [`Vault::get`] returns
/// `None` for it (upstream `SecretVault.open` returns null only for sealed
/// values the phone can't open; absent values read as absent).
#[derive(Debug, Error)]
pub enum VaultError {
    /// The OS keychain refused the operation.
    #[error("keychain error: {0}")]
    Keychain(String),
    /// The value is too large for the platform keychain
    /// (Windows Credential Manager caps around 2560 bytes — cookies fit,
    /// but a corrupted paste might not).
    #[error("secret too large for the platform keychain")]
    TooLarge,
}

/// Storage backend. The keyring implementation talks to Credential Manager /
/// Secret Service / Keychain; tests use an in-memory map.
pub trait SecretBackend: Send + Sync {
    fn get(&self, service: &str, account: &str) -> Result<Option<String>, VaultError>;
    fn set(&self, service: &str, account: &str, value: &str) -> Result<(), VaultError>;
    fn delete(&self, service: &str, account: &str) -> Result<(), VaultError>;
}

/// The OS keychain backend (`keyring` v4 facade).
#[derive(Debug, Default, Clone, Copy)]
pub struct KeyringBackend;

impl SecretBackend for KeyringBackend {
    fn get(&self, service: &str, account: &str) -> Result<Option<String>, VaultError> {
        let entry = keyring::Entry::new(service, account).map_err(keychain)?;
        match entry.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(keyring::Error::Ambiguous(_)) => Ok(None),
            Err(e) => Err(keychain(e)),
        }
    }

    fn set(&self, service: &str, account: &str, value: &str) -> Result<(), VaultError> {
        let entry = keyring::Entry::new(service, account).map_err(keychain)?;
        match entry.set_password(value) {
            Ok(()) => Ok(()),
            Err(keyring::Error::TooLong(_, _)) => Err(VaultError::TooLarge),
            Err(e) => Err(keychain(e)),
        }
    }

    fn delete(&self, service: &str, account: &str) -> Result<(), VaultError> {
        let entry = keyring::Entry::new(service, account).map_err(keychain)?;
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()), // idempotent
            Err(e) => Err(keychain(e)),
        }
    }
}

fn keychain(e: keyring::Error) -> VaultError {
    VaultError::Keychain(e.to_string())
}

/// In-memory backend (tests, previews, and the no-keychain CI path).
#[derive(Debug, Default)]
pub struct InMemoryBackend {
    store: Mutex<HashMap<(String, String), String>>,
    /// Count of read operations (tests assert the cache works).
    pub reads: std::sync::atomic::AtomicUsize,
    pub writes: std::sync::atomic::AtomicUsize,
}

impl InMemoryBackend {
    pub fn new() -> Self {
        Self::default()
    }
}

impl SecretBackend for InMemoryBackend {
    fn get(&self, service: &str, account: &str) -> Result<Option<String>, VaultError> {
        use std::sync::atomic::Ordering;
        self.reads.fetch_add(1, Ordering::Relaxed);
        Ok(self
            .store
            .lock()
            .expect("in-memory vault lock poisoned")
            .get(&(service.to_owned(), account.to_owned()))
            .cloned())
    }

    fn set(&self, service: &str, account: &str, value: &str) -> Result<(), VaultError> {
        use std::sync::atomic::Ordering;
        self.writes.fetch_add(1, Ordering::Relaxed);
        self.store
            .lock()
            .expect("in-memory vault lock poisoned")
            .insert((service.to_owned(), account.to_owned()), value.to_owned());
        Ok(())
    }

    fn delete(&self, service: &str, account: &str) -> Result<(), VaultError> {
        self.store
            .lock()
            .expect("in-memory vault lock poisoned")
            .remove(&(service.to_owned(), account.to_owned()));
        Ok(())
    }
}

/// The vault: get/set/delete with an in-memory read cache
/// (upstream `SecretVault.opened`, the LRU of unsealed values, so a settings
/// read doesn't hit the keystore for every secret every time —
/// utils/SecretVault.kt:31-36).
#[derive(Clone)]
pub struct Vault {
    backend: Arc<dyn SecretBackend>,
    cache: Arc<Mutex<HashMap<VaultKey, Option<String>>>>,
}

impl Vault {
    /// Open over a specific backend.
    pub fn with_backend(backend: Arc<dyn SecretBackend>) -> Vault {
        Vault {
            backend,
            cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Open over the OS keychain (Credential Manager / Secret Service /
    /// Keychain via `keyring` v4).
    pub fn open_system() -> Vault {
        Vault::with_backend(Arc::new(KeyringBackend))
    }

    /// Open over the in-memory backend (tests).
    pub fn open_in_memory() -> Vault {
        Vault::with_backend(Arc::new(InMemoryBackend::new()))
    }

    /// Read a secret; `None` when never set (or removed on this machine,
    /// which upstream treats as sign-in-again).
    pub fn get(&self, key: VaultKey) -> Option<String> {
        {
            let cache = self.cache.lock().expect("vault cache lock poisoned");
            if let Some(hit) = cache.get(&key) {
                return hit.clone();
            }
        }
        let value = self.backend.get(SERVICE, key.name()).ok().flatten();
        self.cache
            .lock()
            .expect("vault cache lock poisoned")
            .insert(key, value.clone());
        value
    }

    /// Store a secret (write-through: keychain first, then cache). Values
    /// above the Windows Credential Manager cap (~2560 bytes) are refused
    /// uniformly on every OS — upstream cookies and tokens are far smaller.
    pub fn set(&self, key: VaultKey, value: &str) -> Result<(), VaultError> {
        if value.len() > 2560 {
            return Err(VaultError::TooLarge);
        }
        self.backend.set(SERVICE, key.name(), value)?;
        self.cache
            .lock()
            .expect("vault cache lock poisoned")
            .insert(key, Some(value.to_owned()));
        Ok(())
    }

    /// Remove a secret (also on sign-out — upstream `forgetAccount` removes
    /// the preference, which unseals to dropping the keychain entry here).
    /// Idempotent.
    pub fn delete(&self, key: VaultKey) -> Result<(), VaultError> {
        self.backend.delete(SERVICE, key.name())?;
        self.cache
            .lock()
            .expect("vault cache lock poisoned")
            .insert(key, None);
        Ok(())
    }

    /// Drop the in-memory cache (after a keychain change made elsewhere, or
    /// in tests).
    pub fn clear_cache(&self) {
        self.cache
            .lock()
            .expect("vault cache lock poisoned")
            .clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    #[test]
    fn exactly_the_12_upstream_secret_keys() {
        // utils/SecretPreferences.kt:41-54
        assert_eq!(VaultKey::ALL.len(), 12);
        let names: Vec<&str> = VaultKey::ALL.iter().map(|k| k.name()).collect();
        for expected in [
            "innerTubeCookie",
            "visitorData",
            "dataSyncId",
            "discordAccessToken",
            "discordRefreshToken",
            "lastfmSession",
            "librefmSession",
            "openRouterApiKey",
            "deeplApiKey",
            "monochromePlaybackApiToken",
            "turnstileBypassToken",
            "proxyPassword",
        ] {
            assert!(names.contains(&expected), "missing vault key {expected}");
        }
        // no duplicates
        let mut sorted = names.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), 12);
    }

    #[test]
    fn set_get_delete_round_trip() {
        let vault = Vault::open_in_memory();
        assert_eq!(vault.get(VaultKey::InnerTubeCookie), None);

        vault.set(VaultKey::InnerTubeCookie, "SID=xyz").unwrap();
        assert_eq!(
            vault.get(VaultKey::InnerTubeCookie),
            Some("SID=xyz".to_owned())
        );

        vault.delete(VaultKey::InnerTubeCookie).unwrap();
        assert_eq!(vault.get(VaultKey::InnerTubeCookie), None);

        // delete of a missing key is idempotent
        vault.delete(VaultKey::InnerTubeCookie).unwrap();
    }

    #[test]
    fn reads_are_cached_until_invalidated() {
        // use the shared-backend form to observe the counters
        let backend = Arc::new(InMemoryBackend::new());
        let vault = Vault::with_backend(Arc::clone(&backend) as Arc<dyn SecretBackend>);
        vault.set(VaultKey::ProxyPassword, "hunter2").unwrap();
        assert_eq!(backend.writes.load(Ordering::Relaxed), 1);

        // set() populates the cache: repeated gets never touch the backend
        let _ = vault.get(VaultKey::ProxyPassword);
        let _ = vault.get(VaultKey::ProxyPassword);
        let _ = vault.get(VaultKey::ProxyPassword);
        assert_eq!(
            backend.reads.load(Ordering::Relaxed),
            0,
            "cache must serve repeats"
        );

        vault.clear_cache();
        let _ = vault.get(VaultKey::ProxyPassword);
        assert_eq!(
            backend.reads.load(Ordering::Relaxed),
            1,
            "cache miss hits the backend"
        );
        let _ = vault.get(VaultKey::ProxyPassword);
        assert_eq!(
            backend.reads.load(Ordering::Relaxed),
            1,
            "second read is cached again"
        );

        // a delete invalidates
        vault.delete(VaultKey::ProxyPassword).unwrap();
        assert_eq!(vault.get(VaultKey::ProxyPassword), None);
    }

    #[test]
    fn from_name_resolves_and_rejects() {
        assert_eq!(
            VaultKey::from_name("deeplApiKey"),
            Some(VaultKey::DeeplApiKey)
        );
        assert_eq!(VaultKey::from_name("notASecret"), None);
    }

    #[test]
    fn too_large_secrets_fail_early() {
        let vault = Vault::open_in_memory();
        let big = "x".repeat(3000);
        assert!(matches!(
            vault.set(VaultKey::DiscordAccessToken, &big),
            Err(VaultError::TooLarge)
        ));
    }

    #[test]
    fn secrets_of_every_key_round_trip() {
        let vault = Vault::open_in_memory();
        for (i, key) in VaultKey::ALL.iter().enumerate() {
            vault.set(*key, &format!("value-{i}")).unwrap();
        }
        for (i, key) in VaultKey::ALL.iter().enumerate() {
            assert_eq!(vault.get(*key), Some(format!("value-{i}")));
        }
    }
}
