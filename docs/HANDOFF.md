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

## Phase 04 — 2026-11-21

**Status:** done (all acceptance criteria met; artifacts emitted)

**Task:** mochy-innertube core — client matrix, transport, auth,
endpoints, paging (Milestone M1, part 1). The phase prompt resequenced
the ledger: the innertube bottom two tiers moved from Phases 05–07 into
Phase 04; the `mochy` app-shell skeleton (PROGRESS's original Phase 04,
CP M0) is displaced and awaits its own prompt.

**Commits:** `phase-04(workspace)` → `phase-04(innertube)` registry/
models/auth → `phase-04(innertube)` transport/endpoints/session/guest/
paging → `phase-04(innertube)` tests → `phase-04(docs)` on branch
`phase-04` cut from `origin/main` (base `597034d`).

### Harvest summary (acceptance numbers)

- **18 client configs ported verbatim** from
  `models/YouTubeClient.kt` (305 lines): every id, version, user agent,
  os/device field, login/STS/PO-token flag, api origin,
  `apiFormatVersion`, `ownVisitorId`. Registry unit tests pin all of
  them; `roles` carries the YTPlayerUtils walk order (MAIN_CLIENT =
  VISIONOS, 3 ACCOUNT_CLIENTS, 3 CAPTION_CLIENTS, 13
  STREAM_FALLBACK_CLIENTS).
- **22 endpoints** (the task's enumeration; see Decision 6 for the
  count) with the full typed body layer: 14 body structs + the 7
  `browse/edit_playlist` actions, golden-tested against the
  kotlinx-serialization wire shapes (camelCase, explicitNulls=false,
  encodeDefaults=true).
- **mochy-innertube test count: 76** (46 unit + 13 golden-header + 6
  mirror + 5 paging-property + 6 fixture-replay); workspace total 192,
  all green, all offline (local wire servers + synthetic fixtures).

### Decisions

1. **Resequencing recorded, not silently applied.** The Phase 04 prompt
   (innertube core) overrides the PROGRESS ledger's original Phase 04
   (app shell / CP M0). PROGRESS.md is retitled per its own preamble
   ("phase titles for 02+ … may be refined by their own prompts"); the
   app-shell items are kept visible there as displaced-unchecked, and
   PORT_SPEC §10's Phase 04 row is annotated. The M0 checkpoint moves
   with them — whichever phase prompt picks the shell up owns CP M0.
2. **tokio is in mochy-innertube's public API** (like mochy-net):
   every upstream endpoint is a `suspend fun`, so the transport is
   unavoidably async (PORT_SPEC §3.1 rule 1's "where avoidable" carve-
   out). The domain logic — registry, auth, bodies, paging, guest
   scrape/mitigator, the `||`-split — is sync and fully tested without
   a runtime. No GPUI, no platform code.
3. **Session = one struct behind `Arc<RwLock<Session>>`** mirroring the
   mutable fields of the upstream `YouTube`/`InnerTube` singletons
   (YouTube.kt:78-116). The lock is std (never held across an await);
   each request clones a `session_snapshot()`. The proxy/ipVersion
   setters' client-rebuild side effects (InnerTube.kt:53-67) become a
   seed-compare-and-rebuild in `ensure_client()` before every request.
4. **The settings mirror is `initialize` + `subscribe`/`handle_one` +
   `pump`.** `pump` (the eternal `observeSettingsChanges` loop) is
   `subscribe` + `handle_one` composed, and tests drive `handle_one`
   directly — deterministic, no subscription race. Upstream's nine
   collectors collapse into one `watch_all` stream filtered by
   `SettingId` (same mapping, same order of precedence: content prefs
   beat the system locale; UI language never changes content prefs).
   Secrets come from mochy-vault (`innerTubeCookie`/`visitorData`/
   `dataSyncId`), per Phase 02 decision F-004.
