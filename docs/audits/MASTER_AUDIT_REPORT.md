# MASTER AUDIT REPORT — Mochy Desktop Phases 01–03

**Audit date:** 2026-11-20
**Auditor mandate:** evidence-driven audit of the existing implementation
against the original phase specifications; fix CRITICAL/HIGH findings;
resolve the macOS CI failure; produce this report, per-phase reports and a
requirements matrix.

---

## Executive summary

**Overall verdict: PASS (with fixes applied during this audit).**

Phases 01–03 are genuinely implemented to specification. The audit
independently re-derived every checkable claim from the pinned upstream
(`bde01cb4`) rather than trusting documentation: the 19-crate workspace,
the 397-key settings registry (398 upstream declarations, alias-folded),
the 12-key vault, the HTTP layer's wire behavior, and the 24-table +
3-view schema mirror at column level (167 columns, 18 indices, verbatim
view SQL and migration DDL). Four real defects were found and fixed; two
documentation inaccuracies and three environment-limited UNVERIFIED items
are recorded. The macOS CI failure was root-caused (deterministically
reproduced), fixed, and covered with regression tests.

**Percentage of requirements verified PASS: 106/109 = 97.2%** (3
UNVERIFIED for environmental reasons; 0 FAIL, 0 PARTIAL, 0 BLOCKED —
see REQUIREMENTS_MATRIX.md).

## Phase statuses

| Phase | Status | Notes |
|---|---|---|
| 01 — workspace/docs/license/CI | **PASS** | All items verified; CI warning hygiene fixed (F-005) |
| 02 — config/vault/net | **PASS (after fixes)** | 11 wrong defaults fixed (F-003); secrets/settings boundary closed (F-004) |
| 03 — db | **PASS (after fixes)** | toggle_library semantics fixed (F-002); macOS CI flake fixed (F-001) |
| Integration | **PASS (after fixes)** | Boundaries clean; unused-deps remediated |

## Findings ledger

| ID | Severity | Phase | Summary | Resolution |
|---|---|---|---|---|
| F-001 | CRITICAL (CI-blocking) | 03 | macOS CI `write_fires_broadcast_invalidation` failure: in-memory DB name from `SystemTime` nanos collided on macOS's µs-resolution clock → two tests shared one shared-cache DB → second `INSERT OR IGNORE` of `song1` returned `Ok(false)`. Broadcast system itself was sound. | Fixed: pid + AtomicU64 naming (db.rs), same hardening for importer temp dirs; reproduced deterministically; 5 new regression tests; 50/50 stress runs |
| F-002 | HIGH | 03 | `toggle_library` forced `liked = true` on add; upstream leaves `liked` unchanged (SongEntity.kt:76-78) | Fixed + regression test |
| F-003 | HIGH | 02 | 11 registry defaults wrong (10 bools false→true incl. all sort-descending keys, showArtistVideo, ytmSync; selectedThemeColor -6035826→-7434610) + 1 dead key visible | Fixed + `audited_defaults_match_upstream_read_sites` test |
| F-004 | MEDIUM | 02/Integration | 12 vault-owned secret keys were serializable into plain settings.json | Fixed: SECRET_KEYS exclusion in encode() + warn + test |
| F-005 | MEDIUM | 01/Integration | 37 unused-workspace-dependency warnings on every CI build | Fixed: scoped `[workspace.lints.cargo]` allow with documented rationale + phase mapping; `[lints] workspace = true` × 19; 0 warnings verified |
| F-006 | LOW | 03 | HANDOFF says "22 converter columns"; correct list has 21 | Documented |
| F-007 | LOW | 02 | Comment says "20 login keys"; list has 21 (correct) | Documented |
| F-008 | LOW | 02 | Registry generator script never committed (reproducibility) | Documented; recommend scripts/ for the next harvester |
| F-009 | LOW | 03 | Shared-cache in-memory test path: theoretical SQLITE_LOCKED if statements stayed open (none do) | Documented |
| F-011 | LOW | 02 | Fixed `.tmp` name in save() (theoretical concurrent-store race) | Documented |
| F-013 | LOW | 01 | CI macOS leg failure (see F-001) | Fixed via F-001 |
| F-014 | LOW | Integration | 12 secret names duplicated across config/vault (no shared const) | Documented; revisit at Phase 04 wiring |

