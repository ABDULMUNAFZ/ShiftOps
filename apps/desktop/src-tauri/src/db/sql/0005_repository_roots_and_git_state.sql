-- Approved discovery roots and the Git working-tree facts collected for
-- each local repository. Removing a root only removes ShiftOps records;
-- repositories stay registered (root_id becomes NULL) and files on disk are
-- never touched.

CREATE TABLE repository_roots (
    id TEXT PRIMARY KEY NOT NULL,
    canonical_path TEXT NOT NULL UNIQUE,
    display_name TEXT NOT NULL CHECK (length(display_name) BETWEEN 1 AND 255),
    root_kind TEXT NOT NULL CHECK (root_kind IN ('repository', 'directory')),
    max_depth INTEGER NOT NULL CHECK (max_depth BETWEEN 0 AND 12),
    follow_internal_symlinks INTEGER NOT NULL DEFAULT 0 CHECK (follow_internal_symlinks IN (0, 1)),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    last_discovered_at TEXT
);

ALTER TABLE repositories ADD COLUMN root_id TEXT REFERENCES repository_roots (id) ON DELETE SET NULL;
ALTER TABLE repositories ADD COLUMN git_kind TEXT
    CHECK (git_kind IS NULL OR git_kind IN ('standard', 'worktree', 'submodule', 'git_file'));
ALTER TABLE repositories ADD COLUMN is_detached INTEGER NOT NULL DEFAULT 0 CHECK (is_detached IN (0, 1));
ALTER TABLE repositories ADD COLUMN is_dirty INTEGER CHECK (is_dirty IS NULL OR is_dirty IN (0, 1));
ALTER TABLE repositories ADD COLUMN staged_count INTEGER CHECK (staged_count IS NULL OR staged_count >= 0);
ALTER TABLE repositories ADD COLUMN modified_count INTEGER CHECK (modified_count IS NULL OR modified_count >= 0);
ALTER TABLE repositories ADD COLUMN untracked_count INTEGER CHECK (untracked_count IS NULL OR untracked_count >= 0);
ALTER TABLE repositories ADD COLUMN last_commit_at TEXT;
ALTER TABLE repositories ADD COLUMN git_collected_at TEXT;
ALTER TABLE repositories ADD COLUMN git_warnings_json TEXT NOT NULL DEFAULT '[]';

CREATE INDEX repositories_root_idx ON repositories (root_id);
