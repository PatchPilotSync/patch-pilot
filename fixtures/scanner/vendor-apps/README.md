Vendor app and licensing tool fixtures (#14). No credentials or license data,
by design (CLAUDE.md rule 6).

- `windows/uninstall.json`: uninstall entries as `read_uninstall_entries()`
  would return them. Includes a renamed app (`Native Access 2`), an entry with
  no version, one whose publisher doesn't match (`UA Connect` from a
  repackager: must not count), and an entry with no name.
- `macos/root/` stands in for `/`, `macos/home/` for `~`. Covers a nested
  install path (iZotope), a per-user install (UA Connect in `~/Applications`),
  a broken `Info.plist` (Spitfire: detected, no version) and the PACE helper,
  which is presence-only.
