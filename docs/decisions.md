# Decisions log

One line per decision, newest at the bottom. Format: `YYYY-MM-DD — Decision — Why — Status`.
Status is **Accepted** (both founders agreed), **Proposed** (needs the other founder's OK), or **Superseded by #N**.
Claude: never undo an Accepted decision without adding a new line that supersedes it.

| # | Date | Decision | Why | Status |
|---|---|---|---|---|
| 1 | 2026-10-23 | Patch Pilot syncs manifests, not software. Only small user-owned files (presets, scripts, templates, configs) sync. | Licensing/iLok make copying plug-ins illegal or impossible; libraries are hundreds of GB. | Proposed |
| 2 | 2026-10-23 | The source of truth is a **Golden Profile** (versioned manifest), not a physical machine. | If the Golden machine dies, the truth must survive. | Proposed |
| 3 | 2026-10-23 | Machines are scored against **target profiles** (Golden minus exclusions). | Laptops shouldn't need every library/AAX plug-in; otherwise scores are always red and get ignored. | Proposed |
| 4 | 2026-10-23 | Scanners read metadata only; never load plug-in binaries in-process. | Loading crashes and is slow. | Proposed |
| 5 | 2026-10-23 | MVP DAWs: Reaper and Ableton Live only. MVP OSes: Windows and macOS. | Depth over breadth; mixed-OS rigs are common. | Proposed |
| 6 | 2026-10-23 | Desktop stack: Tauri 2 + Rust core + SQLite. | Small idle footprint, fast startup, strong cross-platform FS/parsing. | Proposed |
| 7 | 2026-10-23 | Local-first. Cloud relay only stores E2E-encrypted manifests/small blobs. Never use the user's Dropbox/iCloud as transport. | Privacy; avoids conflict-copy files. | Proposed |
| 8 | 2026-10-23 | Installer Vault lives on user-owned storage; we never host installers. | EULA/redistribution risk and storage cost. | Proposed |
| 9 | 2026-10-23 | Patch Notes Hub, third-party preset sync and team features are out of MVP. | Scope; content-heavy. | Proposed |
| 10 | 2026-10-23 | Repo lives in a shared GitHub org with both founders as owners. | Clean ownership; no single point of failure. | Accepted |

## Open questions (decide, then move into the table)

- UI framework: Svelte or React?
- Relationship to RigSync: absorb as the Reaper/config module, or keep separate and Patch Pilot stays inventory-only?
- Ownership split and names for scanner vs parity/UI areas.
- Who signs the founders' agreement, and does it run through Sabins Sound LLC?
