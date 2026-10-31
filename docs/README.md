# Patch Pilot docs

Start with the [project README](../README.md) for what Patch Pilot is and how to install it.
This page lists everything else in `docs/`, grouped by who it's for.

## Using the app

| Doc | What's in it |
| --- | --- |
| [what-syncs.md](what-syncs.md) | Exactly which files keep in sync between linked machines, and which never do |
| [cli.md](cli.md) | The `patchpilot` command-line tool for alpha testers: scan a machine, diff two scans |
| [../app/CHANGELOG.md](../app/CHANGELOG.md) | What changed in each version (also shown in the app's update offer) |

## Building and shipping

| Doc | What's in it |
| --- | --- |
| [../CONTRIBUTING.md](../CONTRIBUTING.md) | How to build, test and open a pull request |
| [../crates/README.md](../crates/README.md) | The Rust crates and how they fit together |
| [../app/README.md](../app/README.md) | The desktop app: React screens and the Tauri shell |
| [../relay/README.md](../relay/README.md) | The sync relay (Cloudflare Worker) and its routes |
| [releasing.md](releasing.md) | How to publish a new version of the app |
| [manifest.md](manifest.md) | The manifest JSON format, with its schema in [manifest.schema.json](manifest.schema.json) |
| [catalog.md](catalog.md) | The plug-in catalog: vendor and product names across formats |
| [../fixtures/README.md](../fixtures/README.md) | Test fixtures and how to add one |
| [brand/](brand/README.md) | The logo, app icons and how to regenerate them |

## Product and project

| Doc | What's in it |
| --- | --- |
| [strategy.md](strategy.md) | Product strategy, architecture and the [12-month roadmap](strategy.md#22-12-month-roadmap) |
| [decisions.md](decisions.md) | Every product and architecture decision, numbered. **Authoritative**: if another doc disagrees, this one wins |
| [handoff.md](handoff.md) | Historical: where things stood on 2026-10-23, after the scanner alpha |
| [founders-agreement-checklist.md](founders-agreement-checklist.md) | Things for the founders to agree in writing |

`screenshots/` holds the images the README uses.
