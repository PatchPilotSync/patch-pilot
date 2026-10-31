# Patch Pilot desktop app

The app people install: a Tauri 2 shell in Rust with a React + TypeScript UI.
It's its own npm project and its own Cargo workspace (with its own
`Cargo.lock`), and it uses the core crates in [`../crates`](../crates) by path.

## Run it

Needs Node 22 and stable Rust. On Linux you also need the WebKitGTK 4.1 dev
packages; CI builds the app on Windows and macOS only.

```sh
npm ci
npm run tauri dev      # opens the app with hot reload
npm run build          # front end only (tsc + vite)
cd src-tauri && cargo clippy --all-targets -- -D warnings && cargo test
```

Keep `@tauri-apps/api` and `@tauri-apps/cli` on the same minor version as the
`tauri` crate in `src-tauri/Cargo.lock`, or `tauri build` refuses to run.

## Where things are

| Path | What's there |
| --- | --- |
| `src/main.tsx` | Entry point. One front end serves two windows: the main window and the tray popover. |
| `src/App.tsx` | The main window and its tabs |
| `src/screens/` | One file per screen: `Rig` (This machine), `Compare`, `Setup`, `Vault`, `History`, `Settings`, `Tray`, first run (`Welcome`, `Scanning`) |
| `src/*.tsx` | Panels and dialogs used across screens (Link, Backup, Bring over, Ignore, Rig Report, Tour, Update...) |
| `src/ui.tsx`, `src/index.css` | Shared components and the "Mica + Frost" look |
| `src/api.ts` | Typed wrappers for every Tauri command. Fields Rust leaves out when empty must be optional here. |
| `src-tauri/src/lib.rs` | Tauri commands, the UI's only way into Rust. Keep them thin; logic belongs in `crates/`. |
| `src-tauri/src/*.rs` | App-only glue: tray, background work, sync backends (relay, cloud folder, GitHub, home network), alerts |
| `src-tauri/tauri.conf.json` | Window, bundle, updater and signing settings |
| `CHANGELOG.md` | What changed in each version, written for users. Each section becomes that version's release notes. |

Releasing a new version is covered in [docs/releasing.md](../docs/releasing.md).
