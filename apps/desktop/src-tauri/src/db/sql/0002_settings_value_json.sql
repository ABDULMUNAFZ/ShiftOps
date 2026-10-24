-- Settings: rename `value` to `value_json` and add `created_at`.
-- SQLite cannot add a NOT NULL column without a constant default, so the
-- table is rebuilt. Rows from version 1 have no creation time; their
-- `updated_at` is the best available value and is used for both.
CREATE TABLE settings_v2 (
    key TEXT PRIMARY KEY NOT NULL,
    value_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

INSERT INTO settings_v2 (key, value_json, created_at, updated_at)
SELECT key, value, updated_at, updated_at FROM settings;

DROP TABLE settings;

ALTER TABLE settings_v2 RENAME TO settings;