**Critical issues: 1 (F-001) — fixed.**
**High-severity: 2 (F-002, F-003) — fixed.**
**Medium: 2 (F-004, F-005) — fixed.**
**Low: 7 — documented (no correctness impact).**

## Tests actually executed (post-fix tree, Linux sandbox, toolchain 1.100.0)

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | CLEAN |
| `cargo check --workspace` | 0 errors, **0 warnings** (was 37 unused-dep warnings) |
| `cargo test --workspace` | **117 passed, 0 failed** across 41 test binaries |
| `cargo clippy --workspace --all-targets -- -D warnings` | 0 errors / 0 warnings |
| `cargo test -p mochy-config` | 29/29 |
| `cargo test -p mochy-vault` | 6/6 |
| `cargo test -p mochy-net` | 28/28 |
| `cargo test -p mochy-db` | 39/39 |
| `cargo test -p mochy-db --test watcher` (×50, 8 threads) | 50/50 runs green |
| `cargo test -p mochy-db --test watcher -- --test-threads=1` | 10/10 |
| CI YAML validation (`yaml.safe_load`) | valid; 3-OS matrix + fmt + clippy -D + test + patch artifact verified |

**Failed tests: none** (post-fix). Pre-fix macOS CI failure `write_fires_
broadcast_invalidation` reproduced deterministically via forced URI
collision, then eliminated.

**Environment limitations (honestly scoped):** this audit ran on Linux.
macOS/Windows behaviors — including the macOS watcher fix — are verified by
deterministic reproduction of the failure mode, collision-free-by-
construction naming, and the stress matrix; **final confirmation requires
the next GitHub Actions run on macos-latest** (CI-pending, not claimed as
verified). Per-OS keychain runtimes and the live DoH path are likewise
CI/runtime items.

## Files created or modified by this audit

**Created:**
- `docs/audits/PHASE_01_AUDIT.md`
- `docs/audits/PHASE_02_AUDIT.md`
- `docs/audits/PHASE_03_AUDIT.md`
- `docs/audits/INTEGRATION_AUDIT.md`
- `docs/audits/MASTER_AUDIT_REPORT.md` (this file)
- `docs/audits/REQUIREMENTS_MATRIX.md`

**Modified (fixes):**
- `mochy-db/src/db.rs` — F-001: unique in-memory DB names (pid + counter)
- `mochy-db/src/queries.rs` — F-002: toggle_library upstream semantics
- `mochy-db/tests/watcher.rs` — F-001 regression coverage (5 new tests)
- `mochy-db/tests/repos.rs` — F-002 regression test
- `mochy-db/tests/importer.rs` — temp-dir collision hardening
- `mochy-config/src/registry.rs` — F-003: 11 defaults + dead-key visibility
- `mochy-config/src/lib.rs` — defaults regression test
- `mochy-config/src/store.rs` — F-004: SECRET_KEYS exclusion + test
- `Cargo.toml` (root) — F-005: workspace lint config + rationale
- all 19 member `Cargo.toml`s — `[lints] workspace = true`
- `docs/HANDOFF.md` — audit entry (see below)

## Fixes completed

F-001, F-002, F-003, F-004, F-005 (all CRITICAL/HIGH/MEDIUM findings).
LOW findings are documented, not code-changed (per the fixing policy).

## Outstanding work

1. **Push and let CI run** — the macOS leg must go green to close F-001
   with runtime evidence (local reproduction + fix + stress already done).
2. F-008: commit the next settings-harvester script into the repo
   (`scripts/`) when defaults change again.
3. F-014: consider a shared secret-name constant when Phase 04 wires
   config↔vault.
4. Phase 04 proceeds per HANDOFF "Next steps" (unchanged by this audit).

## Recommended remediation order before Phase 04

1. Push `phase-03` (+ audit commits) and confirm the 3-OS matrix is green.
2. Re-generate patches/zip from the final HEAD for the phase artifacts.
3. Start Phase 04 (app shell) — no blockers remain.

---

*Audit method note: every claim in this report was verified by direct
inspection (scripts re-deriving upstream facts, byte-diffs, wire-level
test re-runs) — never by trusting HANDOFF/PROGRESS/commit messages. Where
verification was impossible in this environment, the item is marked
UNVERIFIED rather than assumed.*