5. **R1 hotfix seam = `ClientRegistry`.** Client *version strings* are
   overridable at runtime (`set_version`, `from_json`,
   `load_overrides(path)` — a JSON file like
   `{"WEB_REMIX": "1.20260801.03.00"}`); unknown client names warn and
   are ignored so a newer hotfix file never breaks an older build.
   Everything else about a client is code, exactly like upstream. The
   app shell (later phase) wires `config_dir/innertube-clients.json`.
6. **The 22-endpoint count** follows the task's own enumeration, which
   lists `subscription/subscribe + unsubscribe` as one line (one
   subscription surface, two actions) and counts BotGuard
   Create/GenerateIT as one entry. The transport exposes both methods
   and both BotGuard calls; `endpoints::ENDPOINTS` pins the catalog at
   exactly 22 and a test asserts it.
7. **Three dedicated clients, no shared header set.** RYD Votes
   (plain GET + `Content-Type: application/json`),
   videostats reporting (GET as the WEB_REMIX client with
   ver/c/cver/cpn + the playlist `list`/`referrer` pair and
   `musicStatsUrl`'s s.youtube.com → music.youtube.com rewrite), and
   BotGuard Create/GenerateIT (POST with `application/json+protobuf`,
   `x-goog-api-key`, `x-user-agent: grpc-web-javascript/0.1` —
   PoTokenWebView.kt:251-280 constants) each keep their own header
   shape instead of the ytClient set.
8. **Own-visitor-id minting lives in the session** (upstream's
   `ownVisitorIds` ConcurrentHashMap, InnerTube.kt:185) and
   `resolve_visitor` implements the player path's branch
   (InnerTube.kt:256): ownVisitorId clients use their minted id (mint
   on first use; a failed mint sends the request without one, logged at
   debug), everyone else the session visitorData.
9. **Test seam: `TransportConfig::origin_overrides`.** The transport's
   origins are the real constants in production; tests (and only tests)
   rewrite `https://music.youtube.com` / `https://www.youtube.com` to a
   local wire-capturing server — the only way per-request header
   injection is observable with reqwest. The upload handshake's
   response-header capture (`x-guploader-uploadid`) is a small
   last-capture cell, sequential by construction (one caller's
   two-step flow).
10. **The fixture corpus is synthetic until a live recording replaces
    it.** `tests/fixtures/{sw_js_data,visitor_id,search}.json` are
    hand-built minimal payloads shaped to the parser paths the port's
    doc comments cite (no user data, no recorded YouTube traffic); the
    `redact` pass (visitorData/onBehalfOfUser/poToken/trackingParams/
    cpn/authorization/cookie) runs before any live capture is written.
    `tests/fixtures/README.md` states provenance. `cargo test
    --features live` records real captures; CI runs plain
    `cargo test --workspace`, and tests/live.rs is compiled out without
    the feature — the live path can never run in CI.
11. **Paging is a caller-driven state machine** (`ContinuationPager`):
    `admit(token)` (ceiling first, then seen-set — upstream's
    evaluation order) + `record_page(empty)` (playlist: break on the
    first empty page; library: break on the second consecutive empty).
    The pager does no IO — the ceiling is enforced caller-side by
    construction, and the caller keeps the stopping token so "a sync
    knows the list is short" (Utils.kt:8). Phase 05's facade drives it.
12. **proptest pulled forward as this crate's dev-dependency** (pinned
    in Phase 01 for Phase 10's queue tests; the paging property tests
    are exactly the blueprint's "queue/paging state-machine property
    tests" layer). No new external pin was needed beyond `sha1` 0.11.0
    and `base64` 0.23.1 (both verified current on crates.io
    2026-11-21), plus reqwest's `query` feature (0.13 moved
    `RequestBuilder::query` behind serde_urlencoded — the transport's
    query shaping needs it).

### Deviations / upstream conflicts (upstream won, as required)

- **ANDROID_CREATOR's clientId is "14", not 3** — the task text grouped
  it with the id-3 clients; `YouTubeClient.kt:255` says 14. Ported 14.
