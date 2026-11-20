# Phase 01 Audit — Workspace, Documentation, License, CI

**Audit date:** 2026-11-20
**Audited tree:** branch `phase-03` @ `de79197` (+ audit fixes, see MASTER_AUDIT_REPORT)
**Method:** independent re-verification against the pinned upstream
`realidkroo/Mochy-Music` @ `bde01cb4`; no reliance on HANDOFF claims,
commit messages or PROGRESS ticks without direct evidence.

---

## 1. Root repository files

| Item | Verdict | Evidence |
|---|---|---|
| `LICENSE` complete GPL-3.0 text | **PASS** | 674 lines, correct header (`GNU GENERAL PUBLIC LICENSE Version 3, 29 June 2007`), correct footer (`END OF TERMS AND CONDITIONS`, present exactly once). Matches the plain-v3 text upstream ships (no "or later" grant — consistent with `license = "GPL-3.0-only"`). |
| NOTICE credit chain Mochy → ViviMusic → Metrolist → InnerTune/ViMusic | **PASS** | All four entries present with repos, authors, license and what each contributed. Chain matches the upstream AGENTS.md/AboutSheet lineage. |
| Bundled-work notices | **PASS** | yt-dlp EJS bundle documented as Unlicense (safe to vendor); **meriyah/astring explicitly marked NOT verified** with a "do not vendor until verified" instruction — honest, not falsely represented; symphonia MPL-2.0 status documented with the unmodified-consumption policy (no file-level disclosure triggered). |
| README accuracy | **PASS** | Describes project, lineage/licensing (GPL-3.0 + NOTICE links), baseline pin (v8.4.2 build#1271, versionCode 73, `bde01cb4`), no-telemetry stance, 19-crate layout, docs map. |
| `.gitignore` | **PASS** | `/target/`, `**/*.rs.bk`, `/patches/`, `mochy-desktop-phase-*.zip`, IDE/OS noise, `.env*`. Appropriate. |
| `rust-toolchain.toml` | **PASS** | Exact pin `channel = "1.100.0"`, profile minimal, components rustfmt+clippy. Locally verified: `cargo 1.100.0 (3acc79c0c 2026-11-05)` / `rustc 1.100.0 (0ef1bc386 2026-11-09)`. |

## 2. Cargo workspace

- **Exactly the 19 mandated members** — verified by reading `Cargo.toml`
  `members` and cross-checking each directory. No extras, none missing.
- **Every crate has `Cargo.toml` + entry point + doc comment + test module**
  (scripted check; all 19 OK). Top doc comments carry the PORT_SPEC §3
  responsibility text verbatim; each manifest cites its upstream absorption
  in the header comment.
- **Entry points:** `mochy` is a `bin` (println stub — documented as the
  Phase 04 seam), the other 18 are `lib`.
- **Feature flags:** `exclusive-win` (wasapi), `exclusive-linux` (alsa),
  `usb-uac` (rusb), `video` (ffmpeg-next) declared on `mochy-audio` with
  `optional = true` workspace deps — verified in `mochy-audio/Cargo.toml`.
  Optionality is real (`dep:` wiring in `[features]`). The alsa 0.11 pin
  constraint comment (cpal links-conflict) is present and correct.
- **Dependency centralization:** all shared pins live in root
  `[workspace.dependencies]` with per-dep rationale comments. Phase 02/03
  additions (`toml`, `paste`, `hickory-resolver`, `tokio-rusqlite`) are used
  by their crates. No circular dependencies (no inter-crate deps exist yet).
- **Cargo.lock committed** (application workspace; reproducible builds).
- **Layering:** no GPUI/tokio types in mochy-config / mochy-vault public
  APIs (grep + read). No `cfg(target_os)` outside leaf code. mochy-net and
  mochy-db use tokio in their APIs as documented infrastructure/actor
  deviations (see INTEGRATION_AUDIT).

## 3. PORT_SPEC.md sections

All 12 mandated sections present and substantive: mission + baseline pin
(§1), verified-facts corrections (§2), 19-crate table with responsibility +
upstream absorption (§3) + layering rules (§3.1) + feature flags (§3.2),
dead-code list (§4) + latent route-bug decision (§4.1), omissions table
(§5), keymap (§6), theme tokens (§7), per-OS matrix (§8), dependency stack
(§9), phase→milestone map (§10), definition-of-done (§11), testing strategy
(§12, summary). The §2 verified corrections were re-checked during the
Phase 02/03 audits (398 static keys ✓, 24+3 schema ✓, 14 migrations ✓,
schemas stop at v34 ✓) and none were silently changed.

## 4. HANDOFF.md / PROGRESS.md

- Phase 01 entry present with Status / Decisions (10) / Deviations (3) /
  Next steps — matches the template; content cross-checked against the tree
  (toolchain pin, dep pins, vibrato-vs-lindera reasoning, alsa constraint,
  CI patch-artifact design).
- PROGRESS.md: 25-phase checklist present; Phase 01 items ticked and
  accurate.
- Phase 02/03 entries exist and were verified in their own audit files.

## 5. CI (`.github/workflows/ci.yml`)

- YAML parses (Python `yaml.safe_load`). Two jobs: `test` (matrix
  `windows-latest, ubuntu-latest, macos-latest`, `fail-fast: false`) and
  `patches` (ubuntu, `fetch-depth: 0`, `git format-patch origin/main..HEAD`,
  artifact upload with `if-no-files-found: ignore`).
- Steps: pinned-toolchain install parsed from `rust-toolchain.toml` (no
  second drifting copy), `cargo fmt --all --check`, `cargo clippy --workspace
  --all-targets -- -D warnings`, `cargo test --workspace`.
- Commented Phase 24 release-workflow stub present with the plan of record.
- Working-directory behavior: default workspace root — correct.
- **Observed CI state:** the macOS leg failed `mochy-db/tests/watcher.rs`
  (root cause + fix in PHASE_03_AUDIT §F-001); 37 `unused workspace
  dependency` warnings polluted logs (remediation in
  INTEGRATION_AUDIT §5). Both are addressed by this audit's fixes;
  re-running CI is the remaining verification step.

## 6. Findings

| ID | Severity | Finding | Status |
|---|---|---|---|
| F-005 | MEDIUM | 37 unused-workspace-dependency warnings on every CI build (cargo 1.100 lint). Reserved-for-future-phase pins are intentional; removed the warning via the sanctioned `[workspace.lints.cargo] unused_workspace_dependencies = "allow"` with a full rationale + phase mapping, and `[lints] workspace = true` on all 19 members. Verified: `cargo check --workspace` → 0 warnings. | **Fixed** |
| F-013 | LOW | CI's macOS leg had a real (non-warning) failure — tracked as F-001 in the Phase 03 audit file (it is a mochy-db defect, not a workspace defect). | Fixed (F-001) |

No CRITICAL or HIGH findings in Phase 01 scope.

## 7. Verdict

**Phase 01: PASS.** Every requirement verified with direct evidence; one
MEDIUM warning-hygiene issue fixed in place.
