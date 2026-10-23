# Handoff: where Patch Pilot stands

_Written 2026-10-23, after milestone **M1-2: Scanner alpha** closed. For whichever founder (and their Claude) picks up the next milestone. If this file and `docs/decisions.md` disagree, the decisions log wins._

## What's built (M1-2, PRs #17-#29)

A Rust workspace with five crates, green CI on Windows and macOS, and an alpha CLI. No desktop app yet.

| Crate | What it does |
|---|---|
| `patchpilot-core` | Manifest JSON v1 types (`Item`, `Product`, `Manifest`), portable logical paths, `TargetProfile` rules. Schema in `docs/manifest.schema.json`, explained in `docs/manifest.md`. |
| `patchpilot-scanner` | Finds VST2/VST3/AU/AAX/CLAP folders per OS and reads metadata only: VST3 `moduleinfo.json`, AU `Info.plist`, PE version resources, bundle plists, Reaper's CLAP cache. Detects vendor apps and iLok. Never loads a plug-in. |
| `patchpilot-parity` | Name-based product identity, diff into parity states, target profiles, weighted score with reasons, Reaper template dependency parser. |
| `patchpilot-store` | SQLite schema v1 with migrations, immutable BLAKE3-keyed item rows, snapshots, default DB path per OS. Nothing calls it yet. |
| `patchpilot-cli` | `patchpilot scan` and `patchpilot diff` for alpha testers. |

Tests use fixtures under `fixtures/`, never real installed plug-ins.

## How to run it

Needs stable Rust ([rustup.rs](https://rustup.rs)).

```sh
cargo test --workspace
cargo run -p patchpilot-cli -- scan --name "Studio PC" --out studio.json
cargo run -p patchpilot-cli -- diff studio.json laptop.json --profile travel.json
```

The full alpha-tester instructions are in the README. CI runs `cargo fmt --check`, clippy with `-D warnings` and tests on both OSes.

## Decisions waiting on you

Rows **#14-#21** in `docs/decisions.md` are **Proposed**: the LLC question, workspace/CI conventions, manifest format, diff states, profile rules and score weights, SQLite layout, name-based identity, and the CLAP approach. Most were made by Claude while building M1-2 so work could continue. Read them and accept or supersede each one; issue #30 tracks this. Everything else (React for the UI, Tauri + Rust + SQLite, manifests not software, Golden Profile, target profiles) is Accepted.

## Loose ends from M1-2

- **Reaper CLAP cache format is a guess.** The parser in `crates/scanner/src/clap_cache.rs` was written without a real `reaper-clap-*.ini`. If either of us has Reaper with CLAP plug-ins, grab the file and do issue #32.
- **MSRV is wrong.** `json5 1.3` needs a newer compiler than the declared `rust-version = "1.85"`. Issue #31.
- **Template readiness isn't in the score yet**, even though the parser and the ×3 weight exist. Issue #37.
- **The store isn't wired up.** Scans only go to JSON files. Issue #34.
- **Closed alpha** (30 Reaper users sending scans) hasn't started. The CLI is ready for it.

## Next milestone: M3-4: Golden Profile and comparison

GitHub milestone #2, issues **#30-#42**. Suggested order (later ones depend on earlier ones):

1. Founder items: #30 decisions, #31 MSRV, #32 CLAP cache (needs a real file)
2. Core plumbing: #34 save scans to the store → #35 Golden Profile → #36 stored target profiles
3. Parity: #37 template readiness, #39 Up To Speed plan, #38 file-based manifest exchange
4. App: #33 Tauri + React scaffold → #40 first scan screen → #41 comparison view
5. #42 performance benchmark, any time

The plumbing issues each add a CLI command, so they are testable before the app exists.

## How to drive it with Claude, hands-off

This is how M1-2 was built, almost entirely by Claude:

1. In your own Claude project, add the `PatchPilotSync/patch-pilot` repo. Each session reads `CLAUDE.md` first, which points to the strategy, the decisions log and the conventions.
2. Tell Claude something like: _"Work through milestone M3-4 in the suggested order in docs/handoff.md. One issue per branch and PR. Open PRs without asking; only stop when you need a decision from me."_
3. Claude claims an issue, branches as `<area>/<issue#>-short-name`, writes fixture-based tests, keeps CI green on both OSes and adds a row to `docs/decisions.md` for any product or architecture choice.
4. Review rule from CLAUDE.md: the other founder reviews and merges. If you want Claude to merge its own green PRs, say so explicitly for that batch.

Tips from M1-2:

- Branches stacked on different bases conflict on `Cargo.lock` and on `docs/decisions.md` rows. Merge `main` in, keep the branch's lock, run `cargo metadata` to fill the gaps, and keep decision rows in numeric order.
- Keep PRs small. If an issue grows, ask Claude to split it into new issues.
- The non-negotiable rules in CLAUDE.md (never load plug-ins, never copy licensed software, no conflict files, snapshot before writes) apply to every PR. Push back on any PR that bends them.
