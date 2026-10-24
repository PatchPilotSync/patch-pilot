# CLAUDE.md — Patch Pilot

Read this first, every session. Two founders work on this repo with separate Claude sessions; the repo is the only shared memory. If you make a product or architecture decision, add a line to `docs/decisions.md` in the same PR.

## What this is

Patch Pilot is a desktop workstation **parity** tool for audio professionals (Windows + macOS). It keeps every machine in a creator's workflow production-ready by comparing each machine against a **Golden Profile** and guiding the user back to parity.

It is **not** a plug-in manager, backup tool, cloud drive or license manager.

Full product thinking: `docs/strategy.md`. Decisions (authoritative): `docs/decisions.md`.

## Core concepts (use these names in code and UI)

- **Golden Profile** — versioned manifest of the ideal rig. Not tied to one machine. The UI may call it "Golden Rig".
- **Target profile** — Golden Profile plus per-machine exclusion rules (e.g. Travel = no AAX, no libraries over 20 GB).
- **Manifest** — what a machine has: plug-ins, DAW assets, vendor apps, folders. Metadata, not files.
- **Snapshot** — immutable, hashed manifest plus references to blobs of user-owned files.
- **Parity score** — a machine measured against its **target profile**, always shown with reasons.
- **Readiness** — whether templates/racks/projects on a machine have all the plug-ins they depend on.

## Non-negotiable rules

1. **Never load plug-in binaries to scan them.** Read metadata only (VST3 `moduleinfo.json`/class IDs, AU `Info.plist`, PE version resources, bundle plists). If a format truly needs loading (CLAP descriptors), do it in an isolated, killable subprocess and mark it as a decision.
2. **Never copy plug-ins, licensed software or sample libraries between machines.** We sync manifests. Only small user-owned files (presets, scripts, templates, configs) sync.
3. **Never create conflict files.** No `file (1)`, no "conflict copy", no hidden sync folders in user directories. All app state lives in the OS app-data directory.
4. **Snapshot before every write** to user files. Writes are staged then atomically renamed.
5. **Never touch a DAW's config while that DAW is running.**
6. **Never handle license credentials** (iLok, vendor accounts). Detect, don't authenticate.
7. **Never run installers silently.** The user is always present.
8. **Cross-platform from day one.** Every path, scanner and test must consider Windows and macOS. Use logical paths (`SAMPLES_ROOT`) + per-machine path maps, never hard-coded absolute paths in manifests.
9. **Performance budget:** near-zero idle CPU (use filesystem watchers, not polling), low memory, first scan of 500 plug-ins under 60 s.

## Stack (planned — see decisions.md for status)

- Tauri 2 desktop shell, Rust core
- UI: React + TypeScript + Vite + Tailwind (decision #11; scaffold in #33)
- SQLite via `rusqlite`; BLAKE3 hashing; content-addressed blob store
- Later: small sync relay (E2E-encrypted, server can't read user data) + catalog service

## Planned layout

```
crates/
  core/        # domain types: Item, Product, Manifest, Snapshot, Profile
  scanner/     # plugin + DAW + vendor-app scanners, per-OS modules
  catalog/     # plug-in catalog: vendor/product names across formats, latest versions
  parity/      # identity resolution, diff, scoring, plan generation, update alerts
  store/       # SQLite schema + migrations, blob store
  local/       # this machine: default folders, scan + save, app-data paths (shared by CLI and app)
  sync/        # live sync core: rig key, link codes, E2E encryption for the relay (no network)
relay/         # sync relay: Cloudflare Worker + Durable Objects (TypeScript, own npm project)
app/           # Tauri app: React UI in src/, Rust shell in src-tauri/ (own Cargo workspace)
docs/
```

## Ownership (to avoid PR collisions)

- **Scanner area** (`crates/scanner`, identity readers, vendor-app detection): founder A
- **Parity, store, sync and UI** (`crates/parity`, `crates/store`, `app/`): founder B
- Shared types in `crates/core` change via PR reviewed by both.

Fill in names once agreed.

## Workflow

- One GitHub issue = one focused Claude session = one branch = one PR.
- Branch names: `<area>/<issue#>-short-name`, e.g. `scanner/12-vst3-reader`.
- The other founder reviews and merges. Don't merge your own PR unless it's docs-only.
- Keep PRs small. If an issue grows, split it into new issues instead of growing the PR.
- Write tests with **fixtures** (sample plists, moduleinfo.json, reaper.ini, .adg files) under `fixtures/`. Never rely on the developer's real installed plug-ins in tests.

## Build & run

Rust workspace at the repo root; crates are named `patchpilot-<dir>` (e.g. `patchpilot-core`). Needs stable Rust (`rust-toolchain.toml` pins the channel and components).

```sh
cargo build --workspace
cargo test --workspace                              # all crates
cargo test -p patchpilot-scanner                    # one crate
cargo fmt --all                                     # CI runs --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p patchpilot-scanner --test bench -- --ignored --nocapture   # 500-plug-in scan budget
```

CI (`.github/workflows/ci.yml`) runs fmt, clippy and tests on `windows-latest` and `macos-latest` for every PR, for the crates and (separately) for the app. Keep it green on both.

The alpha CLI is `crates/cli` (`cargo run -p patchpilot-cli -- scan --out rig.json`, `... -- diff golden.json machine.json`); see docs/cli.md.

The desktop app lives in `app/` (Node 22). `app/src-tauri` is its own Cargo workspace with its own `Cargo.lock`, so Tauri's dependencies stay out of the core crates' builds; it depends on the crates by path. Keep the Tauri npm packages (`@tauri-apps/api`, `@tauri-apps/cli`) on the same minor version as the `tauri` crate in `Cargo.lock`, or `tauri build` refuses to run.

```sh
cd app
npm ci
npm run tauri dev                                  # window with hot reload
npm run build                                      # front end only (tsc + vite)
cd src-tauri && cargo clippy --all-targets -- -D warnings && cargo test
```

Tauri commands (the UI's only way into Rust) live in `app/src-tauri/src/lib.rs`; keep them thin and put logic in `crates/`. On Linux, building the app needs the WebKitGTK 4.1 dev packages; CI only builds it on Windows and macOS.
