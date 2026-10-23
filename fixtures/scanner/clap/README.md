CLAP fixtures (#8). Vendors are made up; PE files are resource-only stubs from
`fixtures/tools/make-pe-stub.sh` (no code).

| File | Covers |
|---|---|
| `windows/Acme Synth.clap` | PE version resource on a Windows `.clap` |
| `windows/Mystery Box.clap` | A `.clap` Reaper hasn't cached: file metadata only |
| `macos/Acme Synth.clap/` | macOS `.clap` bundle plist |
| `reaper/reaper-clap-win64.ini` | Reaper CLAP cache: two plug-ins in one file, a name without vendor, junk lines, an empty section |

The cache format is our reading of Reaper's file and is not documented by
Cockos. Check it against a real `reaper-clap-*.ini` when one is available.
