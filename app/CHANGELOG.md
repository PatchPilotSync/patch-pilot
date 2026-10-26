# Changelog

What changed in each version of the Patch Pilot app. The section for a version
becomes its release notes, which the app shows when it offers that update
(see `docs/releasing.md`). Write for users, not developers.

## 0.4.14 - 2026-10-26

### Fixed

- Syncing with GitHub: the first upload could fail with "GitHub didn't take the files" followed by codes. Uploads now go through, and if GitHub ever turns one down, the message says why in words.
- Files inside a `.git` folder (a script folder that is its own Git checkout) stay on the machine instead of stopping the upload.

## 0.4.13 - 2026-10-26

### Fixed

- Compare no longer offers **Get Up To Speed** for another machine. Installing happens on the machine that needs it, so looking at your Mac from your PC now says "Get Up To Speed runs on that machine" with a **See its checklist** link. The tray does the same.
- When a backup can't be opened, Patch Pilot now says why (still downloading, removed, damaged, or held by your cloud app), tries again on its own, and shows it under the backup list with **Try again** instead of a red bar across the window.

## 0.4.12 - 2026-10-26

### Improved

- Syncing with GitHub now handles big rigs. The first upload of thousands of files goes up in a few large batches instead of one file at a time, so GitHub no longer asks Patch Pilot to slow down. If an upload is interrupted, it picks up where it stopped on its own.
- A machine joining GitHub sync downloads the other machine's files in one go.
- The Sync & backup line shows progress ("Uploading 3,200 of 15,456 files to GitHub…"), tells you what to do next on your other machine, and has a **View on GitHub** link that opens your repo.
- Choosing a GitHub repo turns on Keep Files in Sync. While syncing, **Sync with** only lists your other machines, not this one.

## 0.4.11 - 2026-10-26

### Improved

- Files left out of a backup now have **Show in Finder** (Mac) or **Show in Explorer** (Windows), which opens their folder with the file selected. Duplicate presets have the same link on each copy when you point at it.

## 0.4.10 - 2026-10-26

### Improved

- The GitHub option is easier to find: **Use GitHub…** now sits beside Change folder on This machine's Sync & backup line, instead of only appearing after you click Change folder.

## 0.4.9 - 2026-10-26

### Improved

