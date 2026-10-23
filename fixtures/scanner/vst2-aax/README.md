VST2 and AAX fixtures (#13). Vendors are made up; PE files are resource-only
stubs from `fixtures/tools/make-pe-stub.sh` (no code).

| File | Covers |
|---|---|
| `windows/Acme Comp x64.dll` | PE version resource; `x64` stripped from the name |
| `windows/Acme EQ Mono.dll` | Clean ProductName, channel tag only in the file name |
| `windows/Acme Gate (64 bit).dll` | PE with **no** version resource: name from file, low confidence |
| `windows/Acme Verb.aaxplugin/` | Windows AAX bundle, PE at `Contents/x64/` |
| `macos/Acme Comp.vst/` | macOS VST2 bundle plist |
| `macos/Acme Verb.aaxplugin/` | macOS AAX bundle plist |
