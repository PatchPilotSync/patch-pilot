# Decisions log

One line per decision, newest at the bottom. Format: `YYYY-MM-DD — Decision — Why — Status`.
Status is **Accepted** (both founders agreed), **Proposed** (needs the other founder's OK), or **Superseded by #N**.
Claude: never undo an Accepted decision without adding a new line that supersedes it.

| # | Date | Decision | Why | Status |
|---|---|---|---|---|
| 1 | 2026-10-23 | Patch Pilot syncs manifests, not software. Only small user-owned files (presets, scripts, templates, configs) sync. | Licensing/iLok make copying plug-ins illegal or impossible; libraries are hundreds of GB. | Accepted |
| 2 | 2026-10-23 | The source of truth is a **Golden Profile** (versioned manifest), not a physical machine. | If the Golden machine dies, the truth must survive. | Accepted |
| 3 | 2026-10-23 | Machines are scored against **target profiles** (Golden minus exclusions). | Laptops shouldn't need every library/AAX plug-in; otherwise scores are always red and get ignored. | Accepted |
| 4 | 2026-10-23 | Scanners read metadata only; never load plug-in binaries in-process. | Loading crashes and is slow. | Accepted |
| 5 | 2026-10-23 | MVP DAWs: Reaper and Ableton Live only. MVP OSes: Windows and macOS. | Depth over breadth; mixed-OS rigs are common. | Accepted |
| 6 | 2026-10-23 | Desktop stack: Tauri 2 + Rust core + SQLite. | Small idle footprint, fast startup, strong cross-platform FS/parsing. | Accepted |
| 7 | 2026-10-23 | Local-first. Cloud relay only stores E2E-encrypted manifests/small blobs. Never use the user's Dropbox/iCloud as transport. | Privacy; avoids conflict-copy files. | Accepted |
| 8 | 2026-10-23 | Installer Vault lives on user-owned storage; we never host installers. | EULA/redistribution risk and storage cost. | Accepted |
| 9 | 2026-10-23 | Patch Notes Hub, third-party preset sync and team features are out of MVP. | Scope; content-heavy. | Accepted |
| 10 | 2026-10-23 | Repo lives in a shared GitHub org with both founders as owners. | Clean ownership; no single point of failure. | Accepted |
| 11 | 2026-10-23 | Using React for UI framework | Claude writes React more reliably | | Accepted |
| 12 | 2026-10-23 | Relationship to RigSync - Might abandon RigSync if PatchPilot can do everything RigSync can | Accepted |
| 13 | 2026-10-23 | Ownership split: 50/50 | we'll decide as we go who handles what | Accepted |
| 14 | 2026-10-23 | Does PatchPilot run through Sabins Sound LLC? | The LLC hasn't been formed as of yet | Proposed |
| 15 | 2026-10-23 | Rust workspace at repo root, crates named `patchpilot-*`, edition 2024, stable toolchain, `Cargo.lock` committed, `unsafe_code` forbidden by default. CI = fmt + clippy `-D warnings` + tests on Windows and macOS. | Reproducible builds for an app; one place to change lints. | Proposed |
| 16 | 2026-10-23 | Manifest JSON v1: portable paths (`{logical:{root,rel}}` or `{absolute}`), times as Unix seconds, raw vendor version strings, per-item identity `confidence`, readers refuse newer `schema_version`. JSON Schema generated from the Rust types (`docs/manifest.schema.json`). | Windows and macOS manifests must compare directly; one source of truth for the format. | Proposed |
| 19 | 2026-10-23 | Local DB: SQLite (`rusqlite`, bundled), WAL, foreign keys on, embedded SQL migrations tracked by `PRAGMA user_version`; refuse DBs from newer builds. `items` rows are immutable and keyed by BLAKE3 of the item JSON, so snapshots share unchanged rows. DB at `%LOCALAPPDATA%\PatchPilot` (not roaming) / `~/Library/Application Support/PatchPilot`. | Snapshots stay readable forever without copying every item; roaming profiles would sync a per-machine index. | Proposed |
