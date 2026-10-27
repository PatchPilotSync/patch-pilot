-- Schema v7: a machine's items of one kind. History compares only the plug-ins
-- of every scan, and the Installer Vault lists only what can be installed;
-- reading them through each scan's full item list (mostly presets and files)
-- took seconds on a big rig (2026-10-27).

CREATE INDEX items_machine_kind ON items (machine_id, kind);
