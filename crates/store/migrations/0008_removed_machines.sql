-- Schema v8: machines the user removed from this one's list (decision #100).
--
-- A removed machine's scans can come back from a linked machine, an export
-- file or its own object on the relay, so its ID is kept with the newest scan
-- time it had here and when it was removed. Only a scan newer than both
-- brings it back, as when it is still in use and scans again.

CREATE TABLE removed_machines (
    id                TEXT PRIMARY KEY,
    removed_at        INTEGER NOT NULL,
    last_generated_at INTEGER
);
