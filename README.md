# Patch Pilot

**Every rig, ready.** A desktop workstation parity tool for audio professionals on Windows and macOS.

Patch Pilot compares each of your machines against a Golden Profile (your ideal rig) and tells you exactly what's missing, outdated or different: plug-ins, DAW scripts and templates, vendor apps, licensing tools and folder layouts. Then it walks you back to parity, without ever creating conflict files or touching anything you didn't approve.

> Status: pre-alpha. Nothing to install yet.

## Screenshots

The desktop app, shown with demo data.

| Your rig | Compare with the Golden Rig | Up To Speed |
| --- | --- | --- |
| ![This machine's plug-ins after a scan](docs/screenshots/rig.png) | ![Travel Laptop scored 40% against the Golden Rig, with reasons](docs/screenshots/compare.png) | ![Checklist to bring Travel Laptop back to parity](docs/screenshots/up-to-speed.png) |

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

Keep a history of scans on a machine with `--save` (with or without `--out`):

```sh
patchpilot scan --save --name "Studio PC"
patchpilot history
```

Saved scans go to Patch Pilot's own database in the app-data folder
(`%LOCALAPPDATA%\PatchPilot` on Windows, `~/Library/Application Support/PatchPilot`
on macOS; `--db` or `PATCHPILOT_DB` picks another file). A rescan with no
changes doesn't add a snapshot. The database keeps real paths because it never
leaves the machine.

Make a saved scan your Golden Rig, then evolve it in versions:

```sh
patchpilot golden init                         # this machine's latest saved scan becomes v1
patchpilot golden propose --from <machine-id> --product "Pro-Q 3"   # take products from another machine
patchpilot golden promote --machine <machine-id>                    # replace Golden with a machine's scan
patchpilot golden history
patchpilot golden show --out golden.json       # then: patchpilot diff golden.json laptop.json
```

Every change is a new version; old versions stay readable (`golden show --version 1`).

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

Profiles can also live in the database and be assigned per machine. Two are
built in: `studio-clone` (everything) and `travel` (no AAX, nothing over 20 GB).

```sh
patchpilot profile list
patchpilot profile import my-laptop.json          # your own rules; built-ins can't be replaced
patchpilot profile set "Travel MacBook" travel    # machine ID or name; `none` clears it
patchpilot diff --machine "Travel MacBook"        # latest saved scan vs latest Golden, with its profile
patchpilot diff                                   # same, for this machine
```

`diff` with two files also uses the machine's stored profile when that machine
is in the local database, unless `--profile` is given.

Get an ordered checklist to bring a machine up to speed (same inputs as `diff`):

```sh
patchpilot plan studio.json laptop.json
patchpilot plan --machine "Travel MacBook"
```

It lists licensing tools and vendor apps first, then "Open Native Access and
install these 2", then manual installs and DAW files to copy. It never installs
or copies anything itself, and it never suggests copying plug-ins between machines.

Move scans, the Golden Rig and your profiles to another machine with one file
(USB stick, shared folder, email to yourself):

```sh
patchpilot export --out rig-bundle.json                 # this machine's latest scan + Golden + profiles
patchpilot export --out rig-bundle.json --all-machines  # every machine in the database
patchpilot import rig-bundle.json                       # on the other machine
```

Import never overwrites anything. Scans are added as new snapshots, your own
profiles win over ones with the same ID, and Golden versions are added only if
this machine's Golden Rig is behind; if both machines changed it, nothing is
imported and you pick with `golden promote` or `golden propose`. The bundle has
no plug-ins or files, only what `scan` reads, and usernames are removed unless
`--keep-paths`.

Scans also read Reaper's track templates, project templates and FX chains
(from the standard Reaper folder, or `--reaper-dir` for a portable install).
`diff` then lists templates that aren't ready on the machine and weighs the
plug-ins they need ×3 in the score.

Known alpha limits: CLAP plug-ins get exact names and vendors only if Reaper has
scanned them (otherwise name and version come from the file), product
matching is by vendor and name, and JSFX used in templates aren't checked.

## Contributing

One issue → one branch → one PR, reviewed by the other founder. See CLAUDE.md.

## License

Proprietary. All rights reserved. Not for redistribution.
