# Patch Pilot — Strategy & Architecture Review

_October 2026. The product thinking behind Patch Pilot. Decisions that came out of it are logged in [decisions.md](decisions.md); if this doc and the decisions log disagree, the log wins._

## The short version: what to change before building

1. **"Golden Rig" should be a Golden *Profile*, not a physical machine.** The core question is "if this machine died today…". If the source of truth is a machine, then when the Golden Rig dies, the truth dies with it. The Golden Profile is a versioned manifest that lives apart from any one computer. A machine can be "seeded from" it or "promoted to" it. Users still see the simple "Golden Rig" idea in the interface.
2. **Not every machine should match everything.** A travel laptop shouldn't hold 600 GB of orchestral libraries or AAX plug-ins. We need **target profiles**, e.g. "Travel = Golden minus Spitfire, minus AAX, minus Kontakt libraries over 20 GB." The parity score measures a machine against *its own target*. Otherwise every laptop permanently shows 60% and people learn to ignore the score.
3. **Changes happen on secondary machines too.** You buy a plug-in on the laptop in a hotel. We need a clean "propose this to Golden" flow. Still one-directional (Golden wins), but changes can enter from anywhere.
4. **It syncs a manifest, not software.** Plug-ins, licenses and big libraries can't be copied. "Bring This Machine Up To Speed" is an honest **guided checklist**: launch this vault installer, open Native Access to install these 4, run iLok License Manager. Only small user-owned files actually sync: presets, scripts, templates, configs. Marketing must never promise one-click restore.
5. **The Installer Vault is weaker than it looks.** The biggest vendors (Native Instruments, Waves, UA, Arturia, Spitfire, iZotope) install through their own download managers and usually leave no reusable installer behind. The vault matters most for indie and boutique developers who ship direct downloads. Worth building, not the headline.
6. **The Patch Notes Hub is a content business hiding inside a software product.** Cut from MVP.
7. **Retention is the existential risk.** Migrations happen every 2–4 years. Ongoing value has to come from drift detection and **template and project readiness**, which matters every week.
8. **The biggest under-exploited feature is dependency awareness.** Reaper `.RPP`/`.RTrackTemplate` files are text; Ableton `.als`/`.adg` files are gzipped XML. Parse them to see which plug-ins a template or rack needs. "Your mix template will open with 3 missing plug-ins on the MacBook" beats "you're 94% in sync."

## 1. Product vision

Patch Pilot keeps every machine in an audio professional's life ready for production. It knows what your ideal setup contains, notices when a machine falls out of step, and walks you back to readiness without touching anything you didn't approve.

**North-star metric:** time to production-ready on a new or drifted machine, in hours instead of days.

## 2. Positioning

> For audio professionals who work across more than one computer, Patch Pilot is the workstation parity tool that tells you exactly what a machine is missing and guides you to fix it. Backup tools copy everything blindly and plug-in managers only see one vendor. Patch Pilot understands your whole rig (plug-ins, DAWs, scripts, templates and libraries) and never clutters your folders with sync junk.

Taglines: _"Every rig, ready."_ / _"Know your rig."_

## 3. Personas

| Persona | Setup | Pain | Willingness to pay |
|---|---|---|---|
| **Two-rig mixer** (core MVP) | Windows studio PC + MacBook, Reaper, 300+ plug-ins | Sessions break on the laptop | Medium; hates subscriptions |
| **Reaper power user** | Heavy ReaPack, custom actions, themes, SWS | Rebuilding the Resource folder by hand | Medium-high; vocal community |
| **Ableton producer / performer** | Studio + touring laptop, Max for Live, racks | "Will my set open?" | Medium; strongest emotional pain |
| **Post / game audio team lead** | 5–30 seats, mixed OS | Seat parity, onboarding, audits | **High** — the real money |
| **Educator / lab admin** | 20–60 lab machines | Lab drift | High budget, slow sales |

Build for the first three; sell to team leads by year 2.

## 4. Competitive landscape

From general knowledge — re-scan the market before pitching anyone.

