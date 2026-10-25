# `patchpilot` command-line tool

For developers and alpha testers. The CLI scans a machine and compares two machines;
it isn't part of the app installer.
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

Make a saved scan your Primary Rig, then evolve it in versions:

```sh
patchpilot primary init                         # this machine's latest saved scan becomes v1
patchpilot primary propose --from <machine-id> --product "Pro-Q 3"   # take products from another machine
patchpilot primary promote --machine <machine-id>                    # replace Primary Rig with a machine's scan
patchpilot primary history
patchpilot primary show --out golden.json       # then: patchpilot diff primary.json laptop.json
```

Every change is a new version; old versions stay readable (`primary show --version 1`).

Compare a machine against your Primary Rig rig:

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
patchpilot diff --machine "Travel MacBook"        # latest saved scan vs latest Primary Rig, with its profile
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

Move scans, the Primary Rig and your profiles to another machine with one file
(USB stick, shared folder, email to yourself):

```sh
patchpilot export --out rig-bundle.json                 # this machine's latest scan + Primary Rig + profiles
patchpilot export --out rig-bundle.json --all-machines  # every machine in the database
patchpilot import rig-bundle.json                       # on the other machine
```

Import never overwrites anything. Scans are added as new snapshots, your own
profiles win over ones with the same ID, and Primary Rig versions are added only if
this machine's Primary Rig is behind; if both machines changed it, nothing is
imported and you pick with `primary promote` or `primary propose`. The bundle has
no plug-ins or files, only what `scan` reads, and usernames are removed unless
`--keep-paths`.

Scans also read Reaper's track templates, project templates and FX chains
(from the standard Reaper folder, or `--reaper-dir` for a portable install).
`diff` then lists templates that aren't ready on the machine and weighs the
plug-ins they need ×3 in the score.

Known alpha limits: CLAP plug-ins get exact names and vendors only if Reaper has
scanned them (otherwise name and version come from the file), product
matching is by vendor and name, and JSFX used in templates aren't checked.