- **ANDROID (MOBILE) stays wired** — the task text said
  "ANDROID + ANDROID_NO_SDK (unused, do not wire)", but upstream wires
  MOBILE as the last STREAM_FALLBACK_CLIENTS entry
  (YTPlayerUtils.kt:186); only ANDROID_NO_SDK is documented dead
  (PORT_SPEC §4). The port wires exactly the upstream roles and marks
  ANDROID_NO_SDK as the single unwired client.
- **`^Cg[t|s]`'s literal-pipe quirk not ported** — the Kotlin character
  class `[t|s]` also matches `Cg|…`, which YouTube never mints; the
  port implements the two real branches (`Cgt`/`Cgs`).
- **ktor's `parameter(name, null)` is a no-op; the port only pushes
  query pairs that are `Some`** — same wire result, stated because the
  code shape differs.
- **The okhttp Cache revalidates conditionally; the port's DiskCache is
  explicit GET-only storage without revalidation** (the mochy-net seam
  Phase 02 built; Phase 08 completes ranged semantics). POSTs bypass
  the cache, like upstream's GET-shaped usage in practice.
- **`dataSyncId`/`visitorData`/`innerTubeCookie` read from the vault,
  not the settings file** (Phase 02 F-004 boundary) — the same values
  upstream's DataStore held, with the same `"||"`-split and
  literal-`"null"` handling in the mirror.
- **`YouTube.getMediaInfo`'s next+RYD composite is NOT here** — it is
  facade/parsing work (Phase 05); the RYD Votes endpoint it calls is.

### Verification (acceptance)

- `cargo test -p mochy-innertube`: **76/76 green, offline** — includes
  the three golden-header acceptance tests (WEB_REMIX signed-out: the
  full client header set with no auth pair; WEB_REMIX signed-in: cookie
  verbatim + Authorization recomputed from the same time/origin triple;
  VISIONOS guest: api format 2, no X-Origin/Referer, the minted visitor
  riding header + context through a served visitor_id round trip), the
  SAPISIDHASH vector test (known triple, digest verified against an
  independent sha1sum), and the paging property tests (dedup +
  50-ceiling + both empty-page policies).
- `cargo test --workspace`: **192/192** (Phase 01–03 suites untouched).
  `cargo fmt --all --check` clean. `cargo clippy --workspace
  --all-targets -- -D warnings`: 0.
- `cargo check -p mochy-innertube --features live --tests` compiles
  (the live recorder is reachable but never runs in CI).
- Wire-level proofs: 429 → exactly one request; offline → zero
  requests; IO failure → retry recovers; repeated GET → one request
  (the 50 MB disk cache serves the repeat).

### Next steps (Phase 05 — models, parsers, browse/search)

1. Read PORT_SPEC §3 row `mochy-innertube` and the endpoint layer's
   citations; the parsers port against `serde_json::Value` inputs from
   `endpoints.rs` (each method already cites its upstream lines).
2. Port `models/` response DTO equivalents + the 21 `pages/` parsers +
   2 glue files (`BrowseResult`, `PageHelper`), then the `YouTube.kt`
   facade (1,814 lines): typed returns over the 22 endpoints, the
   search filters (`SearchFilter`/`SEARCH_GROUPS`), `WatchEndpoint`,
   and `YouTubeDataPage`-driven flows.
3. Wire the facade's continuation loops through
   `paging::ContinuationPager` (the 50-ceiling helper built here) and
   the `completed()` extensions (`Utils.kt:9-74`).
4. The `getMediaInfo` next+RYD composite lands with the parsers.
5. Contract tests: extend the fixture corpus through the record path
   (`--features live`) once real captures are available locally; keep
   the committed set synthetic-or-redacted per the README.
6. Retitle/tick Phase 05 items in PROGRESS.md (transport + guest items
   are already ticked — done here).

## Phase 05 — 2026-11-21

