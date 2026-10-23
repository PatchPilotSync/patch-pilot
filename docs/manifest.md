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
`platform_portable`, `confidence` (`low`, `medium`, `high`).

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

## Changing the format

Additive optional fields don't need a version bump. Anything an older build
could misread (renames, new required fields, changed meaning) bumps
`MANIFEST_SCHEMA_VERSION` and gets a note here. Both founders review changes.
