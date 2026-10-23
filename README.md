# Patch Pilot

**Every rig, ready.** A desktop workstation parity tool for audio professionals on Windows and macOS.

Patch Pilot compares each of your machines against a Golden Profile (your ideal rig) and tells you exactly what's missing, outdated or different: plug-ins, DAW scripts and templates, vendor apps, licensing tools and folder layouts. Then it walks you back to parity, without ever creating conflict files or touching anything you didn't approve.

> Status: pre-alpha. Nothing to install yet.

## Docs

- [CLAUDE.md](CLAUDE.md) — project rules and conventions (read by Claude Code every session)
- [docs/strategy.md](docs/strategy.md) — product strategy and architecture review
- [docs/decisions.md](docs/decisions.md) — decisions log (authoritative)
- [docs/handoff.md](docs/handoff.md) — what's built, open decisions and the next milestone
- [docs/founders-agreement-checklist.md](docs/founders-agreement-checklist.md) — things to agree in writing

## MVP scope

Plug-in inventory (VST2/VST3/AU/CLAP/AAX) · Golden and target profiles · machine comparison and parity score · Reaper and Ableton asset sync with dependency checks · Installer Vault · drift detection · snapshots and rollback.

## Alpha testers: `patchpilot` CLI

Before the app exists, the CLI scans a machine and compares two machines.
It only reads: it never loads plug-ins, changes DAW settings or touches
anything except the file you ask it to write.

Build it (needs [Rust](https://rustup.rs)):

```sh
cargo build --release -p patchpilot-cli
# binary: target/release/patchpilot (patchpilot.exe on Windows)
```

Scan each machine:

```sh
patchpilot scan --name "Studio PC" --out studio.json
patchpilot scan --name "Travel MacBook" --out laptop.json
```

It reads the standard VST2/VST3/AU/AAX/CLAP folders plus vendor apps
(Native Access, Waves Central, iLok, ...). Add other plug-in folders with
`--folder vst2=D:\VSTPlugins` (repeatable). Your username is removed from
paths by default, so the file is safe to send us; `--keep-paths` keeps it.

Compare a machine against your Golden rig:

```sh
patchpilot diff studio.json laptop.json
patchpilot diff studio.json laptop.json --profile travel.json   # e.g. no AAX
```

A profile is a small JSON file; rules are checked in order and the last match wins:

```json
{ "id": "travel", "name": "Travel", "rules": [
  { "action": "exclude", "match": { "format": "aax" } },
  { "action": "exclude", "match": { "size_over": 20000000000 } }
] }
```

Known alpha limits: CLAP plug-ins get exact names and vendors only if Reaper has
scanned them (otherwise name and version come from the file), product
matching is by vendor and name, and template readiness isn't in the score yet.

## Contributing

One issue → one branch → one PR, reviewed by the other founder. See CLAUDE.md.

## License

Proprietary. All rights reserved. Not for redistribution.
