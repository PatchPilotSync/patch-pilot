# Changelog

What changed in each version of the Patch Pilot app. The section for a version
becomes its release notes, which the app shows when it offers that update
(see `docs/releasing.md`). Write for users, not developers.

## Unreleased

### New

- "Check for Updates…" in the menu bar or tray icon's menu checks right away and tells you if you're up to date.

### Improved

- Get Up To Speed crosses steps off by itself. Install something on the list and, a few seconds after the installer finishes, its step is ticked and marked "Installed" instead of disappearing. Steps that are partly done tick off each plug-in as it arrives, and "Mark done" covers anything Patch Pilot can't see. Installing a vendor app or iLok now triggers a rescan too, not just plug-ins.
- "Scanned" times now show when Patch Pilot last checked the machine, even when nothing had changed.

### Fixed

- Patch Pilot now notices new versions while it keeps running in the menu bar or tray: it checks again when you open its window, at most once an hour.
- Arturia Audio Units show their real version (1.1.5) instead of a long number (65797).

## 0.3.0 - 2026-10-24

### New

- Bring over: copy your Golden Rig's own files to the machine you're on with one click. For Reaper that means templates, FX chains, scripts, themes, key maps and menus. For Ableton it means templates, presets, racks, Collections and Options.txt. Files travel encrypted between your linked machines. Plug-ins, sample libraries and anything licensed are never copied.
- Reaper setup: Get Up To Speed restores missing ReaPack repositories and packages, and points Reaper's folder settings at this machine's folders.
- Ableton: Collection names stay the same on every machine, and Patch Pilot warns when a set or rack uses a plug-in this machine doesn't have.
- History and undo: Patch Pilot saves a restore point before it changes any file. Undo a change, or go back to an earlier scan, from History. It never writes to Reaper or Ableton files while that app is open.
- Set up a new machine: a guided checklist that ticks itself off as you install things, then ends with a setup report you can save.
- Installer Vault: keep your installers in one folder you choose. Patch Pilot spots new ones in Downloads, runs the right installer for whatever is missing, and opens vendor apps and iLok for you. It never runs an installer without you.
- Propose to Golden: when a machine has a plug-in, or a newer version of one, that the Golden Rig lacks, Compare offers to add it. Add updates the Golden Rig for every linked machine; Ignore stops suggesting that version.
- Drift alerts: a quiet notification when a machine falls behind the Golden Rig or picks up something new. Turn them off with "Drift Alerts" in the tray menu.
- Update alerts: find out when a newer version of a plug-in is on another machine, in the Golden Rig, or available from its maker.
- Preset folders: third-party preset folders are found and compared between machines.
- Share a Rig Report: a web page or text listing of your plug-ins and score for forums or collaborators. It never includes file paths or licence details.

### Improved

- "Templates not ready" in Compare is now one tidy line. Expand it to see which plug-ins are missing, grouped by app, and which templates use each one. Reaper's built-in plug-ins no longer show as missing.
- The Up To Speed button is now called Get Up To Speed.
- The tour covers the new features.

## 0.2.0 - 2026-10-24

### New

- Link your machines live over the internet. Click "Link a machine" in the top bar, show a code on one machine and type it on the other. Linked machines appear in Compare by themselves and stay current as each one changes. Everything is encrypted on your machine before it leaves, so the sync server can't read your rig.
- Patch Pilot now opens when you log in and keeps watching quietly from the menu bar or tray. When you install, update or remove a plug-in, it rescans by itself a few seconds later, so every score stays current. Turn this off with "Open at Login" in the tray icon's menu.
- A short tour on first launch explains the Golden Rig, comparisons and the parity score, Up To Speed, and the tray or menu bar icon. Skip it any time, and reopen it from "How it works".

### Improved

- Easier to get from the tray to the full window: a labelled Expand button, clickable rows, and double-click anywhere in the popover. On Windows, double-click the tray icon.
- Clearer wording throughout: the main screen now says it lists the plug-ins installed on this machine, and each screen says in a line what it shows. Patch Pilot only reads metadata and never changes anything.

## 0.1.0 - 2026-10-23

### New

- First scan of this machine: "Here's your rig", with plug-ins by vendor and format.
- Make this machine your Golden Rig.
- Compare machines against the Golden Rig, with a parity score, the reasons behind it and an Up To Speed checklist.
- Drop a `patchpilot export` file on the window to add another machine.
- A compact look with light and dark themes that follow your system, and the new Patch Pilot icon.
- Patch Pilot lives in the menu bar (Mac) or system tray (Windows): click the icon for every machine's score and what the worst one needs.
- Automatic updates: Patch Pilot checks for a new version at launch and shows what changed before you install it.