**Status:** done (all acceptance criteria met; artifacts emitted)

**Task:** mochy-innertube parsers + YouTube facade + contract fixtures
(Milestone M1, part 2) — the `models/` DTO layer, the 21 `pages/`
parsers + 2 glue files as pure functions, the 1,814-line `YouTube.kt`
facade, and the fixture corpus over which everything is tested offline.

**Commits:** `phase-05(models)` → `phase-05(pages)` → `phase-05(facade)`
→ `phase-05(fixtures)` → `phase-05(docs)` on branch `phase-05` **cut
from `phase-04`** (see Decision 1).

### Harvest summary (acceptance numbers)

- **DTO layer**: `models/` minus the dead comment stack and
  `AddItemYouTubePlaylistResponse` — Runs+helpers, Endpoint family,
  Thumbnails (with the `croppedSquareThumbnailRenderer` JsonNames fold),
  Icon/Badges/Button/SubscriptionButton/ResponseContext/Continuations,
  Menu, Tabs, SectionListRenderer(+chip cloud), the 8 music renderers,
  the 5 header renderers, the PlaylistPanel family, TwoColumn+Grid,
  YouTubeDataPage, YTItem domain (Song/Album/Playlist/Artist +
  explicit/video/shorts/AI filters), AccountInfo/MediaInfo/RYD/
  SearchSuggestions/CaptionCue/VideoCaptions, and the 14 response
  envelopes. `regex` 1.13.2 pinned for the Utils.kt/Runs.kt/YTItem.kt
  patterns.
- **20 parsers + 2 glue files** ported as pure functions (no IO, no
  clock, no randomness); `LibraryAlbumsPage` skipped (dead, PORT_SPEC
  §4), `NewPipe.kt` deferred to Phase 08 (an extractor bridge, not a
  parser — PROGRESS Phase 08 owns "NewPipeExtractor-needed parts").
