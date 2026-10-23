-- Schema v2: Golden Profile versions (#35, decision 2).
--
-- Each version is a snapshot of its own pseudo-machine `golden@<n>`, so a
-- version's manifest, machine header included, never changes after it is
-- written. Versions are never updated or deleted.

INSERT INTO profiles (id, name, kind) VALUES ('golden', 'Golden Rig', 'golden');

CREATE TABLE golden_versions (
    golden_id         TEXT NOT NULL REFERENCES profiles (id),
    version           INTEGER NOT NULL CHECK (version >= 1),
    parent_version    INTEGER,
    snapshot_id       INTEGER NOT NULL REFERENCES snapshots (id),
    created_at        INTEGER NOT NULL,
    source_machine_id TEXT,
    note              TEXT,
    PRIMARY KEY (golden_id, version),
    FOREIGN KEY (golden_id, parent_version) REFERENCES golden_versions (golden_id, version)
);
