# PROGRESS — 25-phase / 9-milestone checklist

Phase ledger derived from the blueprint roadmap (ch. 14) and
`docs/PORT_SPEC.md` §10. Tick sub-items as they complete; a phase is **done**
only when every sub-item is ticked and its HANDOFF entry exists. Phase titles
for 02+ are derived from the milestone structure and may be refined by their
own prompts — update the title here when that happens (do not renumber).

**Milestones:** M0 Foundation (01–04) · M1 Innertube core (05–08) ·
M2 Audio engine (09–11) · M3 Browse UI + session (12–15) ·
M4 Player + lyrics (16–18) · M5 DSP + AudioLab (19–20) ·
M6 Modules + import (21–22) · M7 Integrations (23) · M8 Polish + ship (24–25)

---

## M0 — Foundation

### Phase 01 — Bootstrap workspace, docs, license, CI
- [x] Root files: GPL-3.0 LICENSE (full text), NOTICE (credit chain + bundled-works notes), README.md, .gitignore, rust-toolchain.toml (pinned stable)
- [x] Cargo workspace with exactly the 19 blueprint crates; each Cargo.toml + lib.rs/main.rs with responsibility doc comment + `#[cfg(test)] mod tests`
- [x] All shared deps in `[workspace.dependencies]`, versions verified on crates.io
- [x] Feature flags: `exclusive-win` (wasapi), `exclusive-linux` (alsa), `usb-uac` (rusb), `video` (ffmpeg-next, optional)
- [x] docs/PORT_SPEC.md: mission + baseline pin, 19-crate table, corrected facts, layering rules, dead-code list, route-bug decision, omissions, keymap, theme tokens, per-OS matrix, phase→milestone map, definition-of-done checklist
- [x] docs/HANDOFF.md: protocol + template + Phase 01 entry
- [x] docs/PROGRESS.md: 25-phase checklist with per-phase sub-items
- [x] .github/workflows/ci.yml: 3-OS matrix (fmt, clippy -D warnings, test) + patch artifacts + commented Phase-24 release stub
- [x] `cargo test` green workspace-wide; clippy `-D warnings` clean; artifacts emitted (patches + zip)

### Phase 02 — Settings registry + vault + net (mochy-config, mochy-vault, mochy-net)
- [x] Settings registry: 398 static keys (397 rows; the MixSortDescendingKey alias folds into albumSortDescending) + per-style dynamic factory helper, key names identical to `PreferenceKeys.kt`
- [x] Defaults table, typed accessors, search index, JSON/TOML serializer, atomic persistence (temp+fsync+rename), live per-key/per-page/global watchers
- [x] Audio-lab store mapping (`eq/AudioLabStore`) + the 22 SharedPreferences stores as config sections
- [x] mochy-vault: 12 keys over keyring 4 (Credential Manager / Secret Service / Keychain) + in-memory test backend
- [x] mochy-net: reqwest factory (rustls, gzip/brotli, H2, cookies, proxy+auth, UA policy), IpVersion pinning, DoH via hickory, moka response cache (byte-range stub) + 50 MB disk cache, offline flag, 3-attempt IO-only backoff, typed 429 short-circuit
- [x] Settings round-trip test; TransferArchive import (typed settings.json + preferences_pb fallback); HANDOFF entry

### Phase 03 — Database (mochy-db)
- [x] Schema v45 mirror: 24 tables + 3 views, identical column names; PRAGMA user_version = 1 at v45 baseline DDL
- [x] Repositories + table-watcher invalidation events (Room Flow replacement)
- [x] Android `mochy.db` importer (v35–45) incl. event-timestamp normalization decision (PORT_SPEC: normalize to UTC + zone record)
- [x] Media3 `exoplayer_internal.db` download-index read path
- [x] Migration tests on fixture `.db` files; HANDOFF entry

### Phase 04 — mochy-innertube core: client matrix, transport, auth, endpoints, paging (M1, part 1)

