# HANDOFF — phase log and decision ledger

This file is the port's institutional memory. Every phase **must** read this
file first and **must** append its entry before finishing. Nothing else in the
repo records why decisions were made.

Authoritative documents:

- `docs/BLUEPRINT.pdf` — the full engineering blueprint (research, feature
  matrix, deep dives, risks). Wins over everything except verified source.
- `docs/PORT_SPEC.md` — the distilled working contract (baseline pin, crate
  seams, layering rules, dead code, omissions, keymap, theme, per-OS matrix,
  roadmap, definition of done). Wins over the blueprint only where it records
  a *verified correction* (its §2).
- `docs/PROGRESS.md` — the 25-phase checklist, ticked per phase.
- The pinned upstream source: `github.com/realidkroo/Mochy-Music` @
  `bde01cb4` (v8.4.2 build#1271, versionCode 73). **Upstream behavior wins
  over any document when they conflict** — log the conflict in your entry.

## Phase protocol (binding)

1. **Before starting:** read this file, `docs/PORT_SPEC.md`,
   `docs/PROGRESS.md`, and the recent git log. Continue where the previous
   phase stopped; never redo or rework finished phases unless your task
   prompt explicitly says so.
2. **During:** follow PORT_SPEC §1 (GPL-3.0, no telemetry, upstream citation
   comments, upstream-wins conflicts) and §3.1 (layering rules). Decide; do
   not ask questions; record decisions here.
3. **Before finishing:** `cargo fmt --check`, `cargo clippy --workspace
   --all-targets -- -D warnings`, `cargo test --workspace` all green; commit
   in small reviewable units as `phase-NN(scope): ...`; emit
   `patches/` + phase zip; update this file and `docs/PROGRESS.md`.
4. **If you approach response limits:** complete the current unit, record the
   remainder under your entry's "Next steps", still emit the artifacts.

## Entry template (copy for every phase)

```markdown
## Phase NN — YYYY-MM-DD

**Status:** done | partial (what remains) | blocked (by what)

**Task:** <one-line summary of the phase task>

### Decisions
- <numbered decisions with rationale — every judgment call that a later
  phase would otherwise have to re-derive>

### Deviations
- <anything that diverges from the blueprint, PORT_SPEC or the task prompt,
  with the reason; "none" is a valid entry>

### Next steps
- <what Phase NN+1 must pick up first, in order>
```

---

## Phase entries

(Appended below, newest last. Do not reorder or rewrite past entries.)

## Phase 01 — 2026-11-20

**Status:** done (all acceptance criteria met; artifacts emitted)

**Task:** Bootstrap workspace, docs, license, CI (Milestone M0, part 1).

**Commits:** `phase-01(root)` → `phase-01(workspace)` → `phase-01(docs)` →
`phase-01(ci)` → `phase-01(handoff)`, five reviewable units on branch
`phase-01` cut from `origin/main` (base `1702d9d`, the pre-existing
"add automation doc" commit).

### Decisions

1. **Repo state.** The target repo was NOT empty: `origin/main` = `1702d9d`
   carrying `docs/BLUEPRINT.pdf` (61 pages, the authoritative full spec per
   the phase rules). `phase-01` was cut from it; BLUEPRINT.pdf is kept
   untouched and `docs/PORT_SPEC.md` was written as the distilled working
   spec (all 12 mandated sections, blueprint ch. 2–18 condensed with the
   rule-6 corrections applied verbatim).
2. **Package metadata.** `license = "GPL-3.0-only"` (upstream ships the
   plain v3 text with no or-later grant), `edition = "2024"`,
   `rust-version = "1.100"` matching the pinned toolchain, `publish = false`,
   and **Cargo.lock is committed** (application workspace — reproducible
   builds beat lockfile churn).
3. **Toolchain.** `rust-toolchain.toml` pins exact `1.100.0` (verified the
   current stable on 2026-11-20: rustc 1.100.0 0ef1bc386 2026-11-09) with
   minimal profile + rustfmt + clippy. Exact pin, not moving `stable`, so CI
   and all three OSes build the same compiler (GPUI pre-1.0 makes this
   non-negotiable — blueprint R2).
4. **Dependency pins.** All external versions verified against crates.io on
   2026-11-20 and pinned once in root `[workspace.dependencies]`
   (PORT_SPEC §9 lists them). Validation method: a **temporary workspace
   member referencing every pin was resolved via `cargo metadata` — 1,178
   packages lock cleanly** (the scratch crate was then deleted and is not
   part of the patch series; re-create it the same way to re-validate).
   Blueprint pins that matched live: gpui 0.2.2, gpui-component 0.10.0,
   tokio 1.54.1, reqwest 0.13.5, rusqlite 0.43.0 + tokio-rusqlite 0.10.0,
   keyring 4.2.0, cpal 0.18.2, symphonia 0.6.1 + adapter-libopus 0.3.0,
   tray-icon 0.26.1, muda 0.21.2, rfd 0.18.0, open 5.4.5, rquickjs 0.14.0,
   rustypipe-botguard 0.1.2, wry 0.57.3, image 0.25.10, resvg 0.50.0,
   dominant_color 0.4.0, moka 0.12.16, thiserror 2.0.21, anyhow 1.0.104,
   tracing 0.1.44, rust-i18n 4.2.4, mdns-sd 0.22.0, axum 0.8.9,
   tokio-tungstenite 0.30.0, notify-rust 4.19.0, tauri-winrt-notification
   0.8.1, interprocess 2.4.9. Verified extras the blueprint left unpinned:
   serde 1.0.229, serde_json 1.0.154, lru 0.19.0, palette 0.7.7, lofty
   0.25.5, pinyin 0.11.0, rustfft 6.4.1, rubato 5.1.0, proptest 1.12.0,
   zbus 6.4.0, mpris-server 0.11.0, mac-notification-sys 0.6.15, windows
   0.62.2, clap 4.6.9, ffmpeg-next 9.1.0, wasapi 0.27.0, rusb 0.9.4.
   reqwest 0.13 feature names verified live: `rustls` (not the old
   `rustls-tls`), `system-proxy`, `socks`, `cookies`, `brotli`.
5. **alsa pinned to 0.11.0, not 0.12.1.** cpal 0.18.2 depends on
   alsa ^0.11 (alsa-sys ^0.4); the standalone alsa 0.12 line needs
   alsa-sys ^0.6 — mixing them is a `links = "alsa"` conflict that breaks
   `exclusive-linux` builds. The pin comment in Cargo.toml states the
   constraint: **re-check alsa whenever cpal is upgraded.**
6. **vibrato 0.5.3 replaces the blueprint's lindera** (see Deviations):
   every lindera line (3.x–6.x, via lindera-dictionary) requires
   encoding_rs ^0.8.35; rustypipe-botguard 0.1.2 → deno_web 0.218.0 pins
   encoding_rs **=0.8.33** exactly. Both crates are required by the app
   (PO tokens are the P0 streaming risk mitigation; Japanese romanization
   is lyrics parity), so lindera is unresolvable in a single Cargo graph.
   Phase 18 pairs vibrato with an IPADIC dictionary build; revisit lindera
   if rustypipe-botguard ever relaxes the deno_web pin.
7. **Feature flags live on mochy-audio.** Cargo has no workspace-level
   `[features]`, so the four port-wide flags (`exclusive-win`, 
   `exclusive-linux`, `usb-uac`, `video`) are declared on the crate that
   owns the gated backends, with optional workspace deps. `video` is
   documented (PORT_SPEC §3.2) as additionally gating mochy-ui-player code
   when that crate gains deps. Contract is in PORT_SPEC §3.2.
8. **Phase ledger.** The phase→milestone map for phases 02–25 (PORT_SPEC
   §10, PROGRESS.md) was derived from the blueprint's M0–M8 milestone
   structure — only Phase 01's task text is authoritative to this phase.
   Later phase prompts may refine titles; PROGRESS.md is updated then
   without renumbering.
9. **CI patch artifacts.** The `patches` CI job generates the phase series
   itself (`git format-patch origin/main..HEAD`) and uploads it — so the
   artifact exists even for pushes made without local patch generation.
   Local `patches/` and the phase zip are gitignored build artifacts.
10. **`mochy` bin is a deliberate println stub** proving the seam builds;
    Phase 04 replaces `fn main` with the GPUI `Application::new()` shell.
    All 19 crates carry their PORT_SPEC responsibility + upstream-absorption
    doc comments and one trivial stub test each (19 tests green).

### Deviations

- **vibrato instead of lindera** (blueprint ch. 13 pick) — forced by the
  encoding_rs conflict described in Decision 6; same role (kuromoji-
  equivalent Japanese tokenizer for lyrics romanization), MIT-equivalent
  licensing, no ecosystem downside found (119k downloads, active).
- **alsa 0.11.0 instead of newest 0.12.1** (blueprint named the crate
  without a version) — forced by the cpal links-conflict (Decision 5).
- **No other deviations.** Every other blueprint pin matched the live
  registry; PORT_SPEC records two upstream-nuance updates (reqwest feature
  rename; palette now 0.7.7 where the blueprint's table showed the 0.4-era
  number for dominant_color) rather than conflicts.

### Verification (acceptance)

- `cargo test --workspace`: **19/19 green** (trivial stub tests per crate).
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- `cargo fmt --all --check`: clean. `cargo doc --no-deps`: clean.
- CI YAML parsed and structure-checked (2 jobs, 3-OS matrix).
- Full dependency-graph resolution proven via the temporary member
  (Decision 4); nothing in the committed tree references a heavy dep yet,
  so the committed Cargo.lock contains only members + the four optional
  audio backends (71 packages).

### Next steps (Phase 02 — settings registry + vault)

1. Read PORT_SPEC §3 rows `mochy-config`/`mochy-vault`, §10 Phase 02, and
   blueprint ch. 9.1 (the `settings!` macro sketch; blueprint Q9 says
   macro for v1, codegen later if search metadata grows).
2. Harvest all 398 static keys + the per-style dynamic factory from
   `constants/PreferenceKeys.kt` (1,399 lines) — **key names must stay
   identical for Android import compatibility**.
3. Wire `mochy-config` deps: serde/serde_json/tracing/thiserror from the
   workspace table (thiserror import in a domain crate stays internal —
   no tokio/GPUI in the public API, PORT_SPEC §3.1).
4. `mochy-vault`: 12 keys (PORT_SPEC §3 row 17) over keyring 4.2.0's
   v4 facade; fall back to a local encrypted file ONLY if keyring is
   unavailable, and record that decision here.
5. Settings round-trip test = the Phase 02 checkpoint; tick PROGRESS
   Phase 02 items; append the Phase 02 entry here.


## Phase 02 — 2026-11-20

**Status:** done (all acceptance criteria met; artifacts emitted)

**Task:** mochy-config (settings registry) + mochy-vault + mochy-net
(Milestone M0, part 2 — the phase prompt pulled mochy-net forward from
Phase 05; see Deviations).

**Commits:** `phase-02(config)` → `phase-02(vault)` → `phase-02(net)` →
`phase-02(docs)` on branch `phase-02` cut from `origin/main`
(base `6618f95`).

### Harvest summary (acceptance numbers)

- **Static keys harvested: 398 upstream declarations → 397 registry rows.**
  Upstream `constants/PreferenceKeys.kt` has 398
  `val X = …PreferencesKey("name")` declarations + 1 dynamic factory
  (line 808). `MixSortDescendingKey` (line 420) re-uses the literal
  `"albumSortDescending"` (the same key as `AlbumSortDescendingKey`), so it
  is an alias and folds into one row — hence 397 unique key names. The
  count test prints `mochy-config static registry key count: 397` and
  asserts ≥ 395. Types: 207 bool / 18 int / 15 long / 17 float / 142 string
  declarations upstream; enum-typed rows carry the Kotlin variant-name set
  (58 registry enums with upstream declaration order).
- **The 12 vault keys** (read from `utils/SecretPreferences.kt:41-54`,
  exactly): `innerTubeCookie`, `visitorData`, `dataSyncId`,
  `discordAccessToken`, `discordRefreshToken`, `lastfmSession`,
  `librefmSession`, `openRouterApiKey`, `deeplApiKey`,
  `monochromePlaybackApiToken`, `turnstileBypassToken`, `proxyPassword`.
- **Crate versions added to the workspace table** (all verified against
  crates.io on 2026-11-20): `toml` 1.2.1, `paste` 1.0.15,
  `hickory-resolver` 0.26.3 (`https-aws-lc-rs` feature — aws-lc-rs is
  reqwest 0.13.5's rustls crypto provider, so no second TLS stack).

### Decisions

1. **Registry row grammar is strict** (`Ident: "androidKey" =>
   kind(default), page: P, title: "..", keywords: "..", hidden: bool;`).
   The committed `registry.rs` is **generated** by a phase-side harvester
   (`scripts`-side, not part of the repo build) that walks the pinned
   upstream tree: 399 `PreferencesKey(` call sites parsed, defaults
   harvested from every read-site pattern (`rememberPreference`,
   `rememberEnumPreference`, `preference(context,…)`/`enumPreference`,
   `[Key] ?: …`, `[Key].toEnum(…)`, `get(Key, …)` — 233 mechanical hits)
   plus dedicated reads (PresenceSettings.kt, LyricsReadings.kt,
   MusicService.kt, ScrobbleTimer.kt, LyricsHelper.kt, LarpMode.kt,
   SongSwipeBox.kt, GlassKit.kt, Theme.kt `DefaultThemeColor`,
   CrossmixSettingsScreen `AutoCrossfadeDurationSeconds = 5f`). Keys with
   no read site are state markers/dead keys: registered with type-zero or
   enum-first defaults and `hidden: true` so the settings UI never shows
   them (the 4 PORT_SPEC §4 dead keys are `hidden`, not dropped — they
   still round-trip Android imports).
2. **Row idents are the Kotlin val names minus `Key`** (UpperCamelCase,
   `#[allow(non_snake_case)]` on the generated accessors): callers write
   `snap.PersistentQueue()` / `store.set_PlayerStyle(…)` — the same shape
   as the Kotlin read/write sites. Setter names are concatenated through
   `paste` (macro_rules cannot build identifiers).
3. **`SettingValue` = the transfer-archive typed-JSON model** (`{t, v}`
   with `b/i/l/f/d/s/ss/bytes`). Our own persisted file uses the same
   encoding under a `{"format":"mochy-desktop-settings","version":1}`
   wrapper, so the desktop settings file and an Android `settings.json`
   merge without a conversion layer. Persistence is atomic
   (temp + fsync + rename + dir fsync); corrupt files warn + fall back to
   defaults rather than refusing to start.
4. **Watchers are std mpsc, not tokio broadcast**: domain crate, no tokio
   in mochy-config's public API (PORT_SPEC §3.1). `watch_key` /
   `watch_page` / `watch_all` return `(Receiver, WatchGuard)`; the guard
   unsubscribes on drop; senders are id'd (mpsc `Sender` has no Eq).
   Persistence is explicit `save()` — later phases choose the flush policy
   (upstream DataStore writes through asynchronously, which is a debounce,
   not a contract).
5. **AudioLab is a typed section, not 12 registry rows**: upstream stores
   JSON blobs under `audio_lab` SharedPreferences keys (`eq`, `reverb`,
   …). `mochy-config` reproduces the data classes + defaults
   (AudioLabStore.kt, ReverbSettings.kt, StageSettings.kt) with
   per-blob parse-fallback-to-default, the 0.1x–8x speed clamp, 8D
   normalization, the slowed/slowed+reverb/nightcore presets, and
   `encodeDefaults=true` round-tripping. Stage's object graph stays raw
   JSON values — Phase 20 owns that model.
6. **22 SharedPreferences stores mapped with owner citations**
   (`stores.rs`), including the archive's `display` → `mochy_settings`
   rename and the 6 `PrefsToCarry` stores; per-module `module_<id>` stores
   are documented as dynamic (owned by Phase 21).
7. **Android import drops login keys, like upstream**
   (TransferArchive.kt:63-73 — 21 keys by name, recounted from source).
   A fresh store still shows the registry default ("") for those rows —
   upstream's target DataStore keeps whatever it already had; the test
   asserts the archive's secret never lands. The raw fallback
   `preferences_pb` reader is a hand-rolled protobuf-wire walker (no prost
   dependency), with an encoder for round-trip tests and future re-export.
8. **Vault backends are a trait** (`SecretBackend`): `KeyringBackend`
   (keyring 4.2; service alias `mochy_secrets` mirroring the Android
   Keystore alias) and `InMemoryBackend` (tests; counts reads/writes so
   the cache test proves hit behavior). The 2560-byte cap (Windows
   Credential Manager's practical limit) is enforced in `Vault::set` for
   every backend. No encrypted-file fallback was implemented: keyring 4
   covers all three OSes (the HANDOFF Phase 01 suggestion of a file
   fallback is unnecessary — no deviation needed).
9. **mochy-net API is async (tokio)** — infrastructure crate; the app's
   dedicated runtime owns networking (PORT_SPEC §3.1 rule 4). The retry
   decision function is pure/sync so the backoff taxonomy is testable
   without a runtime.
10. **429 mapping**: `NetError::YouTubeRateLimited` is produced both by
    reqwest error conversion and by callers checking `status()`; the
    policy aborts on it before any wait. The wire test proves exactly one
    request goes out.
11. **Proxy**: reqwest 0.13 dropped `Proxy::http/socks5` constructors in
    favor of `Proxy::all(url)` — HTTP and SOCKS both ride the URL scheme
    (`http://`/`socks5://`), with `basic_auth` for the
    `Proxy-Authorization` (HTTP) and SOCKS-auth (upstream JVM
    authenticator) cases.
12. **DoH**: reqwest 0.13 exposes no custom-resolver hook, so
    `DohResolver` (hickory, Cloudflare 1.1.1.1, IPv4-only answers,
    duckduckgo.com routing, system fallback — Musixmatch.kt:56-67) feeds
    `ClientSpec::resolve(…)` pins; IpVersion filtering reuses the upstream
    `ifEmpty` fallback and is applied to the pinned list.
13. **https_only was NOT set** on the factory (upstream talks plain HTTP
    to some module endpoints; forcing TLS-only would break them and broke
    the local test server during development).

### Deviations

- **mochy-net moved from Phase 05 into Phase 02** — the Phase 02 task
  prompt includes it verbatim; PROGRESS.md Phase 05 is annotated (the
  InnerTube transport port + data-saver gate stay in Phase 05). This is a
  prompt-driven resequencing, not a spec change.
- **397 rows vs "~398"**: the alias fold (Decision 1 explanation). The
  task's "≥395" acceptance holds with margin; the count is printed by the
  test.
- **reqwest 0.13.5 has no `write_timeout`** (upstream sets 60 s read +
  60 s write): `read_timeout(60s)` stands in; documented in client.rs.
  Not observable for the app's workloads.
- **No linter-provable citation comment on every one of the 397 rows**
  (rule 4): rows cite the Kotlin key name verbatim (the strongest
  citation — it IS the upstream identifier), the file-level header cites
  PreferenceKeys.kt, and the enums carry per-enum line references. A
  397-comment wall would have made the generated file unreadable; the
  key-name identity is machine-checkable (the import test proves it).

### Verification (acceptance)

- `cargo test -p mochy-config -p mochy-vault -p mochy-net`:
  **61 tests green** (27 + 6 + 28). Key-count test prints 397 (≥395 ✓).
- Registry import test converts a synthetic TransferArchive JSON into
  typed settings with **zero unknown keys** and drops the login key.
- `cargo test --workspace`: 37 test binaries, all green (includes the 16
  untouched Phase-01 stub tests). `cargo fmt --all --check` clean.
  `cargo clippy --workspace --all-targets -- -D warnings`: 0 errors.
  `cargo doc --no-deps` on the three crates: clean.
- Wire-level tests (local one-shot TCP server) prove the default UA, the
  per-client UA override, 429 → exactly one request, and 500 → no retry.

### Next steps (Phase 03 — mochy-db)

1. Read PORT_SPEC §3 row `mochy-db`, §10 Phase 03, and blueprint ch. 9.2
   (DB actor) — schema v45 mirror with identical column names, 24 tables
   + 3 views, `PRAGMA user_version = 1` at the v45 baseline DDL.
2. Repositories + table-watcher invalidation events (Room Flow
   replacement) — design against mochy-config's watcher shape
   (`std::sync::mpsc` + guards) so the invalidation API is uniform.
3. Android `mochy.db` importer v35–45 (destructive below 35; contiguous
   35→45) + event-timestamp normalization (PORT_SPEC: UTC + zone record);
   the Media3 `exoplayer_internal.db` download-index read path.
4. Migration tests need fixture `.db` files — decide the fixture strategy
   (hand-built minimal DBs per version vs. recorded device dumps) and
   record it here.
5. mochy-config consumers: Phase 04's services builder should construct
   `SettingsStore::load(config_dir/settings.json)`, mirror
   `offlineMode` into `mochy_net::set_offline`, and wire the proxy prefs
   (ProxyEnabled/Url/Type/Username + vault PasswordKey) into
   `ClientSpec::proxy` — exactly App.kt:187-235's observeSettingsChanges
   loop, one typed subscription instead of nine flows.

## Phase 03 — 2026-11-20

**Status:** done (all acceptance criteria met; artifacts emitted)

**Task:** mochy-db — schema v45 mirror, repositories, invalidation, Android
importer (Milestone M0, part 3).

**Commits:** `phase-03(db)` schema/actor → `phase-03(db)` queries/repos →
`phase-03(importer)` → `phase-03(tests)` → `phase-03(docs)` on branch
`phase-03` cut from `origin/main` (base `e23ff1a`).

### Harvest summary (acceptance numbers)

- **Schema: 24 tables + 3 views, 167 columns, `user_version = 1`, no
  `room_master_table`.** The schema-report test prints the full catalog
  (table → column count) plus the v34→v45 delta report.
- **The v34-vs-v45 column deltas found** (this is the "print a diff
  report" acceptance): v34 JSON carries 17 tables; the seven post-v34
  tables are `playlist_event` (MIGRATION_35_36), `imported_song`
  (38→39), `import_session` + `import_session_song` + `imported_playlist`
  (39→40), `imported_playlist_track` (40→41), `song_edit` (42→43); column
  additions are `song.localUri` (36→37), `playlist.description` (37→38),
  `imported_playlist.remoteId/remoteUrl/lastSyncedAt` (40→41),
  `song.addedByLike` (41→42), `song.genre` (43→44),
  `imported_playlist.authorName/authorAvatarUrl/authorUrl` (44→45). The
  mirror's columns match the v34 JSON exactly (name + affinity + notNull +
  default) and the delta equals that documented set (asserted, not just
  printed).
- **DDL source decisions:** ≤v34 columns/types/defaults/index DDL from
  `app/schemas/com.mochyy.mochymusic.db.InternalDatabase/34.json` (the
  last export; `exportSchema = false` since); v35→v45 additions from the
  hand-written migration `db.execSQL` strings verbatim; fresh-schema
  semantics (column order, DEFAULT presence) from the current
  `db/entities/*.kt` declarations. One documented divergence:
  `playlist.description` has `DEFAULT NULL` when migration-added but no
  DEFAULT in a fresh Room v45 DDL (the entity declares none) — the
  mirror follows the entity; behaviorally identical (nullable column).
- **`mochy-db` test count: 33** (7 unit + 3 schema-report + 14 repository
  + 5 watcher + 4 importer); workspace total 109, all green.

### Decisions

1. **Timestamp normalization scope (Q4 interpretation).** The task named
   `event.timestamp`, but upstream `Converters.kt` stamps EVERY
   `LocalDateTime` column wall-clock-as-UTC (`dateToTimestamp` through
   `ZoneOffset.UTC` + `LocalDateTime.now()` write sites), and the desktop
   stores true UTC (blueprint ch. 7.2 "the port normalizes on import and
   stores UTC" — otherwise Phase 14's UTC writes would mix conventions and
   `likedDate > :since` comparisons would break). The importer therefore
   shifts all 22 converter-stamped columns once
   (`WALL_CLOCK_AS_UTC_COLUMNS` in `importer/event_time.rs`) — still ONE
   transform conceptually — and excludes the one column that is already
   true UTC upstream (`speed_dial_item.createDate` =
   `System.currentTimeMillis()`). A single fixed offset is recorded (the
   zone at import time); per-instant DST history is unknowable from the
   data — worst case 1 hour, never a recap-day boundary shift.
2. **The import timezone record lives in a namespaced `_mochy_meta`
   table** (`key TEXT PRIMARY KEY, value TEXT`), created only by the
   importer. Fresh databases stay a pure mirror (24 tables + 3 views +
   indices, nothing else — the schema test asserts it). The record holds
   source version, offset minutes + label, shift ms, normalized-cell
   count, and the convention string, so recap/stats know events are UTC.
   `read_import_meta()` is the reader.
3. **Importer version gate: `user_version` 35..=45.** Below 35 there is no
   surviving migration path (verified fact #6) — Android itself
   destructively wiped those databases, so importing them would be
   reading garbage; the importer refuses with a typed error. The copy is
   column-intersection driven, so ANY intermediate shape in range works
   (a v35 source lacks the 7 post-v34 tables → zero-row reports with
   filled-default column lists; a v45 source has everything). v35's
   schema equals v34's (no `MIGRATION_34_35` exists — version bump only),
   which is how the v35 fixture is built (34.json createSql executed
   verbatim).
4. **Fixture strategy (HANDOFF Phase 02 open point #4): harvested DDL, not
   device dumps.** The committed `tests/data/room_schema_v34.json` IS the
   upstream export; fixtures are built by executing its `createSql`
   statements (tables, indices, views) + the migration DDL for v45. This
   is reproducible, reviewable, and license-clean; recorded device dumps
   would embed private data and bitrot. Real-device validation remains a
   Phase 22/25 checkpoint item (parity harness).
5. **Atomic wholesale import, no merge.** Upstream backup/restore replaces
   the library wholesale (BackupRestoreViewModel copies whole files); the
   importer matches: one transaction (`BEGIN IMMEDIATE` … `COMMIT`, with
   `foreign_keys` toggled OFF outside it — SQLite ignores the pragma
   mid-transaction), rollback on any failure, `foreign_key_check`
   audited after commit and reported (not fatal: `import_*` orphans are
   by design). Re-import/merge is Phase 22 TransferArchive territory.
6. **`Db::write_raw`** is the documented escape hatch for bulk tools: same
   actor serialization, no wrapping transaction, closure manages
   BEGIN/COMMIT + pragmas. Repositories use `Db::write` (transaction +
   post-commit invalidation). Invalidation fires ONLY after commit — an
   aborted transaction wakes nobody (Room behaves the same).
7. **tokio in mochy-db's API (PORT_SPEC §3.1 tension, task-mandated).**
   The Phase 03 task pins "command channel over tokio-rusqlite"; the
   actor's write futures are runtime-agnostic (dedicated std thread +
   crossbeam in, oneshot out — no timers/IO), so GPUI's executor can
   await them directly. Reads are sync (`Db::read`, pooled). The crate
   exposes no GPUI and no platform-specific code; `tokio::sync::broadcast`
   (the invalidation bus) is the only other tokio type and it is wrapped
   behind `InvalidationHandle`/`watch()`.
8. **Watcher API shape:** the raw broadcast (`subscribe()` →
   `InvalidationHandle::recv(tables)` filters by table set, lag coalesced)
   plus `watch(tables)` → `(std_mpsc Receiver, WatchGuard)` — the same
   idiom as mochy-config's settings watchers (Phase 02's uniformity
   request). The mpsc fan-out runs synchronously in `send()` with
   `try_send` (a stuck consumer can never block the writer); dead
   receivers are reaped on send.
9. **Queue reconstruction basis:** most-recent DISTINCT songs from the
   event table (timestamp DESC, cap 50 — `RECONSTRUCTION_LIMIT`), current
   = head, paused = true (upstream restores with `playWhenReady = false`),
   each entry carrying artists (position order) + the cached format row
   so the first start comes from cache. Fallback with no history: newest
   library songs. Shuffle order is lost — recorded on the
   `QueueReconstruction` (serde) record for Phase 10.
10. **Downloads seed:** Media3 1.7.1 `DefaultDownloadIndex` schema
    (`ExoPlayerDownloads` in `exoplayer_internal.db` — table name verified
    from the androidx/media source: `TABLE_PREFIX + "Downloads"` with the
    default empty name; state constants QUEUED=0/STOPPED=1/DOWNLOADING=2/
    COMPLETED=3/FAILED=4/REMOVING=5/RESTARTING=7). Completed rows mark
    `song.isDownloaded/dateDownload` exactly like the
    `onDownloadChanged` listener; STOPPED/REMOVING/FAILED clear it;
    in-flight states stay for Phase 15's job queue, which also receives
    the full seed (bytes, content length, `custom_cache_key`, and the
    UTF-8 title from the request `data` blob —
    `DownloadRequest.Builder(songId, songId.toUri())
    .setCustomCacheKey(songId).setData(title.toByteArray())`,
    DownloadUtil.kt:477-480). `dateDownload` comes from the index's
    true-UTC `update_time_ms` (upstream stamps wall-clock
    `LocalDateTime.now()`; the true instant is unrecoverable — recorded
    as the deviation it is).
11. **Entity state machines are persistence-pure.** `SongEntity.toggleLike`
    / `toggleLibrary`, artist/album/playlist bookmark toggles are ported
    as exact row-update state machines (cited), and every one documents
    the Phase 14 SongService hook for the YouTube call that upstream
    fired from inside the entity. Repositories never do IO beyond SQLite.
12. **Sort behavior:** SQL ORDER BY matches the DAO text; the Kotlin-side
    `Collator` re-sorts (NAME/ARTIST, PRIMARY strength) are a UI-layer
    concern in the port (mochy-ui-kit's script-aware index rail owns
    collation) — the DB returns the base feed, exactly like
    `songsByRowIdAsc` did before Kotlin re-sorted. The upstream
    `playlist_song_map_preview` view is `WHERE position <= 3` VERBATIM —
    with 0-based positions that yields up to FOUR preview thumbnails; the
    port keeps the exact semantics (a test pins it).

### Deviations

- **`_mochy_meta`** — a 25th, namespaced table on imported databases only
  (the timezone record). Required by "record the import timezone"; fresh
  DBs remain the pure 24+3 mirror.
- **Normalize-all-converter-columns** instead of only `event.timestamp`
  (Decision 1) — the task's transform list is honored in spirit and
  letter for `event.timestamp`; the extension is what keeps the DB
  self-consistent.
- **Collation deferred to UI** (Decision 12) and **album-artist junction
  unordered** (Room emits no ORDER BY for `AlbumArtistMap`; neither do
  we).
- **tokio-rusqlite in the crate's async surface** (Decision 7) — mandated
  by the task prompt over PORT_SPEC §3.1's "where avoidable".
- **No other deviations.** The 3 views exist with upstream SQL text even
  though consuming queries inline the ORDER BY (parity + Android-import
  compatibility of `sqlite_master`).

### Verification (acceptance)

- `cargo test -p mochy-db`: **33/33 green**; the schema-report test
  prints 24 tables + 3 views with column counts (167 columns) and the
  v34→v45 delta.
- The importer test produces a **normalized event table** (wall-clock
  05:30-IST stored → true UTC asserted cell-by-cell, plus likedDate/
  inLibrary/lastUpdateTime shifted, `speed_dial_item.createDate`
  untouched), a **queue-reconstruction record** (paused, current song,
  artists + format, blob-present + shuffle-lost flags, JSON round-trip),
  and a **downloads seed** (3 entries, 2 completed applied, cache key +
  title verified) — all three transforms asserted end-to-end on both a
  v35 and a v45 Android-shaped fixture, plus a Media3 fixture.
- `cargo test --workspace`: 109 green (Phase 01/02 suites untouched).
  `cargo fmt --all --check` clean; `cargo clippy --workspace
  --all-targets -- -D warnings`: 0 errors.

### Next steps (Phase 04 — app shell skeleton, CP M0)

1. Read PORT_SPEC §3 row `mochy`, §10 Phase 04, blueprint ch. 10.1; the
   `mochy` bin's stub `fn main` becomes the GPUI `Application::new()`
   shell with the main window, sidebar + Route enum, per-tab stacks
   (back = pop within tab; re-tap = pop to root + scroll top), and theme
   tokens (dark-only, chrome `0x131313`, hairline + grain, Inter seven
   weights, -0.05em semibold).
2. Services builder → typed `AppContext` as a GPUI Global, following
   Phase 02's wiring list: `SettingsStore::load(config_dir/settings.json)`,
   mirror `offlineMode` into `mochy_net::set_offline`, proxy prefs
   (ProxyEnabled/Url/Type/Username + vault PasswordKey) into
   `ClientSpec::proxy` (App.kt:187-235 semantics), and now
   `Db::open(data_dir/mochy.db)` + `Repositories::new` with watch
   bridges (`Db::watch` → `cx.spawn` re-runs).
3. Register the `mood_and_genres` route properly (PORT_SPEC §4.1).
4. M0 checkpoint: CI green on 3 OS; an imported DB opens (the Phase 03
   importer's output is the fixture); window with sidebar + routing +
   tokens; HANDOFF entry + PROGRESS ticks.
5. The Linux CI leg will need GPUI's system libraries once `mochy` gains
   its gpui dep (the ci.yml comment lists them) — wire that in this
   phase, not later.

## Master audit of Phases 01–03 — 2026-11-20

**Status:** done (audit complete; all CRITICAL/HIGH/MEDIUM findings fixed;
artifacts re-emitted)

**Task:** evidence-driven audit of Phases 01–03 against the pinned
upstream + fix the macOS CI watcher failure + remediate the unused
workspace dependency warnings. Full reports:
`docs/audits/{PHASE_01,PHASE_02,PHASE_03,INTEGRATION}_AUDIT.md`,
`MASTER_AUDIT_REPORT.md`, `REQUIREMENTS_MATRIX.md` (106 PASS / 3
UNVERIFIED / 0 FAIL of 109 requirements).

### Decisions

1. **macOS CI root cause (F-001, CRITICAL):** `Db::open_in_memory` named
   the shared-cache URI from `SystemTime::now()` nanos; macOS
   `clock_gettime` ticks at microsecond granularity, so parallel tests
   could draw identical values, share ONE in-memory DB, and the second
   `INSERT OR IGNORE` of the same song id silently returned `Ok(false)`
   — exactly the `assert!(inserted)` failure on macOS. The broadcast
   system was never at fault (send-after-commit-before-reply ordering
   verified). Fix: `pid` + process-global `AtomicU64` counter — unique by
   construction on every OS. Reproduced deterministically by forcing a
   fixed URI (byte-identical CI failure), then fixed; 50/50 stress runs.
   The importer tests' timestamp-named temp dirs got the same hardening.
2. **toggle_library semantics (F-002, HIGH):** the port forced
   `liked = true` when adding to the library; upstream leaves `liked`
   unchanged on add (SongEntity.kt:76-78) and clears it on remove.
   Fixed to the upstream state machine + regression test.
3. **Registry defaults audit (F-003, HIGH):** mechanical re-harvest of all
   upstream default read sites (247 keys covered) found 11 wrong rows:
   ten `bool` keys defaulting `false` that are `true` upstream (all
   library sort-descending keys, `showArtistVideo`, `ytmSync`) and
   `selectedThemeColor` (-6035826 → -7434610 =
   `Color(0xFF8E8E8E).toArgb()`). `addToPlaylistSortDescending` has
   CONFLICTING upstream sites (`?: true` in PlaylistsViewModel vs `false`
   in AddToPlaylistDialogOnline); resolved `true` for consistency with
   every sibling sort key. The dead key
   `autoPlaylistSongSortDescending` (zero upstream readers) is now
   `hidden: true`.
4. **Secrets/settings boundary (F-004, MEDIUM):** the 12 vault-owned keys
   are registry rows (needed for Android import round-trip), and
   `encode()` serialized every value — a secret set through a typed setter
   would land in plain `settings.json`. `encode()` now skips
   `SECRET_KEYS` (SecretPreferences.kt:41-54), warning when the value is
   non-default; test proves neither key names nor values can appear in
   the file.
5. **Unused workspace deps (F-005, MEDIUM):** kept every reserved pin
   (each maps to a scheduled phase — table in the root Cargo.toml
   comment) and silenced exactly the one cargo lint
   (`[workspace.lints.cargo] unused_workspace_dependencies = "allow"`),
   with `[lints] workspace = true` inherited by all 19 members.
   `cargo check`/`clippy -D warnings`: 0 warnings.
6. **LOW findings documented, not code-changed:** "22"→21 converter
   columns (F-006), "20"→21 login keys comment (F-007), uncommitted
   registry generator (F-008 — commit the next harvester to `scripts/`),
   shared-cache table-lock caveat for the in-memory test path (F-009),
   fixed `.tmp` name in save() (F-011), duplicated secret-name consts
   (F-014, revisit at Phase 04 wiring).

### Deviations

- None from the phase specs. All fixes move the port TOWARD upstream
  behavior; the lint config is a CI-hygiene change with a documented
  rationale.

### Verification (acceptance)

- `cargo fmt --all -- --check` clean; `cargo check --workspace` 0
  warnings (was 37); `cargo clippy --workspace --all-targets --
  -D warnings` 0; `cargo test --workspace` **117/117** across 41
  binaries (mochy-config 29, mochy-vault 6, mochy-net 28, mochy-db 39).
- Watcher stress: 50 consecutive runs, 8 threads, 0 failures; also green
  single-threaded. **macOS confirmation is CI-pending** (local
  reproduction was deterministic; the sandbox is Linux).

### Next steps (unchanged for Phase 04)

1. Push and let the 3-OS matrix run — the macOS leg going green closes
   F-001 with runtime evidence.
2. Proceed with Phase 04 per the Phase 03 entry's list; no blockers.
