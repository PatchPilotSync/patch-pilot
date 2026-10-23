# Fixtures

Test inputs that stand in for real machines. Tests must use these, never the
developer's installed plug-ins, DAWs or vendor apps (CLAUDE.md, Workflow).

## Layout

```
fixtures/
  <area>/            # scanner, parity, store, reaper, ableton, ...
    <case>/          # one folder per scenario, named for what it covers
      ...            # the files the code under test reads
```

Examples: `fixtures/scanner/vst3/moduleinfo-present/`, `fixtures/parity/win-vs-mac/`.

## Rules

- **Metadata only.** A fake plug-in bundle is a folder tree with its metadata
  files (`Info.plist`, `moduleinfo.json`, a tiny PE with a version resource).
  Never commit a real plug-in binary, installer, preset pack or sample.
- **Small.** Keep each fixture to a few KB. Generate large trees in the test
  instead of committing them.
- **No personal data.** Use made-up vendors, product names and usernames
  (`/Users/alice`, `C:\Users\alice`). No real license, serial or account data.
- **Both OSes.** Where behaviour differs, add a Windows and a macOS case.
  Fixture paths must not depend on the OS running the test.
- **Explain non-obvious cases** with a short `README.md` in the case folder.

Load fixtures relative to the workspace, e.g.
`concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/scanner/...")`.
