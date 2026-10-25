# Patch Pilot

**Every rig, ready.** Patch Pilot keeps all your music machines set up the same way, so a session that opens on your studio PC also opens on your laptop, with every plug-in in place.

For producers, mixers and engineers who work on more than one computer, on Windows and macOS.

![The tray popover: every machine's parity and what the Travel Laptop needs](docs/screenshots/tray.png)

## Why it helps

You know the moment: you open a project on the laptop and half the chain is red because a plug-in is missing, out of date, or only installed on the studio machine. Patch Pilot catches that before the session, not during it.

- **No more missing plug-ins.** See at a glance which plug-ins, versions, vendor apps and DAW templates a machine is missing.
- **One setup to aim for.** Pick your best machine as the Primary Rig. Every other machine is measured against it.
- **Know what to fix first.** A simple to-do list tells you what to install, in the right order.
- **Nothing to babysit.** It sits quietly in your menu bar or system tray and notices changes on its own.
- **Safe by design.** It only looks at what's installed. It never opens your plug-ins, changes your DAW settings, copies licensed software or runs installers behind your back.

## How it works

1. **Install it on each machine.** Patch Pilot looks at the plug-ins installed there (VST2, VST3, AU, AAX and CLAP), your vendor apps (Native Access, Waves Central, iLok and more) and your Reaper templates and FX chains.
2. **Choose your Primary Rig.** That's your ideal setup. Update it whenever your setup changes; every version is kept.
3. **Compare.** Each machine gets a **parity score** that shows how close it is to the Primary Rig, with the reasons behind the number. A travel laptop can skip things it doesn't need, like AAX or huge sample libraries.
4. **Get Up To Speed.** Patch Pilot turns the differences into a checklist, starting with what other installs depend on, like licensing tools. You install things yourself and tick them off as you go.

| Your rig | Compare with the Primary Rig | Get Up To Speed |
| --- | --- | --- |
| ![The plug-ins installed on this machine](docs/screenshots/rig.png) | ![Travel Laptop scored 40% against the Primary Rig, with reasons](docs/screenshots/compare.png) | ![Checklist to bring Travel Laptop back to parity](docs/screenshots/up-to-speed.png) |

## Features

- **Primary Rig** — the setup every machine should match, with version history.
- **Parity score** — one number per machine, always with the reasons.
- **Drift detection** — spots when a machine falls behind, like an update installed on one machine but not the others.
- **Template readiness** — tells you which Reaper templates and FX chains won't open fully on a machine, because a plug-in they use is missing.
- **Get Up To Speed** — a step-by-step checklist to bring a machine back in line.
- **Live machine linking** — link your Mac and PC with a short code and Compare updates live. What your machines share is end-to-end encrypted, so nobody else can read it, not even us.
- **Always on, barely there** — opens when your computer starts, waits in the tray or menu bar, and rescans by itself when you add or update a plug-in. It uses next to no CPU while it waits.
- **Snapshots** — every change to a machine is saved, so you can see what changed and when.
- **Automatic updates** — when a new version is out, the app shows you what's new and installs it only when you click.
- **Light and dark** — follows your system setting.

**Coming soon:** the **Installer Vault**, to keep your installers in one place on your own drive, and Ableton Live support.

## What Patch Pilot is not

It's not a backup tool, a cloud drive or a license manager, and it never moves plug-ins or sample libraries between machines. It tells you what each machine needs; you stay in control of what gets installed.

## Status

Early preview. Download the latest version for Windows or macOS from the [releases page](https://github.com/PatchPilotSync/patch-pilot-releases/releases/latest). The Mac version isn't signed yet, so the first time you open it, right-click the app and choose **Open**.

---

## For developers

Patch Pilot is a Tauri 2 desktop app (React + TypeScript UI in `app/`) on top of a Rust workspace in `crates/`, with a small Cloudflare Worker relay for live linking in `relay/`.

```sh
cargo test --workspace          # core crates
cd app && npm ci && npm run tauri dev   # the desktop app
```

- [CLAUDE.md](CLAUDE.md) — project rules, layout and build commands
- [docs/cli.md](docs/cli.md) — the `patchpilot` command-line tool for alpha testers
- [docs/strategy.md](docs/strategy.md) — product strategy and architecture
- [docs/decisions.md](docs/decisions.md) — decisions log (authoritative)
- [docs/handoff.md](docs/handoff.md) — what's built and what's next
- [docs/releasing.md](docs/releasing.md) — how to publish a release
- [docs/founders-agreement-checklist.md](docs/founders-agreement-checklist.md) — things to agree in writing

One issue, one branch, one PR, reviewed by the other founder. See CLAUDE.md.

## License

Proprietary. All rights reserved. Not for redistribution.
