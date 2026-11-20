# Phase 02 Audit — mochy-config, mochy-vault, mochy-net

**Audit date:** 2026-11-20
**Method:** independent harvest/diff scripts against the pinned upstream
(`constants/PreferenceKeys.kt` 1,399 lines, `utils/SecretPreferences.kt`,
`discord/PresenceSettings.kt`, `scrobble/ScrobbleTimer.kt`, all 1,083 Kotlin
files under the app package for read-site defaults). Scripts:
`scripts/audit_schema_parity.py`, `scripts/audit_defaults.py` (audit-side,
not part of the repo build).

---

## 1. mochy-config — settings registry

### 1.1 Preference harvesting — independent recount

- Upstream `constants/PreferenceKeys.kt` at the pin contains **398
  `val X = <type>PreferencesKey("name")` declarations** (205 boolean /
  142 string / 18 int / 17 float / 15 long — recounted mechanically; one
  declaration has no space before `=`, which defeats naive greps).
- **One duplicate key NAME**: `albumSortDescending` is declared by both
  `AlbumSortDescendingKey` and `MixSortDescendingKey` (the documented alias
  fold) → **397 unique key names**.
- **One dynamic factory**: `playerStyleRotatedKey(style)` →
  `booleanPreferencesKey("playerStyleRotated_${style.name}")`
  (PreferenceKeys.kt:808).
- Registry diff result: **397 rows / 0 missing / 0 extra / 0 duplicates /
  0 kind mismatches** (bool↔bool, int↔int, long↔long, float↔float,
  string↔string, enum↔string-typed upstream). Key names are byte-identical
  to the Android names — the import-compatibility contract holds.
- The in-repo count test prints `mochy-config static registry key count: 397`
  (re-run during this audit) — matches the independent count, so the
  "≥395" acceptance holds with the honest number.

**Verdict: PASS** — the "398 static keys" requirement is satisfied as
398 upstream declarations → 397 unique registry rows + the documented alias
fold + the dynamic factory (`per_style_key`, with a test pinning the
upstream format).

### 1.2 Defaults — independent audit (finding F-003, fixed)

Mechanical extraction of every upstream default read site
(`rememberPreference`/`rememberEnumPreference` positional and
`defaultValue =` forms, `[Key] ?: fallback`, `dataStore.get(Key, default)`)
covered 211 keys directly plus 36 constant-backed sites resolved by hand
(`Defaults.*` in PresenceSettings.kt, `ScrobbleTimer.DEFAULT_*`,
`DefaultThemeColor`, `DefaultCoverRadius`, `TransportFadeDefaultSeconds`,
AudioPassMode.Default = SONG_RATE on SDK≥34 — verified equal to the
registry's `enum(AudioPassMode::SONG_RATE)` for the modern-Android
population the port targets).

**24 raw flags → 11 real defects, all fixed in this audit:**

1. Ten `bool` keys whose upstream default is **`true`** but the registry had
   `false`: `songSortDescending`, `playlistSongSortDescending`,
   `likedSongSortDescending`, `artistSortDescending`,
   `albumSortDescending`, `playlistSortDescending`,
   `addToPlaylistSortDescending`, `artistSongSortDescending`,
   `showArtistVideo`, `ytmSync` (evidence: LibraryViewModels.kt:83/126/189/247,
   LocalPlaylistViewModel.kt:64, AutoPlaylistViewModel.kt:87,
   ArtistSongsScreen.kt:68, ArtistViewModel.kt:267
   `dataStore.get(ShowArtistVideoKey, true)`, ContextExt.kt:17 +
   AutoPlaylistScreen.kt:166 for ytmSync). Impact if unfixed: every
   library list sorts ascending on fresh installs, the artist-video stage is
   off, and YouTube-sync (a core account feature) defaults off.
   `addToPlaylistSortDescending` has genuinely **conflicting upstream
   sites** (`?: true` in PlaylistsViewModel.kt:38 vs `false` positional in
   AddToPlaylistDialogOnline.kt:91); resolved to `true` — consistent with
   every sibling sort key — and the conflict is recorded here.
