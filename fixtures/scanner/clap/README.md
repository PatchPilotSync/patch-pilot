CLAP fixtures (#8). Vendors are made up; PE files are resource-only stubs from
`fixtures/tools/make-pe-stub.sh` (no code).

| File | Covers |
|---|---|
| `windows/Acme Synth.clap` | PE version resource on a Windows `.clap` |
| `windows/Mystery Box.clap` | A `.clap` Reaper hasn't cached: file metadata only |
| `macos/Acme Synth.clap/` | macOS `.clap` bundle plist |
| `reaper/reaper-clap-win64.ini` | Reaper CLAP cache: two plug-ins in one file, a name without vendor, junk lines, an empty section |
| `reaper-real/reaper-clap-win64.ini` | A real Reaper cache from a Windows machine (14 FabFilter plug-ins, CRLF, kept byte for byte; it holds no paths or usernames) |

The cache format is not documented by Cockos. The Windows file confirmed our
reading (#32); a real macOS `reaper-clap-macos-*.ini` is still wanted.