*(Retitled per this phase's prompt — the innertube bottom two tiers moved
forward from Phases 05–07. The original app-shell scope is displaced and
awaits its own prompt; its items are kept below, unchecked, so nothing is
lost. See HANDOFF ## Phase 04 decision 1.)*
- [x] 18-client registry verbatim from `YouTubeClient.kt` (ids/versions/UAs/flags/origins) + YTPlayerUtils roles + runtime version overrides (R1 hotfix seam)
- [x] Raw transport (InnerTube.kt port): per-request header injection, gzip, HTTP/2, IPv4/IPv6 pinning, proxy w/ auth via mochy-net, 50 MB GET disk cache, 3-attempt IO-only backoff, 429 short-circuit
- [x] Auth: cookie + origin-bound SAPISIDHASH; `Arc<RwLock<Session>>` + the settings mirror (App.kt initializeSettings + observeSettingsChanges) over mochy-vault secrets
- [x] Guest sessions: sw.js_data scrape (root[0][2], `^Cg[t|s]`), rotation on bot-check failures (BotDetectionMitigator semantics)
- [x] All 22 endpoints (incl. RYD Votes, videostats reporting, BotGuard Create/GenerateIT as dedicated clients; resumable playlist_image_upload handshake; the 7 edit_playlist actions)
- [x] Continuation paging helper: opaque tokens, seen-set dedup, 50-request caller-side ceiling
- [x] Fixture harness: record/replay redacted fixtures, offline by default, `live` feature never in CI
- [x] Acceptance: golden headers (WEB_REMIX signed-out/signed-in, VISIONOS guest), SAPISIDHASH vector, paging property tests, `cargo test -p mochy-innertube` green offline; HANDOFF entry

*Displaced app-shell items (original Phase 04 / CP M0 — not done this
phase, awaiting a phase prompt):*
- [ ] GPUI window set plan (main window first), services builder, typed AppContext as GPUI Global
- [ ] Sidebar navigation, Route enum + per-tab stacks (back = pop within tab; re-tap = pop to root + scroll top)
- [ ] Theme tokens (dark-only, chrome 0x131313, hairline + grain, Inter seven weights, -0.05em semibold)
- [ ] `mood_and_genres` route registered properly (PORT_SPEC §4.1)
- [ ] M0 checkpoint: CI green 3 OS; imported DB opens; window with sidebar + routing + tokens

## M1 — Innertube core

### Phase 05 — mochy-innertube parsers + YouTube facade + contract fixtures (M1, part 2)

*(Retitled per this phase's prompt — the ledger's original Phase 05
(transport) had already landed early: mochy-net in Phase 02, the
InnerTube transport + guest sessions in Phase 04. The parser/facade
work this prompt assigned is done here; see HANDOFF ## Phase 05.)*
- [x] mochy-net: reqwest factory (rustls), proxy SOCKS/HTTP + auth, IPv4/IPv6 pinning, DoH, 50 MB disk response cache, offline flag (pulled forward into Phase 02 by its task prompt; data-saver gate remains — it reads the connectivity observer, Phase 05)
- [x] InnerTube transport port: per-request header injection, gzip, HTTP/2, 3-attempt exponential backoff on IO errors, 429 short-circuit (done in Phase 04 by its task prompt — `InnerTubeTransport` + the 22-endpoint layer)
- [x] Guest visitor-data scraping + rotation (`BotDetectionMitigator` semantics) (done in Phase 04 — `guest.rs` + the mirror's scrape hooks)
- [x] DTO layer (serde) for search/browse/next/player/queue/suggestions/transcript/account/playlist ops/like-subscription responses; comments stack + LibraryAlbumsPage + AddItemYouTubePlaylistResponse skipped as dead (PORT_SPEC §4)
- [x] 21 parsers + 2 glue files (`PageHelper`, `BrowseResult`) as pure functions DTO → domain; NewPipe.kt deferred to Phase 08 (extractor bridge, not a parser)
- [x] YouTube facade (YouTube.kt, 1,814 lines): search variants + filters, home/album/artist/playlist/next/radio/queue, suggestions, like/subscribe, library add/remove, playlist CRUD (50-per-batch, resumable thumbnail upload), history, transcripts, timedtext, videostats, RYD media info — 50-request ceiling through `ContinuationPager`; coverage table in HANDOFF
- [x] Contract fixture corpus (24 synthetic fixtures, `scripts/gen_fixtures.py`) + parser tests over the corpus + serde round-trips + offline facade wire tests; HANDOFF entry

### Phase 06 — Models, parsers, browse/search (mochy-innertube)
- [x] kotlinx-serialization DTO equivalents; 21 page parsers + 2 glue files (done in Phase 05 by its task prompt — `models/` + `pages/`; comments stack and LibraryAlbumsPage skipped as dead)
- [x] Continuation pagination (opaque tokens, seen-set dedup, 50-request caller-side ceiling) (the `ContinuationPager` helper — done in Phase 04; the facade's use of it remains) (the facade use landed with the Phase 05 parser work)
- [x] search/browse/album/artist/playlist/next endpoints (typed facade over the Phase 04 endpoint layer); contract tests on recorded fixtures; HANDOFF entry (done in Phase 05 — the `YouTube` facade + `tests/parsers.rs`/`tests/facade_wire.rs` corpus)

### Phase 07 — Client registry + auth (mochy-innertube)
- [x] 18-client registry (WEB…VISIONOS; ANDROID_NO_SDK excluded as dead) (done in Phase 04 — verbatim port + roles; ANDROID_NO_SDK registered but unwired)
- [x] Cookie auth + SAPISIDHASH (SHA-1 of `time SAPISID origin`, origin-bound) (done in Phase 04 — `auth.rs` + transport injection + mirror; the vault holds the secrets)
- [ ] wry login window with cookie extraction; account menu (name/email/handle/photo) over the Phase 04 endpoint; sign-out forgets keys; HANDOFF entry

### Phase 08 — Stream resolution (mochy-innertube) — CP M1
- [ ] Client walk from VISIONOS through logged-in prefix + fallbacks; skip loginRequired when signed out
- [ ] Format ranking (quality ladder AUTO/LOW/MEDIUM/HIGH, codec rank, sample rate), ranged-GET validation (206)
- [ ] EJS n-solver in rquickjs; PO tokens via rustypipe-botguard (session→player, per-video→URL); stale-splice eviction
- [ ] NewPipeExtractor-needed parts re-implemented (STS, last-resort extraction) studying rustypipe
- [ ] M1 checkpoint: signed-out search/browse/album/artist/playlist parity; stream URL validates; HANDOFF entry

## M2 — Audio engine

### Phase 09 — Decode pipeline (mochy-audio)
- [ ] symphonia decode (Opus via adapter, AAC/MP4, WebM/MKV, Ogg, FLAC, MP3, ALAC, WAV/PCM)
- [ ] Ranged byte cache (512 KB subranges, LRU), itag/content-length stale-splice eviction
- [ ] Encoder-delay/gapless sample accounting for MP4/AAC; HANDOFF entry

### Phase 10 — Player core + queue (mochy-player)
- [ ] PlayerCore: one output stream, N mixer voices, gain automation (equal-power + s-curve by played position), PlaybackClock
- [ ] QueueMachine: 7 queue types, preload-first instant start, manual anchoring, runway (≥5 upcoming, generation token), shuffle surgery, start-radio tail swap, automix fallback chain
- [ ] ErrorPolicy enum taxonomy (offline skip, provider fallback, 403 refresh, 416 retry-at-position, backoff, countdown skip, circuit breaker)
- [ ] Queue persistence (JSON snapshot, same triggers + fingerprint), sleep timer, SongTuning; property tests; HANDOFF entry

### Phase 11 — Output routing (mochy-audio/mochy-player) — CP M2
- [ ] OutputRouter: shared cpal stream; WASAPI-exclusive (`exclusive-win`), ALSA `hw:` (`exclusive-linux`); exactness verified before use; mid-song fallback
- [ ] Device enumeration + hotplug re-open at position; volume curve + duck setting
- [ ] M2 checkpoint: radio plays end-to-end with queue semantics; HANDOFF entry

## M3 — Browse UI + session

### Phase 12 — Design system (mochy-ui-kit)
- [ ] Theme tokens + Inter seven weights, palette service (four extractor behaviors)
- [ ] List/grid items, sheets/popovers/context menus, toasts (TopToaster), shimmer (Bones), chips, index rail (script-aware collation)
- [ ] Glass post-pass (dual-Kawase) + flat-scrim reduce-effects fallback; seek bar family; HANDOFF entry

### Phase 13 — Browse screens (mochy-ui-browse)
- [ ] Home (hero carousel, listening-signal refresh, content filters), search hub + suggestions + results, library hub (6 tabs, A-Z rail, list/grid toggle)
- [ ] Album/artist/playlist pages (3 flavors, drag reorder), history, stats/recap, genre/browse drill-downs (+ `mood_and_genres` proper registration)
- [ ] Skeleton/empty/error states; Ctrl+K search focus; F5/Ctrl+R reload; HANDOFF entry

### Phase 14 — Media session + tray (mochy-media)
- [ ] MPRIS (zbus + mpris-server), SMTC (WinRT), Now Playing (macOS); like/radio custom entries
- [ ] Tray + muda menus, hardware media keys, notifications (per-OS adapters), `mochy://` deep links, single instance (interprocess)
- [ ] YouTube-URL deep-link router (MainActivity 1597-1684 semantics); HANDOFF entry

### Phase 15 — Downloads (mochy-downloads) — CP M3
- [ ] Job queue (parallelism 3), itag-stability resume guard, auto-download rules (on-like, per-playlist watermarks)
- [ ] Offline playback from download cache (full-span coverage check); LRU player cache (1024 MB default, -1 unlimited)
- [ ] Export remux + vorbis tagging + cover/.lrc/.ttml sidecars; storage placement + mover
- [ ] M3 checkpoint: daily driver; HANDOFF entry

## M4 — Player + lyrics

### Phase 16 — Player UI (mochy-ui-player)
- [ ] Player dock (bottom panel, 0..1 progress morph, collapsed mini-player), seek bar family, transport styles
- [ ] Queue page (sections, drag reorder, swipe→row actions), action menu (25+ actions), device output sheet, stream info
- [ ] PlaybackClock consumers (lyrics, fills, ticker); flagship style first; HANDOFF entry

### Phase 17 — Lyrics engine (mochy-lyrics)
- [ ] Provider trait + parallel fan-out (35 s ceiling) + tiered grading (script, language, sync kind, duet, order; Chinese-source tiering; auto-captions last)
- [ ] 10 network providers + 3 YouTube sources + embedded tags (lofty); manual picker, refetch, USER_EDITED
- [ ] Parsers: LRC/TTML/KRC/QRC/YRC/ASS/SRT; word-timing repair; lyrics cache sentinel + generation wipe; HANDOFF entry

### Phase 18 — Karaoke + romanization + translation — CP M4
- [ ] Karaoke renderer: pre-laid-out lines, clipped sung-run paint, word sweep, bloom pass optional, ruby/romanization/translation dots, duet alignment, RTL, tap-to-seek, follow logic
- [ ] Romanization: vibrato (JP) + pinyin (ZH) + Hangul decomposition + 7 Cyrillic tables + Devanagari/Gurmukhi, per-line language detect
- [ ] Translation: DeepL/Mistral/OpenRouter/keyless Google/YouTube-caption alignment; persisted columns
- [ ] Auto-sync DSP (vocal-activity cross-correlation, fingerprint matcher); M4 checkpoint: lyrics parity; HANDOFF entry

## M5 — DSP + AudioLab

### Phase 19 — DSP graph (mochy-audio)
- [ ] Processor chain in upstream order: EQ biquads + always-on limiter → Stage → spatial → reverb → loudness → 8D → silence detect → crossmix renderer → skip-silence; f32; atomic param swap
- [ ] Transplanted Android unit tests 1:1 (SincResampler, AutoEq fit, processor math)
- [ ] Speed/pitch (rubato or own WSOLA), normalization (YT loudness clamp + ReplayGain fold); HANDOFF entry

### Phase 20 — Stage + crossmix + AudioLab — CP M5
- [ ] Stage C++ → Rust port (f32 I/O, HRTF room, stem splitter, wall echoes, late tail)
- [ ] Crossmix: offline analyzer (mono downmix + voice-band energy), planner (27 styles), renderer (beat-locked, echo/flanger/gate/spinback, tempo ramp, joint limiter)
- [ ] Exclusive output bit-exact verification (Win/Linux); AutoEq repository + Adam re-fit
- [ ] mochy-ui-settings AudioLab pages (EQ graph editor, meters); M5 checkpoint: DSP parity; HANDOFF entry

## M6 — Modules + import

### Phase 21 — Module system (mochy-modules)
- [ ] Module trait (mirrors `modules/api` MochyModule.kt v3) + Host (blocking HTTP, storage, sandbox)
- [ ] Registry: built-ins compile-in (Monochrome, JioSaavn; Apple Music = P3 omitted), JSON web/local tiers; enable/pick/order persistence
- [ ] SourceModules routing (parallel, 12 s budget, 10-min failure memory, data-saver step-down)
- [ ] Skin DSL parser (17 look settings, player/bar/page DSL, keyframes, audio-reactive clocks) + route takeover with protected list; HANDOFF entry

### Phase 22 — Importer (mochy-importer) — CP M6
- [ ] Spotify/Apple/Tidal account import (wry capture or manual token), link + CSV import (TuneMyMusic/Exportify mapping)
- [ ] Local file import (lofty tags, sidecars, album grouping, LOCAL_ ids), shared-audio receive via file association
- [ ] TrackMatcher port; import sessions + undo + resync; TransferArchive read/write; YT two-way sync + pending-like reconciliation
- [ ] M6 checkpoint: full library migration from an Android device; HANDOFF entry

## M7 — Integrations

### Phase 23 — Integrations + link (mochy-integrations, mochy-link) — CP M7
- [ ] Discord presence (OAuth PKCE + gateway WS, external assets, 19 presence settings)
- [ ] Last.fm + Libre.fm scrobbling (retry queue, love-on-like, timing rules)
- [ ] Chromecast CASTv2 (port foss renderer verbatim) + DLNA/UPnP (SSDP + SOAP + DIDL-Lite)
- [ ] Canvas providers (Apple/Tidal/artist-video; Spotify + Musixmatch omitted by decision); Shazam recognition (opt-in)
- [ ] Phone-TV link, both roles, byte-compatible (mDNS, ECDH pairing, HMAC HTTP, gzipped SQLite mirror, ranged streaming, TV profiles)
- [ ] M7 checkpoint: interop against real counterparts incl. an Android device; HANDOFF entry

## M8 — Polish + ship

### Phase 24 — Remaining styles, Studio, accessibility, release pipeline
- [ ] Remaining player styles (turntable, video, iPod, poster, timeline, lyrics-stage, bedside clocks) + Player Studio + share posters + recap story
- [ ] Settings search + highlight flash + recommendations; accessibility pass (AccessKit, lyrics live regions)
- [ ] Release pipeline: cargo-dist + axoupdater (package-manager builds disable in-app update), MSI/deb/AppImage/Flatpak/dmg, signing + notarization; enable the release-workflow stub
- [ ] HANDOFF entry

### Phase 25 — Definition-of-done audit — CP M8
- [ ] Parity harness green on the device farm (1 Windows, 2 Linux — PipeWire + plain ALSA, 1 macOS)
- [ ] P0/P1 sweep of the blueprint ch. 5 matrix; P2 implemented-or-deferred ledger
- [ ] Omissions audit (PORT_SPEC §5 remains deliberate and visible); completion report + HANDOFF entry
