# Contributing to Patch Pilot

Patch Pilot is built by two founders, each working with Claude. These are the
house rules for anyone (person or Claude) changing the code. The full project
rules, including the non-negotiables, are in [CLAUDE.md](CLAUDE.md).

## Repo map

| Folder | What's there |
| --- | --- |
| [`crates/`](crates/README.md) | The Rust core: scanning, comparing, storing, syncing |
| [`app/`](app/README.md) | The desktop app (Tauri + React) |
| [`relay/`](relay/README.md) | The small sync relay (Cloudflare Worker) |
| [`fixtures/`](fixtures/README.md) | Test inputs that stand in for real machines |
| [`docs/`](docs/README.md) | Strategy, decisions, formats, release steps |
| `.github/` | CI, release and catalog workflows, issue and PR templates |

## Build and test

```sh
cargo build --workspace
cargo test --workspace
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
```

The app and relay have their own steps in [app/README.md](app/README.md) and
[relay/README.md](relay/README.md). CI runs format, lint and tests on Windows
and macOS for every pull request; keep both green.

## How work flows

- **One issue, one branch, one pull request.** Branch names look like
  `<area>/<issue#>-short-name`, for example `scanner/12-vst3-reader`.
- **Keep pull requests small.** If one grows, split the work into new issues.
- **Record decisions.** Any product or architecture choice gets a numbered
  row in [docs/decisions.md](docs/decisions.md) in the same pull request.
- **Test with fixtures**, never with the plug-ins installed on your machine.
- **Think Windows and macOS** for every path, scanner and test.
- **User-facing changes** get a line in [app/CHANGELOG.md](app/CHANGELOG.md).

## The rules that never bend

Patch Pilot never loads a plug-in to scan it, never copies plug-ins or
licensed software between machines, never creates conflict copies, never
touches a DAW's settings while the DAW is open, never handles license
credentials and never runs an installer without the user. Details and the rest
of the list are in [CLAUDE.md](CLAUDE.md#non-negotiable-rules).

## Outside contributions

Patch Pilot isn't open for outside pull requests yet. Bug reports and ideas are
welcome as [issues](https://github.com/PatchPilotSync/patch-pilot/issues).
When contributions open up:

- Open an issue to discuss anything bigger than a bug fix before writing code.
- Keep each pull request to one change you can explain line by line.
- Large unrequested refactors, including AI-generated ones, are closed without
  review. The founders build with Claude too, but every change is reviewed
  against the rules above and its decision is written down.

## Security

Found a security problem? Please don't open a public issue; see
[SECURITY.md](SECURITY.md).
