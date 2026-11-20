# Mochy Music for Desktop

A native, behavior-preserving desktop port of **Mochy Music** for Android —
the YouTube Music client with a hand-rolled DSP platform, a karaoke-grade
lyrics engine, a runtime module/skin system and a phone-to-TV link —
re-implemented in **Rust + [GPUI](https://gpui.rs)** for **Windows, Linux and
macOS**.

Parity baseline: Mochy Music for Android **v8.4.2 build#1271
(versionCode 73)**, commit `bde01cb4`. Later Android releases are *not* the
target; deltas are triaged post-parity.

## Lineage and license

Mochy Music for Desktop is a derivative work in the fork chain
**Mochy → ViviMusic → Metrolist → InnerTune/ViMusic** and is released under
**GPL-3.0** (see [LICENSE](LICENSE) and [NOTICE](NOTICE) for the full credit
chain and bundled-works notes). The credit chain is preserved on purpose:
this project only exists because those projects do.

No telemetry, no analytics, no crash reporting — logs stay on your machine.

## Project layout

- `docs/BLUEPRINT.pdf` — the full 61-page engineering blueprint (authoritative).
- `docs/PORT_SPEC.md` — the distilled, working port specification.
- `docs/HANDOFF.md` — per-phase log: decisions, deviations, next steps.
- `docs/PROGRESS.md` — the 25-phase / 9-milestone checklist.
- 19 Cargo workspace crates: the GPUI app shell, four UI crates over pure
  domain crates (player, audio, downloads, lyrics, importer, modules, link,
  integrations) over infrastructure crates (innertube, db, config, vault,
  media, net).

## Building

**Status: Phase 01 bootstrap.** The workspace compiles and tests green, but
there is no application UI yet — see `docs/PROGRESS.md` for where the port
stands.

Requirements (placeholder — filled in as phases land):

```sh
# Rust 1.100.0 (pinned by rust-toolchain.toml; rustup installs it automatically)
rustup toolchain install 1.100.0

# Build & test everything
cargo build --workspace
cargo test  --workspace
cargo clippy --workspace --all-targets -- --deny warnings
```

Platform-specific system dependencies (GPUI, audio backends, webviews) will
be listed here as the implementing phases land; CI
(`.github/workflows/ci.yml`) is the reference for the exact per-OS setup.

## Contributing

One phase at a time: read `docs/HANDOFF.md` first, then `docs/PORT_SPEC.md`.
Keep `cargo test` green and clippy clean on all three operating systems.
