# Mochy Music for Desktop — Port Specification (PORT_SPEC)

**Status:** authoritative working spec, distilled from `docs/BLUEPRINT.pdf`
(the 61-page engineering blueprint — where this file and the blueprint
disagree, the blueprint wins and the disagreement must be logged in
`docs/HANDOFF.md`).

**Maintained by:** each phase updates this file when it makes a decision that
later phases depend on. Phase protocol and per-phase logs live in
`docs/HANDOFF.md`; the phase checklist lives in `docs/PROGRESS.md`.

---

## 1. Mission and baseline pin

The mission is a complete, behavior-preserving port of the **Mochy Music**
Android application to a native desktop application in **Rust with GPUI** on
**Windows, Linux and macOS**.

| | |
|---|---|
| Parity baseline (single source of truth) | `github.com/realidkroo/Mochy-Music` @ **commit `bde01cb4`** |
| Baseline version | **v8.4.2 build#1271**, **versionCode 73** |
| Upstream license | GPL-3.0 (plain v3 text; this port is GPL-3.0-only) |
| Later Android releases (v8.4.3+ / HEAD) | NOT the target — consult HEAD only to disambiguate; deltas triaged post-parity |
| Parity interpretation | user-visible functionality and data, not pixel equality; desktop interaction conventions allowed; no feature silently dropped |
| Lineage | Mochy → ViviMusic → Metrolist → InnerTune/ViMusic (credit chain preserved in `NOTICE`, README and the About screen) |

**Non-negotiables (all phases):**

1. **GPL-3.0.** Keep `LICENSE` + `NOTICE` with the credit chain intact.
2. **No telemetry, no analytics**; crash logs are local-only.
3. `cargo test` green and `clippy -D warnings` clean on touched crates before
   any phase finishes.
4. Port behavior, not code style; cite the upstream file (and line range when
   useful) in a comment for every ported behavior. If a task prompt and
   upstream code conflict, **upstream wins** and the conflict is logged.
5. `docs/BLUEPRINT.pdf` is the authoritative full spec; this file is the
   distilled contract.

---

## 2. Verified facts about the baseline (authoritative corrections)

These figures were cross-verified against the pinned commit and **override any
conflicting number** in the blueprint body or in older agent reports:

- **22 Gradle modules** (1 app + 16 providers + 5 module libraries;
  `settings.gradle.kts`).
- **1,296 Kotlin files** (281,624 LOC) — 1,320 including `.kts`.
- **598 UI files / 227 screens**.
- **11 player styles** (PlayerStyle enum): flagship family (= flagship +
  classic), turntable, full video, iPod Classic emulation, poster, timeline,
  lyrics-stage, two bedside clocks, module skins.
- **398 static preference keys** (standard declaration pattern in
  `constants/PreferenceKeys.kt`) **plus a dynamic per-style key factory**
  (≈400 total).
- **14 hand-written DB migrations; only 35→45 is contiguous** (older upgrade
  paths are destructive). Schema version 45 (`MusicDatabase.kt` line 110).
- **3 view-sorted tables** (`sorted_song_artist_map` and friends — ordering
  must move into consuming queries; SQLite does not guarantee view ordering).
- **yt-dlp made VISIONOS its default client and dropped ANDROID_VR**
  (2026.08.19) — VISIONOS remains the app's MAIN_CLIENT
  (`utils/YTPlayerUtils.kt`); treat client churn as a live risk, not settled.
- **Crossmix analyzer input = mono downmix + voice-band energy** (not
  mid/side decomposition).
- **Innertube module = 106 files** (`pages/` = 21 parsers + 2 glue files);
  `YouTube.kt` = 1,814 lines, `YTPlayerUtils.kt` = 1,241 lines.
- **22 SharedPreferences stores** (the transfer archive carries 6).
- **10 network lyrics providers + 3 YouTube sources**.
- **Settings index ≈ 100 entries**.
- **55 locales.**

