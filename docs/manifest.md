# Manifest JSON

A manifest is what one machine has: products and items, metadata only. Types
live in `crates/core` (`patchpilot-core`); the machine-readable schema is
[`manifest.schema.json`](manifest.schema.json), generated from those types.
A test fails if the two drift; regenerate with
`UPDATE_SCHEMA=1 cargo test -p patchpilot-core`.

Example: [`fixtures/core/manifest-v1/studio-pc.json`](../fixtures/core/manifest-v1/studio-pc.json).

## Shape (v1)

| Field | Meaning |
|---|---|
| `schema_version` | `1`. Readers refuse versions newer than they know. |
| `machine` | `id` (random, per install), `name`, `os`, `arch`, `os_version`, `path_map` |
| `generated_at` | Scan end time, Unix seconds UTC |
| `products[]` | `canonical_id`, `vendor`, `name`: one entry per product across formats and OSes |
| `items[]` | One installed thing; see below |

Item fields: `kind` (`plugin`, `daw_asset`, `vendor_app`, `license_tool`,
`folder`, `config`), `product_id` (absent until identity resolution places it),
`format` (`vst2`, `vst3`, `au`, `aax`, `clap`, `jsfx`), `platform`, `arch`,
`version` (raw vendor string), `path`, `content_hash` (`blake3:<hex>`, only for
user-owned files that may sync), `size`, `mtime` (Unix seconds),
`platform_portable`, `confidence` (`low`, `medium`, `high`), `requires`
(product IDs the item needs, sorted and unique; set on DAW templates, #37),
`needs_files` (other files this one needs beside it to work, as `/`-separated
paths below the same logical root, sorted and unique: the rest of its ReaPack
package, the files a script loads, a JSFX's imports, a theme's images; they
travel together, #61).

Reaper templates are `daw_asset` items under the `REAPER_RESOURCES` root, one
product each (`reaper-template/<path slug>`, vendor `REAPER`), portable and
content-hashed (a key map with paths into the resource folder written as
`<REAPER_RESOURCES>/...`, so the same shortcuts hash the same on every
machine). `Effects/` and `Data/` files are items (`reaper-effect/`,
`reaper-data/`) only when a ReaPack package installed them or a tracked file
needs them. Their `requires` lists the plug-ins they use, keyed the same
way the scanner keys products (`<vendor>/<name>`), so a template can require a
product the machine doesn't have.

ReaPack repositories are `config` items (`reapack-repo/<name slug>`, vendor
`ReaPack`) whose path is the repository's index URL as an `absolute` path.
Installed packages are `daw_asset` items (`reapack/<repo>/<category>/<package>`
slugs) at `REAPACK/<repo>/<category>/<package>` with the package version;
only native extensions are per-OS. Reaper folder settings are `config` items
(`reaper-folder/<ini key>`, or `reaper-folder/explorer/<label slug>` for Media
Explorer shortcuts) whose path is the folder, portable through the `HOME` and
`REAPER_RESOURCES` roots where possible (decision #52).

## Paths

Paths are portable so Windows and macOS manifests compare directly:

```json
{ "logical": { "root": "VST3_SYSTEM", "rel": "Acme/Acme Verb.vst3" } }
{ "absolute": "E:\\Tools\\Acme Hub\\AcmeHub.exe" }
```

`rel` is always `/`-separated. Each machine's `path_map` maps root names to
folders; the most specific root wins, matching is case-insensitive, and
resolving refuses `..` or absolute components so a synced manifest can't point
outside its root. `absolute` is a fallback for paths under no root and only
means something on the machine that wrote it.

Well-known roots are defined by the scanner (#5); user roots such as
`SAMPLES_ROOT` come from the user's path map.

## Exchange bundles

`patchpilot export` wraps manifests in a bundle (`patchpilot_core::Bundle`,
`bundle_version` 1): `machines` (each a manifest plus optional `profile_id`),
`golden` (versions oldest first, each with `version`, `created_at`, optional
`source_machine_id` and `note`, and a manifest) and `profiles` (user target
profiles), plus optional `ignored` (Primary Rig suggestions the user ignored:
`product_id`, optional `version`, `ignored_at`; added in M9 without a bump,
older builds ignore it). The bundle version bumps by the same rules as the manifest's, and
a reader refuses a newer bundle or any newer manifest inside it. See decision #28.

## Changing the format

Additive optional fields don't need a version bump (`requires` was added this
way in #37; older builds ignore it). Anything an older build
could misread (renames, new required fields, changed meaning) bumps
`MANIFEST_SCHEMA_VERSION` and gets a note here. Both founders review changes.
