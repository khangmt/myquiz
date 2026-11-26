-- A host-given name for each game (e.g. "SE101.A, Tuesday"), so games of the
-- same quiz with different classes are easy to tell apart in Reports.
-- Nullable and additive, so an older binary still works after a rollback.
ALTER TABLE game_sessions ADD COLUMN name text
    CHECK (name IS NULL OR char_length(name) BETWEEN 1 AND 80);
ALTER TABLE users ALTER COLUMN locale SET DEFAULT 'en';  -- English is the default UI language