| Category | Examples | Why it isn't Patch Pilot |
|---|---|---|
| Vendor managers | Native Access, Waves Central, UA Connect, Arturia Software Center, iLok License Manager | One vendor each, no cross-machine view |
| Backup / imaging | Time Machine, Macrium, Backblaze | Copies everything, same-OS restore, no idea what a plug-in is |
| File sync | Dropbox, iCloud, OneDrive, Syncthing | Creates the conflict-copy junk we want to eliminate |
| Package / dotfile managers | winget, Homebrew, chezmoi | Dev-oriented, no audio catalog; good architectural inspiration |
| DAW-native | ReaPack, Ableton Packs / Cloud | Own content only |
| **Status quo** | Spreadsheets, screenshots, "deal with it when it breaks" | **The real competitor** |

Defensible advantages: (a) cross-platform canonical plug-in identity catalog, (b) deep Reaper/Ableton understanding incl. dependency parsing, (c) sync that never creates conflict files.

## 5. MVP

**In:**
- Plug-in inventory (VST2, VST3, AU, CLAP, AAX) by **reading metadata only — never loading plug-ins**
- Canonical product matching across formats and OSes
- Golden Profile, target profiles with exclusions, parity score per machine
- Machine comparison view and "Up To Speed" guided checklist
- Reaper: Resource folder sync (scripts, JSFX, themes, toolbars/menus, `reaper-kb.ini`, templates), ReaPack package-list restore, path remapping in `reaper.ini`
- Ableton: User Library sync (racks, presets, templates, M4L devices, clips) + plug-in dependency checks
- Vendor apps and iLok: detect installed/version, launch
- Installer Vault: Downloads watcher, manual import, hashing, organized by vendor/version, on **user-owned storage**
- Snapshot before every apply, rollback
- Drift notifications via filesystem watchers

**Out:** Patch Notes Hub, update detection for third-party plug-ins (needs catalog), Ableton `Preferences.cfg`, third-party preset sync (curated top-50 later), team features, other DAWs.

## 6. Architecture

```
+--------------------- Desktop app (per machine) ---------------------+
|  UI (webview)  <-->  Core service (Rust)                            |
|                      |- Scanners: plugins / reaper / ableton /      |
|                      |            vendor apps / folders             |
|                      |- Watcher (FSEvents / ReadDirectoryChangesW)  |
|                      |- Parity engine (diff + scoring)              |
|                      |- Snapshot + content-addressed store          |
|                      |- Apply engine (staged, atomic)               |
|                      '- Vault manager                               |
|  SQLite (local index)          Blob store (user files only)         |
+-------------------------------+-------------------------------------+
                                |  E2E-encrypted manifests + small blobs
                 +--------------v-------------+   +----------------------------+
                 |  Sync relay (dumb store)   |   |  Catalog service           |
                 |  cannot read user data     |   |  plugin identities,        |
                 +----------------------------+   |  vendor app versions       |
                                                  +----------------------------+
```

The client does all the intelligent work. The server stores encrypted data it can't read, plus a public catalog.

## 7. Tech stack

- **Tauri 2 + Rust core** — ~30–80 MB idle vs 200+ MB for Electron; fast startup; Windows and Mac first-class. Rust for FS walking, BLAKE3 hashing, PE/plist/XML parsing.
- **UI:** Svelte or React + Tailwind (open decision). Raycast-style command palette.
- **Storage:** SQLite via `rusqlite` + content-addressed blob directory.
- **Sync relay:** Cloudflare Workers + R2, or Fly.io + S3; managed auth (Clerk / Supabase Auth).
- **Signing:** Apple Developer ID + notarization; Azure Trusted Signing on Windows. Budget from day one.
- **RigSync:** proposal — absorb as the Reaper/config module rather than maintaining two codebases (open decision).

## 8. Local-first vs cloud

Local-first with a cloud relay. Every machine works offline. The cloud only: moves manifests and small blobs between machines, keeps Golden Profile history off any single machine, and serves the catalog. Never use the user's Dropbox/iCloud folder as transport (conflict copies). LAN peer-to-peer later for cloud-averse studios.

## 9. Database (per machine, SQLite)

| Table | Contents |
|---|---|
| `machines` | id, name, os, arch, target_profile_id, last_seen |
| `products` | canonical_id, vendor, name (catalog cache) |
| `items` | id, machine_id, kind (plugin / daw_asset / vendor_app / license_tool / folder / config), product_id, format, platform, version, path, content_hash, size, mtime, platform_portable |
| `snapshots` | id, machine_id, created_at, reason, parent_id, manifest_hash |
| `snapshot_items` | snapshot_id, item_id, hash (immutable) |
| `profiles` + `profile_rules` | Golden and target profiles; include/exclude by vendor, kind, format, size, glob |
| `path_maps` | machine_id, logical name (e.g. `SAMPLES_ROOT`), physical path |
| `vault_entries` | hash, vendor, product_id, version, os, arch, filename, source, verified_sig |
| `dependencies` | asset item → product (from templates, racks, projects) |
| `events` / `decisions` | Drift log; ignored diffs so warnings don't repeat |

