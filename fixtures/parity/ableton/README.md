# Ableton Live settings

Stand-ins for an Ableton User Library and Live's per-version preferences
folders. File contents are placeholders; only paths and hashes matter.

- `User Library/`: what is shared (templates, racks, presets, default
  presets, Max for Live devices, clips, Collections `.xmp`) and what never is
  (`Samples/`, project `Backup/` folders, other file types).
- `prefs-mac/`: `Live x.y.z/` folders as on macOS. The newest release
  (12.1.5) wins over an older release and over a newer beta. Only
  `Options.txt` is shared, and only the Collection names from `Library.cfg`
  (its structure follows a real Live 12.4 file; paths are made up).
  `Preferences.cfg` never is.
- `prefs-win/`: the same on Windows, where files sit in `Preferences/`.