2. `selectedThemeColor` default was `-6035826` (0xFFA3E68E — appears
   nowhere upstream); the real default is
   `DefaultThemeColor.toArgb()` = `Color(0xFF8E8E8E)` = **-7434610**
   (MainActivity.kt:570, Theme.kt:27).

Also fixed: `autoPlaylistSongSortDescending` has **zero upstream readers**
(dead key) but was registered `hidden: false`; now `hidden: true` per the
HANDOFF Phase 02 Decision 1 policy for dead keys.

A regression test (`audited_defaults_match_upstream_read_sites`) pins all
eleven corrected values plus the dead-key visibility.

### 1.3 The `settings!` macro

Verified a real typed declaration system, not a string bag: generates the
exhaustive `SettingId` enum (397 variants), `key_name()/page()/title()/
keywords()/hidden()/kind()/default_value()/from_key_name()/accessor_name()`,
`Settings::with_defaults()`, typed getters + snapshot setters on
`Settings`, broadcast setters on `SettingsStore` (`set_X` via `paste`).
Duplicate row idents are a compile error; duplicate key literals produce an
`unreachable_patterns` arm which is an error under the `-D warnings` gate —
duplicate detection is real. Search indexing (`keywords` + `title`) and
per-page filtering (`page` + `SettingsPage`) are generated and tested.

### 1.4 Persistence

Single JSON file, `{"format":"mochy-desktop-settings","version":1}` wrapper
over the transfer-archive typed-entry encoding. Atomic: temp file →
`write_all` → `sync_all` → `rename` → parent-dir `sync_all`. Load-on-start
falls back to defaults with a warning on corrupt files (tested). Concurrent
updates serialize through the store's inner lock. Round-trip + no-tmp-file
tests re-run green. Crash safety: a torn write can never replace the old
file (rename is atomic; fsync ordering correct).

### 1.5 Android import

TransferArchive `settings.json` import with typed `{t,v}` conversion, the
raw `preferences_pb` wire-walker fallback (hand-rolled, no prost), the 22
SharedPreferences stores mapped with owner citations (incl. the archive
`display` → `mochy_settings` rename and the 6 `PrefsToCarry` stores), and
the 21 login keys dropped exactly as upstream
(TransferArchive.kt:63-73 — key list matches one-for-one; the code comment
says "20", a trivial count nit recorded as F-007). The synthetic import test
converts real typed values (not just JSON parsing) and asserts zero unknown
keys + login-key exclusion.

### 1.6 AudioLabStore

Speed clamp 0.1x–8x (MinSpeed/MaxSpeed verified), log-slider companion,
presets slowed 0.85 / slowed+reverb 0.85+reverb / nightcore 1.25 (verified
against AudioFxScreen.kt:376-383), preserve-pitch + semitones
(`2^(n/12)`), EQ band table (31 Hz–16 kHz verified), ReplayGain
normalization model, clamped construction, JSON round-trip with
`encodeDefaults`. `per_style_key(style, Rotated)` reproduces the factory
format with a test.

## 2. mochy-vault

- **Inventory verified against source**: upstream
  `utils/SecretPreferences.kt:41-54` `SecretKeys` is exactly 12 keys —
  `innerTubeCookie, visitorData, dataSyncId, discordAccessToken,
  discordRefreshToken, lastfmSession, librefmSession, openRouterApiKey,
  deeplApiKey, monochromePlaybackApiToken, turnstileBypassToken,
  proxyPassword` — identical names, identical order, no invented twelfth
  key. The count of 12 is source-verified, not padding.
- `SecretBackend` trait with `KeyringBackend` (keyring 4.2, service
  `mochy_secrets` mirroring the Android Keystore alias) and
  `InMemoryBackend` (counts reads/writes; the cache test proves hit
  behavior). get/set/delete API, in-memory cache, invalidation on set/delete.
- 2560-byte cap enforced for every backend (Windows Credential Manager
  limit). No plaintext secret logging (grep: no secret values in
  `tracing`/`println` paths). No secret serialization into ordinary
  settings files — **this boundary had a gap on the config side (F-004,
  fixed there)**; the vault itself never writes files.
