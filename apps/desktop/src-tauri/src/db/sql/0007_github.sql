-- GitHub accounts, organisations, repositories and pull requests. Tokens
-- are never stored here: an account references its OS keychain credential
-- kind only.

CREATE TABLE github_accounts (
    id TEXT PRIMARY KEY NOT NULL,
    host TEXT NOT NULL,
    api_base_url TEXT NOT NULL,
    github_user_id INTEGER NOT NULL,
    login TEXT NOT NULL,
    name TEXT,
    html_url TEXT,
    auth_method TEXT NOT NULL CHECK (auth_method IN ('oauth_device', 'personal_access_token')),
    credential_kind TEXT NOT NULL CHECK (credential_kind IN ('github_oauth_token', 'github_personal_access_token')),
    scopes_json TEXT NOT NULL DEFAULT '[]',
    scopes_reported INTEGER NOT NULL DEFAULT 0 CHECK (scopes_reported IN (0, 1)),
    status TEXT NOT NULL DEFAULT 'connected' CHECK (status IN ('connected', 'invalid', 'disconnected')),
    validated_at TEXT,
    rate_limit_json TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE (host, github_user_id)
);
CREATE INDEX github_accounts_status_idx ON github_accounts (status, updated_at DESC);

CREATE TABLE github_organizations (
    id TEXT PRIMARY KEY NOT NULL,
    account_id TEXT NOT NULL REFERENCES github_accounts (id) ON DELETE CASCADE,
    github_id INTEGER NOT NULL,
    login TEXT NOT NULL,
    description TEXT,
    synced_at TEXT NOT NULL,
    UNIQUE (account_id, github_id)
);

CREATE TABLE github_repositories (
    id TEXT PRIMARY KEY NOT NULL,
    account_id TEXT NOT NULL REFERENCES github_accounts (id) ON DELETE CASCADE,
    github_id INTEGER NOT NULL,
    owner TEXT NOT NULL COLLATE NOCASE,
    name TEXT NOT NULL COLLATE NOCASE,
    full_name TEXT NOT NULL,
    private INTEGER NOT NULL CHECK (private IN (0, 1)),
    archived INTEGER NOT NULL CHECK (archived IN (0, 1)),
    fork INTEGER NOT NULL CHECK (fork IN (0, 1)),
    default_branch TEXT,
    html_url TEXT NOT NULL,
    clone_url TEXT,
    ssh_url TEXT,
    description TEXT,
    primary_language TEXT,
    languages_json TEXT,
    pushed_at TEXT,
    remote_updated_at TEXT,
    permissions_json TEXT,
    -- The ShiftOps repository this GitHub repository is imported as, if any.
    repository_id TEXT REFERENCES repositories (id) ON DELETE SET NULL,
    synced_at TEXT NOT NULL,
    UNIQUE (account_id, github_id)
);
CREATE INDEX github_repositories_owner_idx ON github_repositories (account_id, owner, name);
CREATE INDEX github_repositories_repository_idx ON github_repositories (repository_id);

CREATE TABLE github_pull_requests (
    id TEXT PRIMARY KEY NOT NULL,
    github_repository_id TEXT NOT NULL REFERENCES github_repositories (id) ON DELETE CASCADE,
    github_id INTEGER NOT NULL,
    number INTEGER NOT NULL CHECK (number > 0),
    title TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('open', 'closed', 'merged')),
    draft INTEGER NOT NULL CHECK (draft IN (0, 1)),
    author_login TEXT,
    html_url TEXT NOT NULL,
    head_ref TEXT NOT NULL,
    head_sha TEXT NOT NULL,
    head_repo_full_name TEXT,
    base_ref TEXT NOT NULL,
    base_sha TEXT,
    mergeable INTEGER CHECK (mergeable IS NULL OR mergeable IN (0, 1)),
    mergeable_state TEXT,
    checks_json TEXT,
    remote_created_at TEXT,
    remote_updated_at TEXT,
    synced_at TEXT NOT NULL,
    UNIQUE (github_repository_id, number)
);
CREATE INDEX github_pull_requests_state_idx ON github_pull_requests (github_repository_id, state, remote_updated_at DESC);