Additional verified anchors used throughout the port:
single-activity Compose app; ~70 registered navigation destinations in
`ui/screens/NavigationBuilder.kt`; 53 ViewModels (2 dead); 17 root
CompositionLocals; 18 emulated YouTube clients; 22 innertube endpoints;
24 tables + 3 views in SQLite; 12 vault keys; 11-processor audio chain;
libmochystage = 979 `.cpp` lines (1,328 with headers); local ports 47801-47803
for the LAN protocol.

---

## 3. Workspace layout (19 crates — FIXED)

Crate seams follow Android subsystem boundaries, not Kotlin classes. **Do not
add, remove, rename or repurpose members** without a recorded HANDOFF
decision. Responsibility texts below are copied verbatim into each crate's
top doc comment.

| # | Crate | Type | Responsibility | Absorbs (upstream Android source @ bde01cb4) |
|---|-------|------|----------------|----------------------------------------------|
| 1 | `mochy` | bin | GPUI entry, window set (main, TV mode, floating lyrics, webview children), services builder, action/keymap registry, deep links, launch gate, single-instance | `App.kt` staged init, `MainActivity.kt` (NavHost + bottom nav + player sheet + deep links 1597-1684), `NavigationBuilder.kt`, `ui/launch/LaunchIntro.kt`, CompositionLocal fabric → typed AppContext |
| 2 | `mochy-ui-kit` | lib | Theme tokens + Inter, list/grid items, sheets/popovers/context menus, toasts, shimmer, glass layer, seek bars, chips, index rail | `ui/component` (105 files) + `ui/menu` (26) + `ui/theme` (8) |
| 3 | `mochy-ui-browse` | lib | Home/search/library/album/artist/playlist/stats screens and their state entities | `ui/screens` minus settings (HomeScreen 1,926 lines, SearchScreen, library/, artist/, playlist/, recap/, genre+browse, SectionDetail, History) |
| 4 | `mochy-ui-player` | lib | Player dock, lyrics page, queue page, style platform, share posters | `ui/player` (186 files): BottomSheetPlayer (2,302 lines), LyricsScreen + LyricsGlowLine (2,474 lines), QueueScreen, styles/ (11), ipod/, bedside/, studio/, share/ |
| 5 | `mochy-ui-settings` | lib | Registry-driven settings UI, AudioLab pages, setup wizard, diagnostics | `ui/screens/settings` (72 files) + audiolab/ + equalizer/ + setup/ (17) + devspace/ diagnostics |
| 6 | `mochy-player` | lib | PlayerCore mixer + voices, QueueMachine, Transport fades, ErrorPolicy, sleep timer, SongTuning | `MusicService.kt` logic (5,667 lines, decomposed) + `playback/queues` (7 queue types) + SleepTimer + SongTuning + PlayerConnection facade |
| 7 | `mochy-audio` | lib | Decode pipeline, DSP graph (EQ, spatial, reverb, loudness, 8D, Stage port, crossmix renderer), exclusive output router, spectrum tap | `eq/` (25 files) + `playback/audio/` (incl. pass/ exclusive + USB) + `cpp/stage` (libmochystage) + crossmix renderer + AudioSpectrum |
| 8 | `mochy-downloads` | lib | Job queue, itag-stability resume, auto-download rules, export remux/tagging | `DownloadUtil` + `ExoDownloadService` + `AutoDownloads` + `DownloadExporter` + remux (WebM→Ogg-Opus, MP4→FLAC) + `StoragePlaces`/`StorageMover` |
| 9 | `mochy-lyrics` | lib | Provider trait + fan-out/grading, parsers, romanization, translation, auto-sync DSP | `lyrics/` (34 files) + songedit lyric-file formats (.krc/.qrc/.yrc/ass/srt/lrc/ttml) |
| 10 | `mochy-importer` | lib | Account/link/CSV/local importers, TrackMatcher, TransferArchive, YT sync engine | `importer/` (42 files) + `utils/SyncUtils` (YT two-way sync) |
| 11 | `mochy-modules` | lib | Module trait + registry, JSON web/local modules, skin DSL parser, route takeover model | `modules/api` + `modules/bridge` (MODULE_API_VERSION 3) + app `modules/` (29 files: ModuleRegistry, SourceModules routing, skin DSL 14 files) |
| 12 | `mochy-link` | lib | Pairing, LAN server/client, remote control, TV profiles — **byte-compatible protocol** | `link/` (26 files: mDNS, ECDH secp256r1 + 6-digit code, HMAC HTTP/1.1 with nonces, gzipped SQLite snapshots, ranged file streaming) |
| 13 | `mochy-integrations` | lib | Discord gateway, scrobbler, Chromecast CASTv2, DLNA, canvas providers, Shazam, Wikipedia | `discord/` (17) + `scrobble/` (5) + `recognition/` (4) + foss `cast/` (CASTv2 TLS + protobuf frames, DLNA SOAP + SSDP) + providers/{lastfm, canvas, applecanvas, artistvideo, shazamkit, wikipedia, genius} |
| 14 | `mochy-innertube` | lib | 18-client registry, endpoints, pagination, auth headers, PO tokens, EJS n-solver, stream-format ranking | `providers/innertube` (106 files) + `utils/YTPlayerUtils.kt` (1,241 lines) + `potoken/` + `cipher/` + `sabr/` |
| 15 | `mochy-db` | lib | rusqlite schema v45 mirror, repositories, invalidation events, Android-DB importer | `db/` (39 files: 24 entities, 3 views, mega-DAO 2,131 lines, 14 hand-written migrations) + di/AppModule DB + app/schemas lineage |
| 16 | `mochy-config` | lib | Settings registry (~400 keys with defaults + search metadata), audio-lab store, atomic persistence, live watchers | `constants/PreferenceKeys.kt` (1,399 lines) + `utils/DataStore` + 22 SharedPreferences stores + `eq/AudioLabStore` |
| 17 | `mochy-vault` | lib | keyring-backed secret store (12 keys) | the Android Keystore-sealed secret sub-store (innertube cookie, visitor + data-sync ids, Discord tokens, Last.fm/Libre.fm sessions, DeepL + OpenRouter keys, Monochrome token, Turnstile bypass, proxy password) |
| 18 | `mochy-media` | lib | MPRIS/SMTC/Now Playing bridges, media keys, tray, notifications | replaces MediaSession/notification code: MediaLibrarySession custom commands + browse surface, Media3 notification provider, tray + non-media channels, hardware media keys |
| 19 | `mochy-net` | lib | reqwest factory with proxy/DNS/IP-version policy, response cache, offline flag | network utils: `AppProxy` (SOCKS/HTTP + auth), `IpVersion` pinning, DoH, `OfflineMode`/`DataSaver` gates, 50 MB disk cache |

