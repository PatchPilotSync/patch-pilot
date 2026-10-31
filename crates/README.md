# Crates

The Rust core of Patch Pilot. Each folder here is one crate in the root Cargo
workspace, published to nobody and named `patchpilot-<folder>`. The desktop app
(`app/src-tauri`) and the CLI are thin layers over these.

| Crate | What it does |
| --- | --- |
| [`core`](core) | Shared domain types: `Item`, `Product`, `Manifest`, `Snapshot`, target profiles, logical paths. Changes here are reviewed by both founders. |
| [`scanner`](scanner) | Finds plug-ins (VST2, VST3, AU, AAX, CLAP), DAW files and vendor apps on Windows and macOS. Reads metadata only; never loads a plug-in. |
| [`catalog`](catalog) | One identity per product across formats and OSes, known vendors, latest versions. Data in `catalog/data/catalog.json` ([docs/catalog.md](../docs/catalog.md)). |
| [`parity`](parity) | Matches products between machines, diffs manifests, scores parity with reasons, and builds the Get Up To Speed plan. |
| [`store`](store) | SQLite schema and migrations, plus the content-addressed blob store for snapshots. |
| [`local`](local) | This machine: default folders, running a scan and saving it, app-data paths. Shared by the CLI and the app. |
| [`sync`](sync) | Live sync core: rig keys, link codes, end-to-end encryption of what goes to the relay. No networking. |
| [`vault`](vault) | Installer Vault: installers kept on the user's own drive, identified, hashed and sorted by vendor and version. |
| [`cli`](cli) | `patchpilot scan` and `patchpilot diff` for alpha testers ([docs/cli.md](../docs/cli.md)). |

## How they depend on each other

From the bottom up (each layer uses only the ones above it in this list):

1. `core`: no Patch Pilot dependencies.
2. `scanner`, `catalog`, `store`, `sync`: each uses `core` only.
3. `parity`: uses `core` and `catalog`.
4. `vault`: uses `core` and `parity`.
5. `local`: brings all of the above together for this machine.
6. `cli` and the app (`app/src-tauri`): the top. Keep them thin and put logic in a crate.

## Tests

```sh
cargo test --workspace            # everything
cargo test -p patchpilot-parity   # one crate
```

Tests read fixtures from [`../fixtures`](../fixtures), never the developer's
real plug-ins.
