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