### 3.1 Layering rules (binding for all phases)

1. **Domain crates expose no GPUI and no tokio in their public APIs** where
   avoidable — they are pure logic, exhaustively testable. (`mochy-*`
   UI/app crates may use both.)
2. **Platform-specific code lives only in leaf crates** behind
   `cfg(target_os)` — never in domain crates.
3. Every Android global singleton becomes a field of **one typed services
   object** (`AppContext`) constructed at startup; no global mutable state.
4. Concurrency model: GPUI runs its own executor; a dedicated tokio runtime
   handles networking/blocking IO, bridged via channels and `cx.spawn`. The
   audio callback owns no locks (mixer pulls from lock-free rings; params are
   atomic snapshots swapped between blocks). The database is a single-writer
   connection behind a DB actor with watcher channels (invalidation events
   replace Room's Flow reactivity).
5. Feature-layer dependency direction only points downward: UI → domain →
   infrastructure. Nothing reverse-references upward except through state
   flows.
6. New external dependencies must be added to `[workspace.dependencies]`
   (root `Cargo.toml`) with a comment stating why; version verification
   against crates.io is recorded in HANDOFF.

### 3.2 Feature flags

Cargo has no workspace-level `[features]`; the four port-wide flags are
declared on `mochy-audio` (the crate owning the gated backends) and forwarded
by dependents when they gain deps:

| Flag | Gated backend | Availability |
|------|---------------|--------------|
| `exclusive-win` | `wasapi` 0.27 | Windows only — WASAPI-exclusive output |
| `exclusive-linux` | `alsa` 0.11 | Linux only — ALSA `hw:` direct output (pin must stay on cpal's alsa/alsa-sys line; see root `Cargo.toml`) |
| `usb-uac` | `rusb` 0.9 | Windows + Linux, **experimental** — userspace USB Audio Class driver; macOS not viable (IOUSBHost arbitration) |
| `video` | `ffmpeg-next` 9.1 | Optional exotic-codec/video backend; symphonia stays the default audio path |

---

## 4. Dead code — do NOT port

Identified dead/placeholder code in the baseline. Documented here so nobody
wastes a phase on it (blueprint Table 16; verified against the pin):

- `ExploreScreen` + `ChartsViewModel` + `NewReleaseViewModel`
  (`ui/screens/ExploreScreen.kt`) — not registered in nav; navigates to a
  non-existent route.
- `OnlineSearchScreen` / `LocalSearchScreen` / the `suggestions` package
  (`ui/screens/search/`) — zero call sites (legacy UIs; the Apple-chart
  suggestions sheet is never invoked).
- `top_playlist` + `cache_playlist` routes (`NavigationBuilder.kt`) —
  registered but never navigated to.
- `ComingSoonScreen` route — registered, never navigated to.
- `CrashActivity` (`AndroidManifest.xml:92`) — declared in the manifest, class
  absent from all source sets.
- `ThemeViewModel` / `LibraryMixViewModel` (`viewmodels/`) — no usages.
- Dead preference keys (`PreferenceKeys.kt`, zero readers):
  `SliderStyle`, `SquigglySlider`, `SwipeToRemoveSong`, `SeekExtraSeconds`.
- Comments provider stack (`providers/innertube` `models/comment/`) — fully
  implemented, no UI consumer.
- `NetworkConfig.kt` (`providers/innertube`) — second client factory,
  unreferenced.
- `LibraryAlbumsPage`, the `ANDROID_NO_SDK` client, and
  `AddItemYouTubePlaylistResponse` (`providers/innertube`) — models/parsers
  with no call sites.

### 4.1 Latent route bug — decided

`HomeScreen.kt:1705` navigates to the **`mood_and_genres`** route, which is
never registered in `NavigationBuilder.kt`. **Decision: register
`mood_and_genres` properly** in the port (map it to the moods/genres
drill-down screen) rather than dropping the entry point — the upstream intent
(see `FEmusic_moods_and_genres` handling) is clearly a browseable destination.

---

## 5. Documented omissions (deliberate, visible)

Out of scope by decision — recorded so completeness claims stay honest:

| Omission | Reason / substitute |
|----------|---------------------|
| Android Auto (21 files, `car/`) | No desktop analog. Optional future car/TV mode. |
| vivo OriginCard lock-screen card (`playback/OriginCard.kt`) | Reverse-engineered Android-only protocol; dropped. |
| Home-screen widgets v1 (5 receivers, `widget/`) | No cross-OS API. Substitute: tray + always-on-top mini window; OS widgets later. |
| Apple-DRM module (`modules/applemusic`, Widevine HLS) | P3 stretch goal — no desktop CDM; documented limitation, not in the v1 definition of done. |
| Musixmatch WAF webview path | Webview bypass dropped in v1 (blueprint R9); the portable app-API song-facts path is kept. |
| Spotify canvas webview source | wry-dependent token harvest; dropped in v1 (blueprint Q10); Apple/Tidal/artist-video canvas providers remain (HTTP only). |
| Music haptics (`HapticAudioProcessor`) | Dropped, or optional gamepad rumble. |
| Bluetooth codec introspection (`BluetoothCodecs.kt`) | OS handles codecs; display dropped. |
| Phone-call watcher / telephony | No telephony on desktop; becoming-noisy maps to device-removal events. |
| Module browser feed (`ModuleCatalog` empty, `ModuleFeed` FeedUrl='') | PLACEHOLDER upstream; defer until a catalog exists (P3). |

P3 items (Apple Music DRM module, module browser) are stretch goals beyond
the definition of done and are listed so the omission is a decision, not a
silent drop.

---

## 6. Desktop keymap (ships from day one)

The Android app has no keymap; the port adds one (blueprint ch. 10.4).

| Key | Action |
|-----|--------|
| `Space` | play / pause |
| `←`/`→` | seek back / forward |
| `↑`/`↓` | volume up / down |
| `N` / `P` | next / previous track |
| `L` | like current song |
| `Q` | queue current song next |
| `D` | download current song |
| `Ctrl+K` | focus search |
| `F` | fullscreen |
| `Esc` | closes the open page, then the player |
| `Tab` | respects the focus ring (AccessKit) |
| `Ctrl+R` | reload feed (replaces pull-to-refresh; home feed also self-refreshes on listening signals) |

Gesture mapping: long-press → right-click / `Shift+F10`; swipe rows → hover
actions + the shortcuts above; drag-to-open player pages stays a drag;
swipe-to-song → wheel-on-cover + media keys; double-tap-like → double-click
and `L`.

---

## 7. Theme tokens

- **Dark-only** (`ForceDarkTheme = true` upstream; the light palette was
  never finished — blueprint Q3: dark-only for v1, light theme post-parity).
- **Chrome color `0x131313`** with **hairline** borders and **grain**.
- **Inter at seven weights**; the semibold identity is **-0.05em letter
  spacing**; fonts are skin-swappable.
- Real color flows from artwork via **four palette-extractor behaviors**
  (bottom-sample dark tint, Apple-six softened set, tonal palette, glow)
  published as entities that draw code reads.
- Glass chrome becomes a compositor post-pass (dual-Kawase downsampled blur
  behind registered surface bounds; chrome tint + hairline + shadow as quads);
  the progressive variable-radius blur ports as a WGSL translation of the
  25-tap AGSL shader with the three-band planner. The upstream flat-scrim
  fallback (0.45 alpha scrim + one-pixel hairline) ships as the
  reduce-effects path.
- The 60-fps discipline: positions are read inside Element paint from the
  **PlaybackClock** interpolator, never in update logic and never triggering
  re-layout (lyrics, seek-bar fill, mini ticker, visualizers all consume the
  one clock).

---

## 8. Cross-platform matrix (summary)

| Concern | Windows | Linux | macOS | Shared abstraction |
|---------|---------|-------|-------|--------------------|
| Media session | SMTC (WinRT) | MPRIS (zbus + mpris-server) | MediaPlayer framework (MPRemoteCommandCenter) | `mochy-media` trait; like/radio as custom MPRIS menu entries |
| Exclusive output | WASAPI-exclusive (`wasapi`) | ALSA `hw:` (`alsa`) | **shared-only** (hog mode fragile — documented limitation) | OutputRouter with capability flags; exactness verified before use; mid-song fallback to shared |
| Userspace USB DAC | WinUSB via `rusb` (experimental) | usbfs ioctls, 1:1 port (experimental) | **not viable** (IOUSBHost arbitration) | `usb-uac` feature flag |
| Credential storage | Credential Manager (keyring) | Secret Service (keyring) | Keychain (keyring) | `mochy-vault` over keyring v4 |
| Webviews | WebView2 (wry) | webkit2gtk (wry) | WKWebView (wry) | wry child windows + cookie read (`cookies_for_url` incl. HttpOnly) |
| Tray + menus | tray-icon + muda | tray-icon (StatusNotifier) + muda | tray-icon (NSStatusItem) + muda | tray-icon + muda |
| Notifications | WinRT toasts (tauri-winrt-notification) | notify-rust (D-Bus) | Notification Center (mac-notification-sys) | per-OS adapter in `mochy-media` |
| Deep links | registry scheme `mochy://` | `.desktop` MimeType | CFBundleURLTypes | single-instance + URL router (interprocess) |
| Global hotkeys | RegisterHotKey | XDG GlobalShortcuts portal | MPRemoteCommandCenter / CGEventTap | keymap + per-OS registrar |
| Updater | MSI + axoupdater | package-manager builds **disable** in-app update | DMG + notarized updater | cargo-dist channels; per-OS policy |
| Audio focus/ducking | unneeded (mixer) | unneeded (PipeWire) | unneeded (CoreAudio) | optional duck setting; mix-with-others always on |

Media keys route through the session bridges; background playback is an
in-process player with a close-to-tray setting.

---

## 9. Verified dependency stack

All versions verified against crates.io **2026-11-20** and pinned once in the
root `Cargo.toml` `[workspace.dependencies]`. Full resolution was validated
by locking a temporary member that referenced every pin (1,178 packages
resolved cleanly; see HANDOFF Phase 01).

Key pins: gpui 0.2.2 + gpui-component 0.10.0 · tokio 1.54.1 · reqwest 0.13.5
(rustls/socks/cookies/brotli) · serde 1.0.229 · rusqlite 0.43.0 (bundled) +
tokio-rusqlite 0.10.0 · keyring 4.2.0 · cpal 0.18.2 · symphonia 0.6.1 +
symphonia-adapter-libopus 0.3.0 · tray-icon 0.26.1 + muda 0.21.2 + rfd 0.18.0
+ open 5.4.5 · rquickjs 0.14.0 · rustypipe-botguard 0.1.2 · wry 0.57.3 ·
image 0.25.10 + resvg 0.50.0 + dominant_color 0.4.0 + palette 0.7.7 ·
moka 0.12.16 · thiserror 2.0.21 / anyhow 1.0.104 / tracing 0.1.44 ·
rust-i18n 4.2.4 · mdns-sd 0.22.0 + axum 0.8.9 · tokio-tungstenite 0.30.0 ·
notify-rust 4.19.0 + tauri-winrt-notification 0.8.1 (+ mac-notification-sys
0.6.15) · interprocess 2.4.9 · lofty 0.25.5 · **vibrato 0.5.3** (replaces
blueprint's lindera — encoding_rs conflict with rustypipe-botguard; see
HANDOFF) + pinyin 0.11.0 · rustfft 6.4.1 · rubato 5.1.0 · proptest 1.12.0 ·
zbus 6.4.0 + mpris-server 0.11.0 + windows 0.62.2 · clap 4.6.9.

Phase 02 additions (verified on crates.io 2026-11-20, recorded in
HANDOFF ## Phase 02): `toml` 1.2.1 (the `settings!` macro's TOML backup
serializer), `paste` 1.0.15 (identifier concatenation inside the macro),
`hickory-resolver` 0.26.3 with the `https-aws-lc-rs` transport (DoH for the
Musixmatch path; aws-lc-rs shares reqwest rustls' crypto provider so no
second TLS stack is pulled).

Phase 04 additions (verified on crates.io 2026-11-21, recorded in
HANDOFF ## Phase 04): `sha1` 0.11.0 (the SAPISIDHASH login header —
YouTube's legacy scheme, not security-critical), `base64` 0.23.1 (the
get_transcript params envelope), and the `query` feature on the existing
reqwest 0.13.5 pin (0.13 moved `RequestBuilder::query` behind
serde_urlencoded; the innertube transport shapes query params like
upstream's ktor `parameter()` calls).

Phase 07 additions (verified on crates.io 2026-11-21, recorded in
HANDOFF ## Phase 07): `url` 2.5.8 (the deep-link/share parser — already
in the graph via reqwest, pinned so `mochy` names one version), `tao`
0.37.0 (the login webview's event loop; the exact line wry 0.57.3's own
dev-dependencies pin) and `cookie` 0.18 (the type wry's
`cookies_for_url` returns; same line reqwest already resolves). All
three are optional-dependency seams of the `login-window` feature except
`url`, which the always-compiled parser uses.

Phase 08 additions (recorded in HANDOFF ## Phase 08): `libc` 0.2 (the
`usb-uac` driver's platform types — libusb1-sys's FFI takes
`libc::timeval` for the event wait; already in the graph via
cpal/alsa, pinned so mochy-audio does not float it). No other new
pins: symphonia 0.6.1 + symphonia-adapter-libopus 0.3.0, cpal 0.18.2,
wasapi 0.27.0, alsa 0.11.0 and rusb 0.9.4 were all pre-pinned and are
now exercised by the decode/output/USB work.

Licensing constraints that ride along: yt-dlp EJS bundle = Unlicense (safe to
vendor); **meriyah/astring licenses need verification before vendoring**;
symphonia = MPL-2.0 (used unmodified — no file-level disclosure triggered);
no FFmpeg in the default build (optional `video` backend only).

---

## 10. Roadmap: phases → milestones (M0–M8)

Nine milestones, each closed by a parity checkpoint (CP) validated against
the Android app running side by side. The 25-phase decomposition below is the
canonical phase ledger (PROGRESS.md tracks it 1:1); phase titles for 02+ are
derived from the blueprint's milestone structure and may be refined by their
own prompts — PROGRESS.md is updated when they are.

| Phase | Milestone | Scope (crates) | Checkpoint |
|-------|-----------|----------------|------------|
| 01 | M0 | Bootstrap workspace, docs, license, CI | — |
| 02 | M0 | `mochy-config` settings registry (398 static keys + dynamic factory, defaults, search index, atomic persistence, watchers) + `mochy-vault` (12 keys over keyring) | settings round-trip |
| 03 | M0 | `mochy-db`: schema v45 mirror (24 tables, 3 views), repositories + invalidation watcher, Android `mochy.db` importer (v35–45) | imported Android DB opens |
| 04 | M0→M1 | **Retitled by its prompt:** mochy-innertube core (client registry, transport, auth, endpoints, paging) — the innertube bottom two tiers pulled forward from Phases 05–07; the `mochy` app-shell skeleton + CP M0 are displaced and await their own prompt (HANDOFF ## Phase 04 decision 1) | innertube core green offline |
| 05 | M1 | `mochy-net` (reqwest factory, proxy/DoH/IP-version, response cache, offline flag) + innertube transport (`InnerTube.kt` port) | — |
| 06 | M1 | `mochy-innertube` models + `pages/` parsers (21 + 2 glue), search/browse/album/artist/playlist | — |
| 07 | M1 | 18-client registry, auth (SAPISIDHASH, cookie, visitor data), wry login window, account menu | — |
| 08 | M1 | **Retitled by its prompt:** stream resolution landed as the Phase 06 prompt's task (client walk, format ranking, EJS n-solver, in-process PO tokens — rustypipe-botguard studied, not depended on; see HANDOFF ## Phase 06 decision 2) → **CP M1** | signed-out browse/search parity; stream URL validates (offline suites; live run behind `--features live`) |
| 09 | M2 | `mochy-audio` decode pipeline (symphonia + adapters, ranged byte cache, stale-splice eviction) | — |
| 10 | M2 | `mochy-player` PlayerCore (one mixer, N voices, PlaybackClock), QueueMachine (7 queue types, manual anchoring, runway, shuffle surgery, persistence), ErrorPolicy | — |
| 11 | M2 | OutputRouter (cpal shared + `exclusive-win`/`exclusive-linux`), device selection + hotplug → **CP M2** | radio plays end-to-end |
| 12 | M3 | `mochy-ui-kit` (tokens, Inter, components, glass + scrim fallback, shimmer, toasts) | — |
| 13 | M3 | `mochy-ui-browse` (home/search/library/album/artist/playlist/stats) | — |
| 14 | M3 | `mochy-media` (MPRIS/SMTC/NowPlaying, tray, media keys, notifications, `mochy://` deep links, single instance) | — |
| 15 | M3 | `mochy-downloads` (job queue, itag stability, auto-download rules, offline playback, LRU cache) → **CP M3** | daily driver |
| 16 | M4 | `mochy-ui-player` (dock + mini player, queue page, seek bars, action menus, PlaybackClock consumers) | — |
| 17 | M4 | `mochy-lyrics` providers (10 network + 3 YouTube + embedded), fan-out/grading, parsers, file formats | — |
| 18 | M4 | Karaoke rendering (word sweep, 60-fps discipline), romanization (vibrato + pinyin + hand-ported tables), translation, auto-sync DSP → **CP M4** | lyrics parity |
| 19 | M5 | `mochy-audio` DSP graph (EQ + limiter, spatial, reverb, loudness, 8D, speed/pitch) with transplanted tests | — |
| 20 | M5 | Stage port (HRTF room), crossmix engine, exclusive-output verification, AutoEq; `mochy-ui-settings` AudioLab pages → **CP M5** | DSP parity |
| 21 | M6 | `mochy-modules` (trait/registry, built-ins Monochrome + JioSaavn, JSON tiers, skin DSL parser, takeover model) | — |
| 22 | M6 | `mochy-importer` (accounts, links/CSV, local files, TransferArchive, YT sync) → **CP M6** | full library migration from Android |
| 23 | M7 | `mochy-integrations` (Discord, scrobble, CASTv2, DLNA, canvas, Shazam) + `mochy-link` both roles → **CP M7** | integrations interop incl. Android device |
| 24 | M8 | Remaining player styles + Player Studio + share posters + recap; settings search + highlights; accessibility pass; release pipeline (cargo-dist, installers, signing/notarization, auto-update) | — |
| 25 | M8 | Definition-of-done audit: parity harness on the device farm, P0/P1 sweep, P2 deferral ledger, signed installers → **CP M8** | completion audit |

---

## 11. Definition of done (M8 audit — 10 areas)

| Area | Done means |
|------|-----------|
| **Streaming** | Guest + logged-in playback on all 3 OSes; client matrix with PO tokens; quality ladder; error taxonomy verified by fault injection |
| **Library** | A user migrates a real Android library (songs, likes, playlists, history, settings) and finds every list, sort and filter behaving as on Android |
| **Playback** | Queue semantics (manual anchoring, runway, shuffle, persistence) pass property tests and the scripted parity suite |
| **Lyrics** | Karaoke rendering with word timing, romanization and translation at 60 fps; provider grading identical on recorded fixtures |
| **Audio** | DSP chain ports pass transplanted tests; exclusive output verified bit-exact on Windows and Linux; documented macOS limitation |
| **Desktop integration** | MPRIS/SMTC/NowPlaying, tray, media keys, notifications, deep links, single instance all working per OS |
| **Integrations** | Discord, scrobbling, casting and phone-TV link pass interop tests against real counterparts (including an Android device) |
| **Data** | Settings registry covers all keys with defaults; Android DB + TransferArchive import verified from v35–v45 fixtures |
| **Distribution** | Signed installers (MSI, deb/AppImage/Flatpak, notarized dmg) with auto-update where the platform allows |
| **Quality** | Release gates green (fmt + clippy -D + test, 3 OS); parity harness green on the device farm; no P0/P1 feature marked partial |

The port is complete when every P0 and P1 feature in the blueprint's chapter 5
matrix is implemented and verified, every P2 feature is implemented or
explicitly deferred by a recorded decision, and the omissions in §5 remain
deliberate and visible.

---

## 12. Testing strategy (summary)

Release gates on every phase: `cargo fmt --check`, `cargo clippy --workspace
--all-targets -- -D warnings`, `cargo test --workspace` — on all three OSes
(CI). Test layers in build order (blueprint ch. 15): transplanted Android
unit tests (DSP math, parsers, matchers, graders — ported 1:1 with original
fixtures), innertube contract tests over recorded redacted JSON, stream-
resolution simulation with fault injection, queue state-machine property
tests (proptest), DB migration tests on fixture `.db` files v35–v45, audio
graph offline-render comparisons against Android captures, UI snapshot tests
at fixed clock positions, the parity harness (checkpoint scripts as runnable
regressions), and protocol interop (link vs an Android device, CASTv2 vs a
real Chromecast, DLNA vs a TV).

Performance budgets carried over: cold start < 2 s; lyrics scroll without
dropped frames at 60 Hz; no audible glitch on device hotplug.
