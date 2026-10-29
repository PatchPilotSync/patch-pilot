# What keeps in sync between machines

With **Keep Files in Sync** on, each machine backs up after every change and
places what's new, changed or renamed on its linked machines (decisions #70,
#74, #77). Every placement is a restore point that History can undo.

## Syncs on its own

| App | What | When the app must be closed |
| --- | --- | --- |
| Reaper | Track templates, project templates, FX chains | no |
| Reaper | Scripts (ReaScripts and the files they load), JSFX and data from ReaPack packages | no |
| Reaper | Themes (`ColorThemes/`) and custom toolbar icons (`Data/toolbar_icons/`) | no |
| Reaper | FX presets saved in Reaper's preset menu (`presets/*.ini`) | no |
| Reaper | Shortcuts and custom actions (`reaper-kb.ini`; script paths point at each machine's own folder; on a Mac, shortcuts macOS keeps for itself such as Cmd+H move to Control, decision #81) | yes |
| Reaper | Menus and toolbars (`reaper-menu.ini`) | yes |
| Reaper | Mouse modifiers (`reaper-mouse.ini`) | yes |
| Reaper | Theme adjuster settings (`reaper-themeconfig.ini`) | yes |
| Reaper | Startup action, the action SWS runs whenever Reaper opens (only `GlobalStartupAction` in SWS's `S&M.ini`; the rest of that file stays per machine, decision #108) | yes |
| Ableton Live | Templates (including `Default.als`), default presets, racks, presets, Max for Live devices, clips, grooves, tunings | yes |
| Ableton Live | Collection names, and which Collections and browser categories are shown (removed with the sidebar's Edit); each machine keeps its own User Library path | yes |
| Ableton Live | Places in Dropbox (or another cloud folder) or the User Library, pointed at the same folder on each machine (decision #79) | yes |
| Ableton Live | What's tagged into Collections (the `.xmp` files in the User Library's `Ableton Folder Info`) | yes |
| Ableton Live | Control surface scripts in the User Library (`Remote Scripts/`) and `Options.txt` | yes |
| Plug-ins | Preset folders of the vendors Patch Pilot knows, the AU and VST3 preset folders, and folders named "Presets" inside a known vendor's folder (decision #89); a folder laid out differently on a Mac and a PC syncs only between machines of the same kind | no |

Renaming or moving any of these files renames or moves it on the other
machines too, as long as their copy wasn't edited there.

## Never syncs on its own

| What | Why |
| --- | --- |
| Deleted files | A delete on one machine never removes the file elsewhere. |
| Plug-ins, apps, sample libraries | They need their installers and licences (Get Up To Speed and the Installer Vault help). |
| Reaper `reaper.ini` | It mixes preferences with the audio device, MIDI ports and folder paths of that machine. |
| Reaper screensets (`reaper-screensets.ini`) | Window positions depend on each machine's screens. |
| Reaper FX browser folders and tags (`reaper-fxfolders.ini`, `reaper-fxtags.ini`) | They name plug-ins by file, and Mac and PC files differ. |
| Reaper `reaper-extstate.ini` | Scripts keep window positions and machine paths there. |
| Reaper folder settings, ReaPack repositories | Set up from Get Up To Speed's Reaper setup card instead. |
| Live `Preferences.cfg` | Binary, and holds the audio device, MIDI, plug-in folders and login. |
| Live Places outside a cloud folder and the User Library (another drive) | Only that machine has the folder; each machine keeps its own. |
| Collection tags on samples outside the User Library | Live keeps them beside the samples, in folders Patch Pilot doesn't sync. |
| Plug-ins tagged into Collections | The tag travels, but Live only shows it once the plug-in is installed; Windows and Mac plug-in names can differ. |
| Installer Vault index | Restored on purpose from the backup, not synced. |

## How files travel

Through the cloud folder picked (Dropbox, Google Drive, OneDrive, iCloud or
any folder), a private GitHub repo (decision #73), or **straight over your
home network** with no cloud at all (decision #94). Over the home network,
linked machines find each other on the same Wi-Fi or wired network and copy
files directly, encrypted with the key only linked machines hold; changes
move while both machines are on that network. The first time, macOS asks to
let Patch Pilot find devices on the local network, and Windows may ask to
allow it through the firewall on private networks: say yes to both.

## Choosing what syncs

Under Sync & backup, **Choose what syncs** lists each of the kinds above
under Reaper, Ableton Live and Plug-ins, all on by default. Untick one on a
machine to keep that machine's own: changes to it neither come there nor go
from there to the others (decision #80). Backups still have it.

## Sync with

"Sync with" on the Sync & backup line brings over another machine's latest
backup by hand, for example right after linking. While syncing is on it only
offers each machine's latest backup, and what it brings over stays in sync
afterwards like anything placed on its own (decision #78).
