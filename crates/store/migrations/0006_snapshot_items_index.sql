-- Schema v6: look up an item in a snapshot by its ID. Counting what each scan
-- added and removed (the History list, "last scanned" on every screen) checked
-- every item against every item of the scan before it, which took minutes on
-- a big rig with a long history and left the window blank meanwhile.

CREATE INDEX snapshot_items_item ON snapshot_items (snapshot_id, item_id);
