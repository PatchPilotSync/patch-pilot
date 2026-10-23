-- Patch Pilot local index, schema v1. See docs/strategy.md section 9.
-- Lives in the OS app-data directory, never in user folders (CLAUDE.md rule 3).

CREATE TABLE machines (
    id                TEXT PRIMARY KEY,
    name              TEXT NOT NULL,
    os                TEXT NOT NULL CHECK (os IN ('windows', 'macos')),
    arch              TEXT NOT NULL,
    os_version        TEXT,
    target_profile_id TEXT REFERENCES profiles (id) ON DELETE SET NULL,
    last_seen         INTEGER
);

-- Logical root -> physical folder, per machine.
CREATE TABLE path_maps (
    machine_id    TEXT NOT NULL REFERENCES machines (id) ON DELETE CASCADE,
    logical_name  TEXT NOT NULL,
    physical_path TEXT NOT NULL,
    PRIMARY KEY (machine_id, logical_name)
);

-- Catalog cache.
CREATE TABLE products (
    canonical_id TEXT PRIMARY KEY,
    vendor       TEXT NOT NULL,
    name         TEXT NOT NULL
);

-- Immutable: a row is never updated. A changed item is a new row with a new
-- item_hash; snapshots point at the rows that were current when taken.
CREATE TABLE items (
    id                INTEGER PRIMARY KEY,
    machine_id        TEXT NOT NULL REFERENCES machines (id) ON DELETE CASCADE,
    item_hash         TEXT NOT NULL,
    kind              TEXT NOT NULL,
    product_id        TEXT REFERENCES products (canonical_id),
    format            TEXT,
    platform          TEXT NOT NULL,
    arch              TEXT,
    version           TEXT,
    path_root         TEXT,
    path_rel          TEXT,
    path_absolute     TEXT,
    content_hash      TEXT,
    size              INTEGER,
    mtime             INTEGER,
    platform_portable INTEGER NOT NULL DEFAULT 0,
    confidence        TEXT NOT NULL,
    UNIQUE (machine_id, item_hash),
    CHECK ((path_root IS NULL) = (path_rel IS NULL)),
    CHECK ((path_root IS NULL) <> (path_absolute IS NULL))
);
CREATE INDEX items_product ON items (product_id);

CREATE TABLE snapshots (
    id            INTEGER PRIMARY KEY,
    machine_id    TEXT NOT NULL REFERENCES machines (id) ON DELETE CASCADE,
    created_at    INTEGER NOT NULL,
    reason        TEXT NOT NULL,
    parent_id     INTEGER REFERENCES snapshots (id),
    manifest_hash TEXT NOT NULL,
    generated_at  INTEGER NOT NULL
);
CREATE INDEX snapshots_machine ON snapshots (machine_id, id);

CREATE TABLE snapshot_items (
    snapshot_id INTEGER NOT NULL REFERENCES snapshots (id) ON DELETE CASCADE,
    position    INTEGER NOT NULL,
    item_id     INTEGER NOT NULL REFERENCES items (id),
    hash        TEXT NOT NULL,
    PRIMARY KEY (snapshot_id, position)
);

-- Golden Profiles and target profiles.
CREATE TABLE profiles (
    id             TEXT PRIMARY KEY,
    name           TEXT NOT NULL,
    kind           TEXT NOT NULL CHECK (kind IN ('golden', 'target')),
    golden_id      TEXT REFERENCES profiles (id),
    golden_version INTEGER,
    snapshot_id    INTEGER REFERENCES snapshots (id)
);

CREATE TABLE profile_rules (
    profile_id TEXT NOT NULL REFERENCES profiles (id) ON DELETE CASCADE,
    position   INTEGER NOT NULL,
    action     TEXT NOT NULL CHECK (action IN ('include', 'exclude')),
    -- JSON of patchpilot_core::Matcher, e.g. {"format":"aax"}
    matcher    TEXT NOT NULL,
    PRIMARY KEY (profile_id, position)
);

-- Installers on user-owned storage. We never host installers (decision 8).
CREATE TABLE vault_entries (
    hash         TEXT PRIMARY KEY,
    vendor       TEXT,
    product_id   TEXT REFERENCES products (canonical_id),
    version      TEXT,
    os           TEXT,
    arch         TEXT,
    filename     TEXT NOT NULL,
    source       TEXT,
    verified_sig INTEGER NOT NULL DEFAULT 0
);

-- Template / rack / project -> product it needs.
CREATE TABLE dependencies (
    asset_item_id INTEGER NOT NULL REFERENCES items (id) ON DELETE CASCADE,
    product_id    TEXT NOT NULL,
    PRIMARY KEY (asset_item_id, product_id)
);

-- Drift log.
CREATE TABLE events (
    id         INTEGER PRIMARY KEY,
    machine_id TEXT REFERENCES machines (id) ON DELETE CASCADE,
    at         INTEGER NOT NULL,
    kind       TEXT NOT NULL,
    payload    TEXT
);

-- Diffs the user chose to ignore, so warnings don't repeat.
CREATE TABLE decisions (
    id         INTEGER PRIMARY KEY,
    machine_id TEXT REFERENCES machines (id) ON DELETE CASCADE,
    product_id TEXT NOT NULL,
    state      TEXT NOT NULL,
    decision   TEXT NOT NULL CHECK (decision IN ('ignore', 'accept')),
    decided_at INTEGER NOT NULL,
    note       TEXT
);
