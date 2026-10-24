# Plug-in catalog

`crates/catalog/data/catalog.json` tells Patch Pilot which vendor and product
a plug-in really is, whatever its format or OS calls it, and (optionally) the
newest release. See decision #44.

## Editing it

- **Vendors:** `id` is the slug used in product IDs and never changes once
  shipped. Put other names plug-ins report (company names, short forms) in
  `aliases`. Company suffixes (`GmbH`, `Inc`, `LLC`, `AB`, ...) are ignored
  anyway.
- **Products:** `name` as the vendor brands it; `aliases` for other spellings
  (with or without the vendor's name in front). Only add products whose
  spellings actually differ between formats or OSes, or that should resolve
  when no vendor can be read.
- **Latest versions:** add `"latest": { "version": "4.0.2", "released": "2026-10-01" }`
  only after checking the vendor's site. The version must be written the way
  the plug-in reports it. A wrong value shows every user a false update alert.
- Bump `updated` (`YYYY-MM-DD`) on every change: the app only takes a fetched
  catalog that is newer than the one it has.
- Run `cargo test -p patchpilot-catalog` (it checks the file loads and that
  every product's vendor exists).

## Publishing

Merging a change to main runs `.github/workflows/catalog.yml`, which copies
the file to `catalog.json` on the public `PatchPilotSync/patch-pilot-releases`
repo. Apps pick it up within a day, without a new release. The same file is
compiled into the next app release as the offline fallback.