- Keep Files in Sync now catches a machine up to your Primary Rig. When two machines sync for the first time and a file differs, the Primary Rig's copy wins (you can undo it in History) instead of asking about every file.
- The Sync & backup line says what syncing is doing: how many files from each machine are already the same, brought over, still downloading, or waiting for you to choose. **Check now** looks in your cloud folder straight away.
- More reliable pickup from Dropbox and OneDrive on Mac, which don't always report files they download: Patch Pilot looks again a few minutes after another machine scans, and keeps checking while files are still arriving.
- "Restore" from a backup is now called **Sync with**, so it's clearer what it does.
- "Files skipped" in a backup now explains itself: click it to see each file, where it is, why it was left out (too big, with its size, or couldn't be read) and what to do.

## 0.4.8 - 2026-10-26

### New

- Advanced: sync with GitHub. Under This machine → Sync & backup, you can keep backups and synced files in a private GitHub repo instead of a cloud folder. Every change is kept in the repo's history.

### Improved

- This machine opens shorter: your whole plug-in list now sits in one All plug-ins folder (with its count, vendors and any updates in the header), like Preset folders and Vendor apps. Open it once and it stays open.

### Fixed

- The first launch after an update could show "index snapshot_items_item already exists" and leave Compare greyed out until Patch Pilot was restarted. Your data was never affected.

## 0.4.7 - 2026-10-26

### Improved

- Much faster on big rigs. Opening Patch Pilot, Compare and the tray no longer stall, and background rescans no longer keep your CPU busy. The cause was how "last scanned" was looked up, which got slower with every plug-in, preset and saved scan.
- No more blank window: while Patch Pilot loads you see the logo and what it's doing, in the main window and the tray.
- Compare shows the last result at once when you come back to a machine and refreshes it in the background.

## 0.4.6 - 2026-10-26

### New

- Keep your files in sync. Pick your cloud folder (Dropbox, OneDrive, Google Drive, iCloud Drive or any folder) in the tour or on This machine, and new or changed Reaper templates, FX chains, scripts and themes, Ableton User Library files and plug-in presets reach your other linked machines on their own the next time Patch Pilot runs there. A quiet notification says what came in, and History can undo it.
- It's careful: files deleted on one machine stay on the others, settings like key maps never sync on their own, ignored files stay out, and Ableton files wait until Live is closed. If a file changed on two machines, Patch Pilot keeps both as they are and asks which copy you want, with no "conflicted copy" files.
- "Keep Files in Sync" in the tray menu turns it off and on.

### Improved

- The top bar now says "Machines linked" instead of "Live sync on", so it isn't mixed up with syncing files.
- One cloud folder for everything: picking it for syncing also sets it for big files.

## 0.4.5 - 2026-10-26

### New

- Bring over all your templates at once: "Bring over all templates" in Compare (or "Copy all templates here" when you look at another machine) and "All templates" on the Bring over card bring every Reaper track and project template and every Ableton template. The Bring over card also has "All presets".
- Big files no longer need a Send click. When you bring over a file too big for the link and a cloud folder is picked on this machine, Patch Pilot asks for it through that folder: the machine that has it sends it by itself the next time Patch Pilot runs there, and this machine puts it in place when it arrives. Big files in the list show which service they'll come through, such as "Via Dropbox".

### Improved

- Cleaner, more consistent layout across every screen.

## 0.4.4 - 2026-10-26

### New

- Back up everything to your cloud folder. A Backup line on This machine copies your Reaper templates, FX chains, scripts, themes and key maps, your Ableton templates and settings, your plug-in presets and the Installer Vault list into a "Patch Pilot Backup" folder in Dropbox, OneDrive, Google Drive, iCloud Drive or any folder you pick. Click Back up now, or tick "Back up after every scan that finds a change".
- Each backup copies only what changed, and earlier versions are kept, so you can go back to any backup. Patch Pilot never deletes anything in that folder, and it's laid out by app so you can browse it yourself.
- Restore from a backup on any machine, Mac or PC: pick a backup and what to restore, and each file goes to the right place, with Undo in History. A new machine that finds another machine's backup offers to restore it, and Set up has a "Restore from a backup" choice.

### Fixed

- Script packages brought over from a saved scan now keep every file they need. This needs a rescan with this version.

## 0.4.3 - 2026-10-26

### New

- Ignore anything you don't want on a machine. Hover over a file, a folder, a script package, a whole app's files, a missing or outdated plug-in in Compare, a Get Up To Speed step, or a suggestion for the Primary Rig, and click Ignore. It stops being asked for and leaves every count: the parity score, Get Up To Speed, Bring over, the tray and alerts.
- "N ignored" under each list shows what you ignored. Un-ignore brings it back, and you can choose whether an ignore applies to this machine only or to every linked machine. Ignored suggestions can be suggested again.
- Files too big for the link (up to 64 MB) can travel through a cloud folder you already use, such as Dropbox, OneDrive, Google Drive or iCloud Drive, or any folder you pick. The machine that needs a file places it by itself once it has fully arrived, and the copy in the cloud folder is removed once no linked machine needs it.
- Find and clean up duplicate presets on This machine (undo in History).

## 0.4.2 - 2026-10-26

### Improved

- Scripts arrive whole. A script that loads other files (a library, images, settings), every file of a ReaPack package, a JSFX with its imports, and a theme with its image folder now come over together, so they work on arrival. If one of those files is too big to send over the link, Patch Pilot says so instead of bringing a script that would fail.
- Bring over whole folders from Compare: each Reaper folder has a "Bring over" button for this machine, and when you look at another linked machine, "Copy here" copies its files straight onto this one.
- One button for all of them: "Bring over all Reaper scripts" in Compare (or "Copy all Reaper scripts here" when you look at another machine), and "All scripts" on the Bring over card, bring every script and ReaPack package at once, each with the files it needs. Anything too large to send is left out and counted.
- Bring over now carries plug-in presets (FabFilter, Serum, Vital, u-he, Valhalla and more) between linked machines; files over 1.9 MB show as too large to send for now.
- In Bring over, every folder has a checkbox, and ticking a script ticks the files it needs.
- Compare shows when a script is missing files it needs on a machine.
- A script that loads files from another folder (a shared library, a theme's images) now counts once among the suggestions for the Primary Rig, together with those files. This needs a rescan with this version on the machine that has them.

### Fixed

- Bring over no longer keeps offering a key map this machine already has. The same shortcuts saved on a Mac and on a PC now count as the same, and the list refreshes after every Bring over.
- When everything is here, the Bring over card turns green and says it's up to date, and when something is skipped it says why.

## 0.4.1 - 2026-10-26

### Improved

- Suggestions for the Primary Rig say what they are ("3 plug-ins, 12 scripts and 2 templates") instead of one big number, and a script package counts once: the files in one Scripts folder are one script, and everything ReaPack installs is one. In Compare they're listed in folders by kind, and a whole package can be added or ignored in one click.

## 0.4.0 - 2026-10-26

### Improved

- Reaper files in Compare and Bring over fold by folder: Scripts, Themes, FX chains, templates and configuration each get a folder, and so does every subfolder inside them (ReaTeam Scripts, Items Editing...). Copies a sync app like Dropbox made of a file ("conflicted copy") are marked, and Bring over leaves them unticked.

### Changed

- The Golden Rig is now called the Primary Rig, everywhere in the app. Nothing about how it works has changed, and machines on older versions still sync with it.
- On the machine the Primary Rig was made from, things added or found since read "New since v1" instead of "Not in Primary", and one button, "Update Primary Rig", makes the next version from that machine's latest scan.
- Preset folder rows (Arturia presets, Kilohearts presets, ...) now show where the folder is and how big it is, and say what Patch Pilot does with it.
- "no profile" on Compare now reads "whole rig".

### Fixed

- Compare really stays on the machine you click now. With a big rig, the slower comparison of this machine could finish after the one you picked and replace it on screen, while the sidebar still showed your pick.

## 0.3.1 - 2026-10-25

### New

- "Check for Updates…" in the menu bar or tray icon's menu checks right away and tells you if you're up to date.

### Improved

- Long lists fold into folders. Plug-ins on This machine sit in one folder per vendor (or format), with a count and how many have updates; click to open, or Expand all. Typing in the filter opens the folders that match. Compare, Get Up To Speed, Bring over, Reaper setup, the Installer Vault, History and the Rig Report fold the same way, and Patch Pilot remembers which folders you left open.
- Get Up To Speed crosses steps off by itself. Install something on the list and, a few seconds after the installer finishes, its step is ticked and marked "Installed" instead of disappearing. Steps that are partly done tick off each plug-in as it arrives, and "Mark done" covers anything Patch Pilot can't see. Installing a vendor app or iLok now triggers a rescan too, not just plug-ins.
- "Scanned" times now show when Patch Pilot last checked the machine, even when nothing had changed.

### Fixed

- Live sync works again on machines with thousands of ReaPack scripts. Their list was too big for the sync service, so the machine showed "Sync offline". Uploads are now compressed. Update every linked machine: older versions can't read the compressed lists.
- Compare stays on the machine you picked. A sync or rescan landing in the background could jump it back to this machine.
- Patch Pilot now notices new versions while it keeps running in the menu bar or tray: it checks again when you open its window, at most once an hour.
- Arturia Audio Units show their real version (1.1.5) instead of a long number (65797).
- ReaPack scripts show as one "ReaPack: 8,071 packages" row in Compare and its suggestions instead of thousands of rows, and alerts mention them once.
- The button for adding something to the Golden Rig names the machine it comes from, e.g. "Add Windows PC's copy to Golden Rig". The "your other machines show up here" note in Compare is gone once a machine is linked.

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
