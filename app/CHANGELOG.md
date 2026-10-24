# Changelog

What changed in each version of the Patch Pilot app. The section for a version
becomes its release notes, which the app shows when it offers that update
(see `docs/releasing.md`). Write for users, not developers.

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
