-- Schema v3: stored target profiles (#36, decisions 3 and 18).
--
-- Target profiles live in `profiles` (kind 'target') with ordered rules in
-- `profile_rules`; `machines.target_profile_id` assigns one per machine.
-- Built-in presets can't be edited or replaced by an import.

ALTER TABLE profiles ADD COLUMN builtin INTEGER NOT NULL DEFAULT 0;

INSERT INTO profiles (id, name, kind, golden_id, builtin) VALUES
    ('studio-clone', 'Studio clone', 'target', 'golden', 1),
    ('travel', 'Travel', 'target', 'golden', 1);

INSERT INTO profile_rules (profile_id, position, action, matcher) VALUES
    ('travel', 0, 'exclude', '{"format":"aax"}'),
    ('travel', 1, 'exclude', '{"size_over":20000000000}');