- Upstream parity note: `SecretPreferences.unseal` drops values the device
  cannot open (forces re-login) — mirrored by the DB importer's
  `forced_relogin: true` and the vault's absent-value-is-None semantics.

## 3. mochy-net

- **Client factory**: reqwest 0.13.5 (rustls, gzip+brotli, HTTP/2, cookie
  store), 30 s connect / 60 s read timeout (write-timeout inexpressible in
  reqwest 0.13 — documented stand-in, acceptable), system proxy by default,
  explicit SOCKS/HTTP proxy with auth (URL-scheme routing, `Proxy::all` +
  `basic_auth` — reqwest 0.13 dropped the per-scheme constructors,
  documented), IP-version pinning applied to `resolve` pins, per-client UA
  override. Default UA `Mochy (github.com/nxfu/Mochy-Desktop)` —
  **wire-verified** by a one-shot local TCP server test (as are the
  per-client override, 429 single-request, 500-no-retry).
- **Retry**: 3 attempts, 500 ms initial, ×2 — backoff sequence
  `[500ms, 1000ms]` asserted; IO errors only; 429 and offline abort before
  any wait; non-IO HTTP errors surface (500 wire test proves exactly one
  request). Cancellation-safe by construction (plain loop + sleep).
- **429**: `NetError::YouTubeRateLimited` produced by the reqwest error
  conversion and directly usable by callers; typed short-circuit verified.
- **Offline gate**: global `AtomicBool` consulted before every attempt in
  `retry_on_io_errors` (test proves zero requests while offline). All
  request paths in the crate route through the wrapper; no bypass found.
- **DoH**: hickory `https-aws-lc-rs` transport, Cloudflare 1.1.1.1,
  IPv4-only answers, `duckduckgo.com` suffix routing, system fallback —
  matches Musixmatch.kt:56-67 semantics. (Live upstream DoH behavior is not
  exercised in tests — UNVERIFIED at network level by design; config-level
  tests cover the routing table.)
- **Caching**: moka in-memory response cache with byte-range slicing stub
  (full body serves covered ranges; uncovered = miss) and the 50 MB LRU
  disk cache for innertube GETs (stable-hash filenames, header-embedded
  key, mtime re-index, eviction) — `INNERTUBE_DISK_CACHE_BYTES` = 50 MiB
  matches InnerTube.kt:114. Ranged-GET 206 re-validation is correctly
  scoped to Phase 08 (documented).

## 4. Findings

| ID | Severity | Finding | Status |
|---|---|---|---|
| F-003 | HIGH | 11 wrong registry defaults (10 bools defaulting false that are true upstream + theme color -6035826 instead of -7434610) + 1 dead key visible. | **Fixed** + regression test |
| F-004 | MEDIUM | The 12 vault-owned secret keys are registry rows and `encode()` serialized every value — a secret set through the registry would land in plain `settings.json`. Fixed: `SECRET_KEYS` exclusion in `encode()` (default "" skipped silently, real values skipped with a `warn!`), test proves no secret ever reaches the file. | **Fixed** + test |
| F-007 | LOW | Comment says "The 20 login keys"; the list (correctly) has 21 entries matching upstream. | Documented |
| F-008 | LOW | `registry.rs` header says "GENERATED by scripts-side harvester", but the generator script was never committed — the rows cannot be regenerated as-is. (This audit's independent harvest confirms the committed rows are correct.) | Documented; recommend committing the next harvester to a `scripts/` dir |
| F-011 | LOW | `save()` uses a fixed `.tmp` sibling name — two stores pointed at one file would race. Single-instance app + single store make this theoretical. | Documented |

## 5. Verdict

**Phase 02: PASS with fixes applied.** The registry is a faithful 397-key
mirror (names/types verified byte-exact; defaults now verified against 247
read sites), the vault inventory is source-verified at exactly 12 keys, and
the HTTP layer behaves as specified on the wire. Post-fix:
`cargo test -p mochy-config` 29/29, `-p mochy-vault` 6/6, `-p mochy-net`
28/28.