## 10. Parity engine

1. **Identity resolution** (the hard, valuable part). VST3: class ID is identical on Windows and Mac; `moduleinfo.json` when present. AU: type/subtype/manufacturer from `Info.plist`. CLAP: reverse-DNS ID. VST2/AAX: bundle plist on Mac, PE version resource on Windows, then fuzzy catalog match.
2. **Normalize** to (product, kind, format, platform, version, hash).
3. **Filter** through the target profile; mark portable vs platform-specific (JSFX portable, SWS binary not).
4. **Diff** into: missing, outdated, newer-than-golden, modified, extra, n/a-on-this-OS.
5. **Score** with weights (a plug-in a template needs >> a theme). Always show reasons: "92% · 3 plug-ins your templates need are missing."
6. **Plan** ordered actions: vault install, vendor-app handoff, file sync, manual step — with dependencies (iLok before PACE plug-ins).

## 11. Snapshots

- Immutable hashed JSON manifest + refs into a content-addressed blob store. Blobs **only for user-owned files**; plug-ins/libraries are metadata only.
- Taken on: first scan, before every apply, on drift acceptance, daily if changed, manually.
- Diff chain from parent with periodic full snapshots; dedupe by hash.
- Rollback restores user files exactly and produces a reverse checklist for software.
- Retention: everything 30 days, then weekly/monthly. Golden Profile snapshots forever.

## 12. Conflict resolution

1. **3-way detection** — Golden, target, last common snapshot. One side changed = no conflict.
2. **Classify** by file type; key-level diff for `.ini` (e.g. `reaper-kb.ini`); "which version?" for binaries.
3. **Explain** in plain language.
4. **Recommend**: keep Golden / promote to Golden / merge by key (ini only) / ignore permanently.
5. **Snapshot, then apply atomically** — stage, fsync, rename. Refuse to touch DAW configs while the DAW runs (Reaper rewrites `reaper.ini` on exit).

**Hard rules:** never write `(1)` / "conflict copy" files; never leave hidden sync folders in user directories (state lives in app-data); never apply without a snapshot.

## 13. Installer Vault

- **Location:** user-chosen (external SSD, NAS, own cloud drive). We do **not** host installers (EULA/redistribution risk, cost).
- **Layout:** `Vault/<Vendor>/<Product>/<version>/<os-arch>/file` + `vault.db`. Human-readable even if Patch Pilot disappears.
- **Intake:** watch Downloads for .exe/.msi/.pkg/.dmg/.zip; identify by filename, code signature, catalog; ask once, then move/copy.
- **Integrity:** BLAKE3 + signature check; warn when the signer changes.
- **Restore:** launch with user present. Never silent installs.

## 14. Security

- Scans reveal what someone owns and their folder layout: **E2E-encrypt manifests**, device-derived keys, user-held recovery key.
- Never touch license credentials, iLok passwords or vendor sessions.
- Least privilege: narrow privileged helper for `/Library/Audio/Plug-Ins`; avoid Full Disk Access.
- Sign/notarize everything; signed auto-updates.
- Vault installers are a malware vector: verify signatures, alert on signer change, never auto-execute.
- Catalog contributions opt-in, stripped of paths/usernames.

## 15. Vendor & licensing challenges

- **No public APIs.** We read undocumented local files (Native Access, Waves, …). Isolate each in an adapter with fixture tests; degrade to "unknown".
- **iLok/PACE:** detect License Support install + version; cannot query licenses. Say so in the UI.
- **Update detection** needs a latest-version source per product: vendor adapters → community-observed versions → scraping last.
- **EULAs:** personal vault on user storage is generally reasonable; hosting/sharing installers isn't. Lawyer review before launch.
- **Plug-in naming is chaos** ("x64", "(Mono)", "Stereo"). Budget real time for matching.

## 16. UX & onboarding

