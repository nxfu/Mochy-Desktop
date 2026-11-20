# Integration Audit — Cross-Phase Architecture (Phases 01–03)

**Audit date:** 2026-11-20

---

## 1. Dependency direction and boundaries

- **Inter-crate edges: none yet.** Phases 02/03 crates (config, vault, net,
  db) are self-contained; the 13 not-yet-implemented crates are dependency-
  free stubs. There is therefore no upward or circular reference anywhere in
  the graph (verified by reading every member manifest).
- **Domain purity:** `mochy-config` and `mochy-vault` expose **no tokio and
  no GPUI** in their public APIs (watchers are `std::sync::mpsc` + guards —
  a deliberate Phase 02 decision recorded in HANDOFF). No `cfg(target_os)`
  exists outside leaf crates (grep across all four implemented crates:
  zero hits). Platform-specific code today exists only as mochy-audio's
  feature-gated optional backends.
- **Documented exceptions (accepted):**
  - `mochy-net` is async-tokio in its public API — positioned as
    infrastructure (PORT_SPEC §3 row 19); the retry *decision function* is
    pure/sync for testability. Recorded in HANDOFF Phase 02 Decision 9.
  - `mochy-db`'s write futures are runtime-agnostic (dedicated thread +
    oneshot; no timers/IO), and the only tokio type reaching the public API
    is the broadcast receiver wrapped behind `InvalidationHandle` — the
    Phase 03 task pinned "command channel over tokio-rusqlite". Recorded in
    HANDOFF Phase 03 Decision 7.
- **AppContext consumption path (Phase 04 wiring list)** is coherent:
  `SettingsStore::load` → mirror `offlineMode` into `mochy_net::set_offline`
  → proxy prefs + `VaultKey::ProxyPassword` into `ClientSpec::proxy` →
  `Db::open` + `Repositories::new` + watch bridges. Every named API exists
  with compatible shapes (checked during this audit).

## 2. Secrets boundary (vault vs. settings)

- **The gap (F-004, fixed):** the 12 vault-owned keys are registry rows
  (they must be — Android import round-trips the key set), and the
  persistence encoder serialized every value, so a secret set through a
  typed setter would have landed in plain `settings.json`. Android keeps
  these keys in the Keystore-sealed store, never in the plain DataStore.
- **The fix:** `mochy_config::store::SECRET_KEYS` (the 12 names, source-
  cited) is now excluded by `encode()` — default-seeded empty values skip
  silently; a non-default value skips with a `warn!` pointing at the vault.
  A test proves neither the key names nor the values can appear in the
  persisted file, and that reload falls back to defaults.
- The vault itself never writes files (keyring backends only); no plaintext
  secret logging exists anywhere (grep across the four crates).

## 3. Offline-mode control path

`mochy_net::set_offline` (global atomic mirroring the upstream singleton)
is the single switch; `retry_on_io_errors` consults it before every
attempt, and no alternate request path in the crate bypasses the wrapper
(read of every `reqwest` call site). The Phase 04 plan mirrors the
`offlineMode` preference into it — the settings key exists in the registry
and the mapping point is documented. ✅

## 4. Database invalidation usability by later phases

- `Db::watch(tables) → (std_mpsc::Receiver<TableSet>, WatchGuard)` matches
  mochy-config's watcher idiom (same guard/unsubscribe shape) — uniform
  consumption for Phase 04's `cx.spawn` re-runs.
- Happens-before guarantee: awaiting a `write()` implies the invalidation
  is already published — subscribers created *before* the write can never
  miss it; subscribers created after receive the buffered event on their
  next `recv`. Verified by tests (multi-subscriber, repeated writes, guard
  drop).
- The importer fires one all-tables invalidation after its single
  transaction — a pre-subscribed watcher sees exactly one re-run signal.

## 5. Unused workspace dependencies (F-005, fixed)

- 37 warnings on every build (cargo 1.100's `unused_workspace_dependencies`
  lint) because the workspace pre-pins every future-phase dependency (Phase
  01 Decision 4 design).
- **Investigation:** every warned pin maps to a scheduled phase (gpui → 04;
  wry/rustypipe-botguard/rquickjs → 07/08; symphonia/lru → 09; proptest →
  10; cpal → 11; image/resvg/dominant_color/palette → 12; interprocess/
  zbus/mpris-server/windows/tray-icon/muda/notify-rust/
  tauri-winrt-notification/mac-notification-sys → 14; lofty → 17/22;
  vibrato/pinyin → 18; rustfft/rubato → 19; mdns-sd/axum/
  tokio-tungstenite → 23; rust-i18n → 24; clap/tracing-subscriber/anyhow →
  04+). **None should already be in use** by Phases 01–03 code; none is
  genuinely unnecessary; removing them would fork the pin list from
  PORT_SPEC §9 and invite drift.
- **Remediation:** the sanctioned cargo lint config
  `[workspace.lints.cargo] unused_workspace_dependencies = "allow"` at the
  workspace root with the full rationale + phase mapping in a comment
  block, plus `[lints] workspace = true` in all 19 members (also future-
  proofing centralized lint policy). Verified: `cargo check --workspace`
  and `cargo clippy --workspace --all-targets -- -D warnings` now emit
  **zero warnings**. This is scoped to exactly one cargo lint — not a
  blanket suppression, and rustc/clippy lints remain at their defaults
  (`-D warnings` still enforced in CI).

## 6. Consistency with PORT_SPEC

- Workspace member list, feature-flag contract, layering rules: unchanged
  and honored (§3).
- The Phase 02 resequencing (mochy-net pulled from 05) and Phase 03's
  deviations (`_mochy_meta`, normalize-all-converter-columns, collation
  deferral) are recorded in HANDOFF as required.
- PORT_SPEC §2 verified facts were re-confirmed during this audit where
  checkable (398 keys, 24+3 schema, 14 migrations, v34 export cutoff,
  12 vault keys) — no silent changes.

## 7. Findings

| ID | Severity | Finding | Status |
|---|---|---|---|
| F-004 | MEDIUM | Secret keys could persist to the plain settings file through registry setters (vault/settings boundary gap). | **Fixed** + test |
| F-005 | MEDIUM | 37 unused-workspace-dependency warnings; remediated with documented, scoped lint allow + member inheritance. | **Fixed** |
| F-014 | LOW | mochy-config and mochy-vault both define the 12 secret key names independently (registry rows vs `VaultKey::name()`). They are identical today (both source-cited); a shared const would prevent future drift but would create a config↔vault edge. Current duplication is the lower-cost option at this stage. | Documented (revisit when Phase 04 wires them) |

## 8. Verdict

**Integration: PASS with fixes applied.** Boundaries are clean, the secrets
gap is closed, the invalidation design is consumable by Phase 04, and CI is
warning-free without weakening any lint gate.
