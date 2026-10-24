//! Non-secret credential metadata. Secret values live only in the OS keychain
//! (`crate::credentials`); this table records when a credential was stored and
//! its last validation result.

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use super::string_enum;
use crate::error::AppResult;
use crate::time_util;

string_enum!(Provider { Github => "github", Devin => "devin", Other => "other" });
string_enum!(
    #[allow(clippy::enum_variant_names)]
    CredentialKind {
    GithubOauthToken => "github_oauth_token",
    GithubPersonalAccessToken => "github_personal_access_token",
    DevinApiToken => "devin_api_token",
    ProviderToken => "provider_token",
});
string_enum!(ValidationStatus {
    NotValidated => "not_validated",
    Valid => "valid",
    Invalid => "invalid",
    Error => "error",
});

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialRecord {
    pub provider: Provider,
    pub kind: CredentialKind,
    pub account: String,
    pub created_at: String,
    pub updated_at: String,
    pub last_validated_at: Option<String>,
    pub last_validation_status: ValidationStatus,
}

fn map(r: &rusqlite::Row<'_>) -> rusqlite::Result<CredentialRecord> {
    Ok(CredentialRecord {
        provider: r.get("provider")?,
        kind: r.get("credential_kind")?,
        account: r.get("account")?,
        created_at: r.get("created_at")?,
        updated_at: r.get("updated_at")?,
        last_validated_at: r.get("last_validated_at")?,
        last_validation_status: r.get("last_validation_status")?,
    })
}

const COLUMNS: &str = "SELECT provider, credential_kind, account, created_at, updated_at, last_validated_at, last_validation_status";

pub fn get(
    conn: &Connection,
    provider: Provider,
    kind: CredentialKind,
) -> AppResult<Option<CredentialRecord>> {
    Ok(conn
        .query_row(
            &format!(
                "{COLUMNS} FROM credential_metadata WHERE provider = ?1 AND credential_kind = ?2"
            ),
            params![provider, kind],
            map,
        )
        .optional()?)
}

/// Record a store or replace. Returns true when a record already existed.
/// A replaced secret has not been validated yet, so status resets.
pub fn upsert(
    conn: &Connection,
    provider: Provider,
    kind: CredentialKind,
    account: &str,
) -> AppResult<bool> {
    let existed = get(conn, provider, kind)?.is_some();
    conn.execute(
        "INSERT INTO credential_metadata (provider, credential_kind, account, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?4)
         ON CONFLICT (provider, credential_kind) DO UPDATE SET account = excluded.account,
             updated_at = excluded.updated_at, last_validated_at = NULL, last_validation_status = 'not_validated'",
        params![provider, kind, account, time_util::now()],
    )?;
    Ok(existed)
}

pub fn remove(conn: &Connection, provider: Provider, kind: CredentialKind) -> AppResult<bool> {
    Ok(conn.execute(
        "DELETE FROM credential_metadata WHERE provider = ?1 AND credential_kind = ?2",
        params![provider, kind],
    )? > 0)
}
