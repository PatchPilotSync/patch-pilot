-- Schema v4: restore points for writes to user files (rollback).
--
-- Before Patch Pilot changes a user file it saves the file's current bytes
-- to the blob store (app data, keyed by BLAKE3) and records a restore point
-- here, so the change can be undone exactly. Rows are never updated; an
-- undo is a new restore point that points at the one it undid.

CREATE TABLE restore_points (
    id          INTEGER PRIMARY KEY,
    machine_id  TEXT NOT NULL,
    created_at  INTEGER NOT NULL,
    kind        TEXT NOT NULL CHECK (kind IN ('change', 'undo')),
    label       TEXT NOT NULL,
    -- 'reaper' / 'ableton': the app whose files these are.
    daw         TEXT,
    -- The machine's latest scan when the change was made.
    snapshot_id INTEGER REFERENCES snapshots (id) ON DELETE SET NULL,
    undoes      INTEGER REFERENCES restore_points (id)
);
CREATE INDEX restore_points_machine ON restore_points (machine_id, id);

CREATE TABLE restore_point_files (
    restore_point_id INTEGER NOT NULL REFERENCES restore_points (id) ON DELETE CASCADE,
    position         INTEGER NOT NULL,
    path             TEXT NOT NULL,
    -- Blob of the file before the change; NULL if it didn't exist.
    before_hash      TEXT,
    -- BLAKE3 of what was written; NULL if the change removed the file.
    after_hash       TEXT,
    PRIMARY KEY (restore_point_id, position)
);
