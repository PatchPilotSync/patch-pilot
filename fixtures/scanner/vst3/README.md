VST3 metadata fixtures (#6). Product names, vendors and class IDs are made up.

| Case | What it covers |
|---|---|
| `moduleinfo-present` | macOS bundle with `moduleinfo.json` (JSON5: comments, trailing commas) and `Info.plist`; high confidence |
| `plist-only` | macOS bundle with only `Info.plist`; medium confidence, no class IDs |
| `pe-only` | Windows bundle; version resource in `Contents/x86_64-win/*.vst3` |
| `legacy-single-file` | Pre-bundle Windows `.vst3` file |
| `name-only` | Nothing readable; name from the folder, low confidence |
| `broken-moduleinfo` | Truncated `moduleinfo.json`; falls back to the PE and records a warning |

The PE files are resource-only stubs from `fixtures/tools/make-pe-stub.sh`: no code.
