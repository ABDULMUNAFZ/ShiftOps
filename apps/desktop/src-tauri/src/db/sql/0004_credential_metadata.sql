-- Non-secret facts about credentials held in the OS keychain. Secret values
-- are never stored in SQLite.
CREATE TABLE credential_metadata (
    provider TEXT NOT NULL CHECK (provider IN ('github', 'devin', 'other')),
    credential_kind TEXT NOT NULL CHECK (credential_kind IN (
        'github_oauth_token', 'github_personal_access_token', 'devin_api_token', 'provider_token')),
    -- Keychain account name; the keychain service is the application's.
    account TEXT NOT NULL UNIQUE,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    last_validated_at TEXT,
    last_validation_status TEXT NOT NULL DEFAULT 'not_validated'
        CHECK (last_validation_status IN ('not_validated', 'valid', 'invalid', 'error')),
    PRIMARY KEY (provider, credential_kind)
);