- **YouTube facade**: 64 of the 70 non-dead upstream methods (see the
  coverage table); the 50-request ceiling enforced through the Phase 04
  `ContinuationPager` (albumSongs + the `completedLibrary`/
  `completedPlaylist` extensions + musicHistory's own bounded walk).
- **Fixture corpus**: 24 synthetic fixtures (generated by
  `scripts/gen_fixtures.py`), every parser covered by at least one
  fixture test.
- **mochy-innertube test count: 138** (71 unit + 29 parser-corpus +
  8 facade-wire + 6 fixture-replay + 13 golden-header + 6 mirror +
  5 paging-property), all offline; workspace total 254.

### Facade coverage table (YouTube.kt, 1,814 lines)

| upstream member | port | note |
|---|---|---|
| session accessors (locale/visitorData/dataSyncId/cookie/proxy/proxyAuth/useLoginForBrowse/offline/ipVersion) | `Session` (Phase 04) | YouTube.kt:78-129 |
| refreshVisitorData() / refreshVisitorData(client) / clearGuestSession | `refresh_visitor_data` / `refresh_visitor_data_for` / `clear_guest_session` | YouTube.kt:131-141 |
| searchSuggestions / searchSummary / search(+6 filters) / searchContinuation | ✓ | YouTube.kt:143-263, 1532-1542 |
| album (+privately-owned branch) / albumSongs | ✓ | YouTube.kt:265-411 |
| artist / artistItems / artistItemsContinuation | ✓ | YouTube.kt:413-562 |
| playlist / parseRelatedItems / playlistContinuation | ✓ | YouTube.kt:564-690 |
| home / homeContinuation | ✓ | YouTube.kt:692-724 |
| explore / newReleaseAlbums / moodAndGenres / browse | ✓ | YouTube.kt:726-787 |
| library / libraryContinuation / libraryRecentActivity | ✓ | YouTube.kt:789-949 |
| getChartsPage / determineChartType / convertToChartItem / convertMusicTwoRowItem | ✓ | YouTube.kt:951-1119 |
| musicHistory | ✓ | YouTube.kt:1121-1180 |
| likeVideo / likePlaylist / subscribeChannel / getChannelId | ✓ | YouTube.kt:1182-1208 |
| addToPlaylist / addVideosToPlaylist / addPlaylistToPlaylist / removeFromPlaylist / moveSongPlaylist | ✓ | YouTube.kt:1210-1232 |
| createPlaylist / renamePlaylist / uploadCustomThumbnailLink / removeThumbnailPlaylist / deletePlaylist | ✓ | YouTube.kt:1234-1259 |
| player / newPlaybackNonce / registerPlayback / reportWatchtime / musicStatsUrl | ✓ (nonce+url since Phase 04) | YouTube.kt:1261-1300 |
| next (automix recursion) / lyrics / related / queue / transcript | ✓ | YouTube.kt:1302-1433 |
| captions / captionsAs / videoCaptions / captionCues | ✓ (+ Captions.kt: pickCaptionTrack/timedTextToLrc/timedTextToCues) | YouTube.kt:1435-1471 |
| visitorData / accountInfo / feedback | ✓ | YouTube.kt:1473-1494 |
| addSongToLibrary / removeSongFromLibrary / toggleSongLibrary | ✓ | YouTube.kt:1496-1526 |
| getMediaInfo | ✓ | YouTube.kt:1528 + InnerTube.kt:764-834 |
| LibraryFilter (4) / MAX_GET_QUEUE_SIZE / VISITOR_DATA_REGEX | ✓ | YouTube.kt:1544-1556 |
| completedLibrary / completedPlaylist (Utils.kt) | `completed_playlist` / `completed_library` | Utils.kt:9-74 |
| YouTubeUrlParser / YouTubeConstants | ✓ (`utils.rs`, `models::constants`) | utils/YouTubeUrlParser.kt |
| **getNewPipeStreamUrls / newPipePlayer** | **deferred — Phase 08** | YouTube.kt:1558-1599; the NewPipeExtractor last-resort bridge, not a facade parser; PROGRESS Phase 08 owns "NewPipeExtractor-needed parts re-implemented studying rustypipe" |
| **comments / commentContinuation / commentReplies** | **skipped — dead upstream** | YouTube.kt:1601-1813; `models/comment/` implemented but unwired (PORT_SPEC §4) |

**Count: 64 ported / 2 deferred-to-Phase-08 / 3 dead-skipped of 70
non-dead-adjacent members — 100% of the live facade minus the Phase 08
bridge.**

### Decisions

1. **Branch `phase-05` is cut from `phase-04`, not `origin/main`.** The
   task text says "cut from origin/main", but Phase 04 is unpushed and
   Phase 05 builds directly on its endpoint layer — a main-based branch
   could not compile. The patch series therefore stacks: apply
   `patches/phase-04/*` first, then `patches/phase-05/*` (both emitted;
   see the zip manifests). Whichever phase lands on main first, the
   other rebases trivially (no overlapping files outside Cargo.lock).
2. **serde `Option` leniency documented, not eliminated.** kotlinx
   requires a *key* for every constructor parameter without a default,
   so a malformed response fails the whole page upstream; serde's
   `Option<T>` treats a missing key as `None` and the parser drops that
   item. Well-formed YouTube responses are identical through both; the
   port degrades instead of erroring on malformed ones (models/mod.rs
   doc). Non-null Kotlin fields stay required in Rust.
3. **The fixture corpus is generated, not recorded.**
   `scripts/gen_fixtures.py` emits the 24 committed fixtures
   (json.dumps guarantees validity — hand-written JSON cost three
   debugging rounds before the generator replaced it). Provenance and
   the redaction contract live in tests/fixtures/README.md; the
   `--features live` record path can still replace the corpus with real
   redacted captures later.
4. **`AlbumItem` derives `id` from `browse_id`.** Upstream's
   `override val id: String = browseId` default became a method, not a
   stored copy — the struct-literal sites can never disagree with the
   upstream default again (a real bug class the corpus caught in the
   first draft).
5. **`transcript()` keeps the upstream edge-only trim** (♪ and spaces at
   the string edges, YouTube.kt:1428-1430); the bracket/♪ *scrub* is
   `captions()`' json3 path (Captions.kt). The wire test pins both
   behaviors so they cannot be conflated.
6. **The `getMediaInfo` author thumbnail keeps its protocol-relative
   `//` prefix** (InnerTube.kt:816-819 replaces only `s48`→`s960`); the
   `//`→`https:` normalization exists only in `toAccountInfo`
   (AccountMenuResponse.kt:44-46). Both pinned by tests.
7. **`cargo fmt` reformatted `tests/parsers.rs` assertion layout**
   mid-development; the committed style is the fmt output (no semantic
   change).

### Deviations / upstream conflicts (upstream won, as required)

- **`AlbumPage.getSongs`' two-column secondary read stays
  `musicShelfRenderer`** (AlbumPage.kt:76-77) although real album
  secondaries carry a `musicPlaylistShelfRenderer` — the effective song
  source is `albumSongs` (YouTube.kt:377-411), exactly like upstream;
  the dead path is preserved verbatim and the corpus test asserts its
  shape.
- **Nullable chains never drop songs**: upstream `?.` chains
  (artist endpoint, history remove-token) yield null FIELDS; the first
  draft used `?` and dropped whole items — fixed to `and_then`
  (ArtistPage.kt:112-114, HistoryPage.kt:77-79).
- **`MusicResponsiveHeaderRenderer.Button.menuRenderer` is a
  `MenuRenderer`, not a `Menu`** (MusicResponsiveHeaderRenderer.kt:16-19)
  — the first draft wrapped it one level too deep; the corpus decode
  caught it.
- **The search playlist author is the first run of the first secondary
  segment** (SearchPage.kt:159-165) — with the synthetic corpus that is
  the "Playlist" label run with no id, matching upstream exactly.
- **`library()`'s upstream off-by-one** (`tabs.size >= tabIndex`,
  YouTube.kt:813) fails the call for any out-of-range tab index through
  the "No content found" path; the port returns the same error without
  reproducing the index panic.