1. Install → first scan (<60 s for 500 plug-ins, metadata only) → **"Here's your rig."**
2. "Make this your Golden Rig?" → profile + first snapshot.
3. **Readiness:** "Your 4 Reaper templates depend on 61 plug-ins, all present."
4. Pair second machine with a code.
5. Second machine scans, shows score, offers **"Bring this machine up to speed."**

Tray/menu-bar app + Cmd/Ctrl+K palette. Three-pane window (Machines · Diff · Detail). Daily digest, not a ping per change.

## 17. New machine migration

1. Install, pair, choose target profile ("Studio clone" / "Travel").
2. Scan + diff → phased plan:
   - **Foundation:** DAWs, iLok, vendor managers
   - **Vault installs:** batched, user present
   - **Vendor handoffs:** "Open Native Access and install these 7"
   - **DAW assets:** Reaper Resource sync, ReaPack restore, User Library
   - **Paths:** map `SAMPLES_ROOT` etc. to new drives
   - **Manual:** authorizations, Kontakt re-linking
3. Live progress via rescans.
4. Readiness check until every template is green.
5. Export migration report.

## 18. Pricing

| Tier | Price | Includes |
|---|---|---|
| **Free** | $0 | 1 machine: inventory, readiness checks, shareable Rig Report |
| **Pro** | $59 one-time (1 yr updates; ~$29/yr to continue) | Unlimited personal machines, Golden Profile, LAN/manual sync, Vault, snapshots |
| **Pro Sync** | ~$4/mo or $39/yr | Hosted encrypted sync, cloud Golden history, update alerts |
| **Studio** | ~$12/seat/mo, min 5 | Fleet dashboard, shared profiles, onboarding, audit reports |

## 19. Monetization

- Year 1: Pro licenses. Long term: **Studio** (post, game audio, schools).
- Labeled, optional **affiliate links** for missing plug-ins.
- Later: vendor "verified integration" partnerships.
- Never sell usage data. Say so publicly.

## 20. Go-to-market

1. **Reaper community first** — Cockos forum, r/Reaper, ReaPack-aware features.
2. **Free Rig Report** as the viral loop.
3. Creator content: "I rebuilt my studio PC in 3 hours."
4. **Ableton second** — touring/live performers.
5. **Studio** — 3–5 design partners from our own network.

## 21. Risks

| Risk | Severity | Mitigation |
|---|---|---|
| Low usage between migrations → churn | High | Readiness checks, drift digest, template dependencies |
| Wrong identity matches → lost trust | High | Conservative matching, visible confidence, one-click corrections feeding the catalog |
| Vendor formats change | Medium | Adapter isolation, fixture tests, graceful "unknown" |
| Sync damages a DAW config | **Critical** | Snapshot before write, DAW-running check, atomic writes, heavy beta logging |
| Catalog maintenance | Medium | Community-observed versions; defer patch notes |
| Scope creep | High | Every feature must answer "does this make a machine production-ready?" |
| Two-founder friction | Medium | Written agreement, area ownership, decisions log |
| Vendor builds it | Low-med | They won't go cross-vendor; acquisition target instead |

## 22. 12-month roadmap

| When | Milestone |
|---|---|
| M1–2 | Rust scanners (Win + Mac, all formats), identity v1, SQLite index, first-scan UI. Closed alpha, 30 Reaper users. |
| M3–4 | Golden Profile, target profiles, comparison + score, snapshots, LAN/file manifest exchange. |
| M5–6 | Reaper module (Resource sync, ReaPack restore, path remap, template deps). Public beta, Rig Report. |
| M7–8 | Ableton module, Installer Vault, vendor app + iLok detection. |
| M9 | Hosted encrypted sync, migration workflow v1. **Pro launch.** |
| M10–12 | Catalog service, update alerts, top-30 preset locations, Studio design partners. |

## 23. 3-year vision

- **Year 1:** best parity tool for Reaper + Ableton on Windows + Mac.
- **Year 2:** Studio/Edu fleets; Bitwig, Logic, then Cubase/Nuendo; Patch Notes Hub on catalog data; client-session readiness.
- **Year 3:** **the identity layer for audio software** — session portability between studios, vendor update partnerships, a public API.

## Verdict

Patch Pilot can become the definitive parity tool if we (1) build the Golden *Profile* with target profiles, (2) lead with readiness and dependency checks rather than the vault, (3) treat the plug-in identity catalog as the core asset, and (4) aim the business at studios and teams by year 2.
