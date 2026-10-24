-- Repository import modes: approved managed-workspace roots, managed clone
-- attempts, and who owns a repository's files.
--
-- `ownership` is 'external' for checkouts the user already had (discovered or
-- linked; ShiftOps never modifies or deletes them) and 'shiftops_managed'
-- for clones ShiftOps made inside an approved workspace. Metadata-only
-- records have no files and no ownership.
--
-- `github_link_method` records how a record got its GitHub identity, so an
-- explicit mismatch override survives later rediscovery.

CREATE TABLE managed_workspaces (
    id TEXT PRIMARY KEY NOT NULL,
    canonical_path TEXT NOT NULL UNIQUE,
    approved_at TEXT NOT NULL,
    -- Increases with every approval; the highest is the active workspace.
    approval_seq INTEGER NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE repository_clones (
    id TEXT PRIMARY KEY NOT NULL,
    repository_id TEXT REFERENCES repositories (id) ON DELETE SET NULL,
    github_repository_id TEXT REFERENCES github_repositories (id) ON DELETE SET NULL,
    workspace_id TEXT NOT NULL REFERENCES managed_workspaces (id),
    full_name TEXT NOT NULL,
    clone_url TEXT NOT NULL,
    -- The directory ShiftOps created for this clone. Cleanup may delete only this path.
    destination_path TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('running', 'succeeded', 'failed', 'cancelled', 'cleaned_up')),
    run_id TEXT,
    error_code TEXT,
    error_message TEXT,
    partial_path_exists INTEGER NOT NULL DEFAULT 0 CHECK (partial_path_exists IN (0, 1)),
    started_at TEXT NOT NULL,
    completed_at TEXT,
    cleaned_at TEXT
);
-- One clone attempt holds a destination until it succeeds or is cleaned up.
CREATE UNIQUE INDEX repository_clones_open_destination_idx
    ON repository_clones (destination_path) WHERE status IN ('running', 'failed', 'cancelled');
CREATE INDEX repository_clones_started_idx ON repository_clones (started_at DESC);

ALTER TABLE repositories ADD COLUMN ownership TEXT
    CHECK (ownership IS NULL OR ownership IN ('external', 'shiftops_managed'));
ALTER TABLE repositories ADD COLUMN github_link_method TEXT
    CHECK (github_link_method IS NULL OR github_link_method IN
        ('metadata_only', 'discovery', 'remote_match', 'override', 'managed_clone'));

-- Every checkout registered before this version was found by discovery.
UPDATE repositories SET ownership = 'external' WHERE local_path IS NOT NULL;
UPDATE repositories SET github_link_method = CASE WHEN local_path IS NULL THEN 'metadata_only' ELSE 'discovery' END
    WHERE github_owner IS NOT NULL;