- **Unicode digit durations**: Kotlin's `\p{Nd}` regex accepts non-ASCII
  digits and `toIntOrNull` parses them; the port's regex accepts them
  but converts ASCII only (durations in the wild are ASCII; noted, not
  fixed).

### Verification (acceptance)

- `cargo test -p mochy-innertube`: **138/138 green, offline** — every
  `pages/` parser has at least one fixture test (29 corpus tests), serde
  round-trips over search/album/player/next, 8 facade wire tests (search
  with the filter in the body, album, queue, account, the getMediaInfo
  two-call composite, charts with types+positions, transcript, history).
- `cargo test --workspace`: **254/254** (Phases 01-04 suites
  untouched). `cargo fmt --all --check` clean. `cargo clippy
  --workspace --all-targets -- -D warnings`: 0.
- `cargo check -p mochy-innertube --features live --tests` compiles
  (the live recorder stays reachable, never runs in CI).
- The corpus is redacted (no live visitorData; audited by test).

### Next steps (Phase 06+ per the ledger)

1. The ledger's Phase 06 items are now done (this phase) — the next
   innertube work is the ledger's Phase 07 remainder: the **wry login
   window with cookie extraction** and the account menu UI over
   `accountInfo()`; sign-out must forget the vault keys.
2. Phase 08 (stream resolution): the client walk, format ranking, EJS
   n-solver, PO tokens — and the deferred NewPipe bridge study.
3. Record real captures over `--features live` when a networked
   maintainer run is possible; the corpus generator stays the source of
   truth for shapes until then.
4. The app shell (displaced Phase 04 items in PROGRESS) still awaits
   its prompt.
