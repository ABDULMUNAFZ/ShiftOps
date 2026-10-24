//! Persistence for GitHub accounts, organisations, repositories and pull
//! requests. Only metadata: tokens live in the OS keychain.

#![cfg_attr(not(test), allow(dead_code))]

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use serde_json::Value;

use super::credentials::CredentialKind;
use super::repositories::{self, Repository};
use super::{json_col, json_col_opt, new_id, string_enum};
use crate::error::{AppError, AppResult};
use crate::time_util;

string_enum!(AuthMethod { OauthDevice => "oauth_device", PersonalAccessToken => "personal_access_token" });
string_enum!(AccountStatus { Connected => "connected", Invalid => "invalid", Disconnected => "disconnected" });
string_enum!(PullState { Open => "open", Closed => "closed", Merged => "merged" });

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubAccount {
    pub id: String,
    pub host: String,
    pub api_base_url: String,
    pub github_user_id: i64,
    pub login: String,
    pub name: Option<String>,
    pub html_url: Option<String>,
    pub auth_method: AuthMethod,
    pub credential_kind: CredentialKind,
    pub scopes: Value,
    /// Classic and OAuth tokens report scopes; fine-grained tokens do not.
    pub scopes_reported: bool,
    pub status: AccountStatus,
    pub validated_at: Option<String>,
    pub rate_limit: Option<Value>,
    pub created_at: String,
    pub updated_at: String,
}

const ACCOUNT_COLUMNS: &str = "SELECT id, host, api_base_url, github_user_id, login, name, html_url, auth_method, \
    credential_kind, scopes_json, scopes_reported, status, validated_at, rate_limit_json, created_at, updated_at";

fn map_account(r: &rusqlite::Row<'_>) -> rusqlite::Result<GithubAccount> {
    Ok(GithubAccount {
        id: r.get("id")?,
        host: r.get("host")?,
        api_base_url: r.get("api_base_url")?,
        github_user_id: r.get("github_user_id")?,
        login: r.get("login")?,
        name: r.get("name")?,
        html_url: r.get("html_url")?,
        auth_method: r.get("auth_method")?,
        credential_kind: r.get("credential_kind")?,
        scopes: json_col(r, "scopes_json")?,
        scopes_reported: r.get("scopes_reported")?,
        status: r.get("status")?,
        validated_at: r.get("validated_at")?,
        rate_limit: json_col_opt(r, "rate_limit_json")?,
        created_at: r.get("created_at")?,
        updated_at: r.get("updated_at")?,
    })
}

#[derive(Debug, Clone)]
pub struct AccountUpsert {
    pub host: String,
    pub api_base_url: String,
    pub github_user_id: i64,
    pub login: String,
    pub name: Option<String>,
    pub html_url: Option<String>,
    pub auth_method: AuthMethod,
    pub credential_kind: CredentialKind,
    pub scopes: Option<Vec<String>>,
    pub rate_limit: Option<Value>,
}

/// Record a validated connection. ShiftOps keeps one connected account:
/// any other connected account is marked disconnected.
pub fn connect(conn: &Connection, a: &AccountUpsert) -> AppResult<GithubAccount> {
    let now = time_util::now();
    let scopes = serde_json::to_string(&a.scopes.clone().unwrap_or_default())?;
    let rate = a
        .rate_limit
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?;
    conn.execute(
        "UPDATE github_accounts SET status = 'disconnected', updated_at = ?3
         WHERE status <> 'disconnected' AND NOT (host = ?1 AND github_user_id = ?2)",
        params![a.host, a.github_user_id, now],
    )?;
    conn.execute(
        "INSERT INTO github_accounts (id, host, api_base_url, github_user_id, login, name, html_url, auth_method,
             credential_kind, scopes_json, scopes_reported, status, validated_at, rate_limit_json, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 'connected', ?12, ?13, ?12, ?12)
         ON CONFLICT (host, github_user_id) DO UPDATE SET api_base_url = excluded.api_base_url,
             login = excluded.login, name = excluded.name, html_url = excluded.html_url,
             auth_method = excluded.auth_method, credential_kind = excluded.credential_kind,
             scopes_json = excluded.scopes_json, scopes_reported = excluded.scopes_reported, status = 'connected',
             validated_at = excluded.validated_at, rate_limit_json = excluded.rate_limit_json,
             updated_at = excluded.updated_at",
        params![
            new_id(),
            a.host,
            a.api_base_url,
            a.github_user_id,
            a.login,
            a.name,
            a.html_url,
            a.auth_method,
            a.credential_kind,
            scopes,
            a.scopes.is_some(),
            now,
            rate
        ],
    )?;
    Ok(conn.query_row(
        &format!("{ACCOUNT_COLUMNS} FROM github_accounts WHERE host = ?1 AND github_user_id = ?2"),
        params![a.host, a.github_user_id],
        map_account,
    )?)
}

pub fn active(conn: &Connection) -> AppResult<Option<GithubAccount>> {
    Ok(conn
        .query_row(
            &format!("{ACCOUNT_COLUMNS} FROM github_accounts WHERE status <> 'disconnected' ORDER BY updated_at DESC LIMIT 1"),
            [],
            map_account,
        )
        .optional()?)
}

pub fn get_account(conn: &Connection, id: &str) -> AppResult<GithubAccount> {
    conn.query_row(
        &format!("{ACCOUNT_COLUMNS} FROM github_accounts WHERE id = ?1"),
        [id],
        map_account,
    )
    .optional()?
    .ok_or_else(|| AppError::not_found("GitHub account", id))
}

pub fn set_status(conn: &Connection, id: &str, status: AccountStatus) -> AppResult<()> {
    conn.execute(
        "UPDATE github_accounts SET status = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, status, time_util::now()],
    )?;
    Ok(())
}

pub fn record_validation(
    conn: &Connection,
    id: &str,
    scopes: Option<&[String]>,
    rate_limit: Option<&Value>,
) -> AppResult<()> {
    conn.execute(
        "UPDATE github_accounts SET scopes_json = COALESCE(?2, scopes_json), scopes_reported = CASE WHEN ?2 IS NULL THEN scopes_reported ELSE 1 END,
             rate_limit_json = COALESCE(?3, rate_limit_json), validated_at = ?4, status = 'connected', updated_at = ?4
         WHERE id = ?1",
        params![
            id,
            scopes.map(serde_json::to_string).transpose()?,
            rate_limit.map(serde_json::to_string).transpose()?,
            time_util::now()
        ],
    )?;
    Ok(())
}

pub fn record_rate_limit(conn: &Connection, id: &str, rate_limit: &Value) -> AppResult<()> {
    conn.execute(
        "UPDATE github_accounts SET rate_limit_json = ?2 WHERE id = ?1",
        params![id, serde_json::to_string(rate_limit)?],
    )?;
    Ok(())
}

// ---------------------------------------------------------------- organisations

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubOrganization {
    pub id: String,
    pub github_id: i64,
    pub login: String,
    pub description: Option<String>,
    pub synced_at: String,
}

/// Replace the account's organisation list with what GitHub returned.
pub fn replace_organizations(
    conn: &Connection,
    account_id: &str,
    orgs: &[(i64, String, Option<String>)],
) -> AppResult<()> {
    let now = time_util::now();
    conn.execute(
        "DELETE FROM github_organizations WHERE account_id = ?1",
        [account_id],
    )?;
    for (github_id, login, description) in orgs {
        conn.execute(
            "INSERT INTO github_organizations (id, account_id, github_id, login, description, synced_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![new_id(), account_id, github_id, login, description, now],
        )?;
    }
    Ok(())
}

pub fn organizations(conn: &Connection, account_id: &str) -> AppResult<Vec<GithubOrganization>> {
    let mut stmt = conn.prepare(
        "SELECT id, github_id, login, description, synced_at FROM github_organizations WHERE account_id = ?1
         ORDER BY login COLLATE NOCASE",
    )?;
    let rows = stmt.query_map([account_id], |r| {
        Ok(GithubOrganization {
            id: r.get(0)?,
            github_id: r.get(1)?,
            login: r.get(2)?,
            description: r.get(3)?,
            synced_at: r.get(4)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

// ---------------------------------------------------------------- repositories

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubRepository {
    pub id: String,
    pub account_id: String,
    pub github_id: i64,
    pub owner: String,
    pub name: String,
    pub full_name: String,
    pub private: bool,
    pub archived: bool,
    pub fork: bool,
    pub default_branch: Option<String>,
    pub html_url: String,
    pub clone_url: Option<String>,
    pub ssh_url: Option<String>,
    pub description: Option<String>,
    pub primary_language: Option<String>,
    pub languages: Option<Value>,
    pub pushed_at: Option<String>,
    pub remote_updated_at: Option<String>,
    pub permissions: Option<Value>,
    pub repository_id: Option<String>,
    pub synced_at: String,
}

#[derive(Debug, Clone, Default)]
pub struct RepositoryUpsert {
    pub github_id: i64,
    pub owner: String,
    pub name: String,
    pub full_name: String,
    pub private: bool,
    pub archived: bool,
    pub fork: bool,
    pub default_branch: Option<String>,
    pub html_url: String,
    pub clone_url: Option<String>,
    pub ssh_url: Option<String>,
    pub description: Option<String>,
    pub primary_language: Option<String>,
    pub pushed_at: Option<String>,
    pub remote_updated_at: Option<String>,
    pub permissions: Option<Value>,
}

const REPO_COLUMNS: &str = "SELECT id, account_id, github_id, owner, name, full_name, private, archived, fork, \
    default_branch, html_url, clone_url, ssh_url, description, primary_language, languages_json, pushed_at, \
    remote_updated_at, permissions_json, repository_id, synced_at";

fn map_repo(r: &rusqlite::Row<'_>) -> rusqlite::Result<GithubRepository> {
    Ok(GithubRepository {
        id: r.get("id")?,
        account_id: r.get("account_id")?,
        github_id: r.get("github_id")?,
        owner: r.get("owner")?,
        name: r.get("name")?,
        full_name: r.get("full_name")?,
        private: r.get("private")?,
        archived: r.get("archived")?,
        fork: r.get("fork")?,
        default_branch: r.get("default_branch")?,
        html_url: r.get("html_url")?,
        clone_url: r.get("clone_url")?,
        ssh_url: r.get("ssh_url")?,
        description: r.get("description")?,
        primary_language: r.get("primary_language")?,
        languages: json_col_opt(r, "languages_json")?,
        pushed_at: r.get("pushed_at")?,
        remote_updated_at: r.get("remote_updated_at")?,
        permissions: json_col_opt(r, "permissions_json")?,
        repository_id: r.get("repository_id")?,
        synced_at: r.get("synced_at")?,
    })
}

/// Upsert by GitHub's stable repository id. Renames and transfers update owner and name in place.
pub fn upsert_repository(
    conn: &Connection,
    account_id: &str,
    r: &RepositoryUpsert,
) -> AppResult<GithubRepository> {
    conn.execute(
        "INSERT INTO github_repositories (id, account_id, github_id, owner, name, full_name, private, archived, fork,
             default_branch, html_url, clone_url, ssh_url, description, primary_language, pushed_at, remote_updated_at,
             permissions_json, synced_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)
         ON CONFLICT (account_id, github_id) DO UPDATE SET owner = excluded.owner, name = excluded.name,
             full_name = excluded.full_name, private = excluded.private, archived = excluded.archived,
             fork = excluded.fork, default_branch = excluded.default_branch, html_url = excluded.html_url,
             clone_url = excluded.clone_url, ssh_url = excluded.ssh_url, description = excluded.description,
             primary_language = excluded.primary_language, pushed_at = excluded.pushed_at,
             remote_updated_at = excluded.remote_updated_at, permissions_json = excluded.permissions_json,
             synced_at = excluded.synced_at",
        params![
            new_id(),
            account_id,
            r.github_id,
            r.owner,
            r.name,
            r.full_name,
            r.private,
            r.archived,
            r.fork,
            r.default_branch,
            r.html_url,
            r.clone_url,
            r.ssh_url,
            r.description,
            r.primary_language,
            r.pushed_at,
            r.remote_updated_at,
            r.permissions.as_ref().map(serde_json::to_string).transpose()?,
            time_util::now()
        ],
    )?;
    let repo = conn.query_row(
        &format!(
            "{REPO_COLUMNS} FROM github_repositories WHERE account_id = ?1 AND github_id = ?2"
        ),
        params![account_id, r.github_id],
        map_repo,
    )?;
    link_existing(conn, &repo)?;
    get_repository(conn, &repo.id)
}

/// If a ShiftOps repository already holds this GitHub identity (for example a
/// discovered local checkout), link to it.
fn link_existing(conn: &Connection, repo: &GithubRepository) -> AppResult<()> {
    if repo.repository_id.is_some() {
        return Ok(());
    }
    let host = account_host(conn, &repo.account_id)?;
    let holder: Option<String> = conn
        .query_row(
            "SELECT id FROM repositories WHERE github_host = ?1 AND github_owner = ?2 AND github_repo = ?3",
            params![host, repo.owner, repo.name],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(id) = holder {
        conn.execute(
            "UPDATE github_repositories SET repository_id = ?2 WHERE id = ?1",
            params![repo.id, id],
        )?;
    }
    Ok(())
}

fn account_host(conn: &Connection, account_id: &str) -> AppResult<String> {
    Ok(conn.query_row(
        "SELECT host FROM github_accounts WHERE id = ?1",
        [account_id],
        |r| r.get(0),
    )?)
}

pub fn set_languages(conn: &Connection, id: &str, languages: &Value) -> AppResult<()> {
    conn.execute(
        "UPDATE github_repositories SET languages_json = ?2, synced_at = ?3 WHERE id = ?1",
        params![id, serde_json::to_string(languages)?, time_util::now()],
    )?;
    Ok(())
}

pub fn get_repository(conn: &Connection, id: &str) -> AppResult<GithubRepository> {
    conn.query_row(
        &format!("{REPO_COLUMNS} FROM github_repositories WHERE id = ?1"),
        [id],
        map_repo,
    )
    .optional()?
    .ok_or_else(|| AppError::not_found("GitHub repository", id))
}

pub fn find_repository(
    conn: &Connection,
    account_id: &str,
    owner: &str,
    name: &str,
) -> AppResult<Option<GithubRepository>> {
    Ok(conn
        .query_row(
            &format!("{REPO_COLUMNS} FROM github_repositories WHERE account_id = ?1 AND owner = ?2 AND name = ?3"),
            params![account_id, owner, name],
            map_repo,
        )
        .optional()?)
}

/// Import a GitHub repository as metadata only: a ShiftOps repository with
/// GitHub coordinates and no local checkout (starts `untrusted`). When a
/// local checkout of the same repository is registered, it is linked instead.
/// A later discovery of a matching checkout attaches its path to this record
/// (see `repositories::record_local_discovery`).
pub fn import_metadata_only(
    conn: &Connection,
    github_repository_id: &str,
) -> AppResult<(Repository, bool)> {
    // A checkout discovered after this record was cached may already hold the identity.
    link_existing(conn, &get_repository(conn, github_repository_id)?)?;
    let gh = get_repository(conn, github_repository_id)?;
    if let Some(id) = &gh.repository_id {
        return Ok((repositories::get(conn, id)?, false));
    }
    let host = account_host(conn, &gh.account_id)?;
    let created = repositories::create(
        conn,
        &repositories::NewRepository {
            source_type: repositories::SourceType::Github,
            name: gh.name.clone(),
            local_path: None,
            remote_url: gh.clone_url.clone(),
            github: Some(repositories::GithubCoordinates {
                host,
                owner: gh.owner.clone(),
                repo: gh.name.clone(),
            }),
            default_branch: gh.default_branch.clone(),
        },
    )?;
    conn.execute(
        "UPDATE github_repositories SET repository_id = ?2 WHERE id = ?1",
        params![github_repository_id, created.id],
    )?;
    Ok((created, true))
}

// ---------------------------------------------------------------- pull requests

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubPullRequest {
    pub id: String,
    pub github_repository_id: String,
    pub github_id: i64,
    pub number: i64,
    pub title: String,
    pub state: PullState,
    pub draft: bool,
    pub author_login: Option<String>,
    pub html_url: String,
    pub head_ref: String,
    pub head_sha: String,
    pub head_repo_full_name: Option<String>,
    pub base_ref: String,
    pub base_sha: Option<String>,
    /// `None` while GitHub is still computing mergeability, or for list results.
    pub mergeable: Option<bool>,
    pub mergeable_state: Option<String>,
    pub checks: Option<Value>,
    pub remote_created_at: Option<String>,
    pub remote_updated_at: Option<String>,
    pub synced_at: String,
}

#[derive(Debug, Clone, Default)]
pub struct PullUpsert {
    pub github_id: i64,
    pub number: i64,
    pub title: String,
    pub state: Option<PullState>,
    pub draft: bool,
    pub author_login: Option<String>,
    pub html_url: String,
    pub head_ref: String,
    pub head_sha: String,
    pub head_repo_full_name: Option<String>,
    pub base_ref: String,
    pub base_sha: Option<String>,
    pub mergeable: Option<bool>,
    pub mergeable_state: Option<String>,
    pub checks: Option<Value>,
    pub remote_created_at: Option<String>,
    pub remote_updated_at: Option<String>,
}

const PR_COLUMNS: &str = "SELECT id, github_repository_id, github_id, number, title, state, draft, author_login, html_url, \
    head_ref, head_sha, head_repo_full_name, base_ref, base_sha, mergeable, mergeable_state, checks_json, \
    remote_created_at, remote_updated_at, synced_at";

fn map_pr(r: &rusqlite::Row<'_>) -> rusqlite::Result<GithubPullRequest> {
    Ok(GithubPullRequest {
        id: r.get("id")?,
        github_repository_id: r.get("github_repository_id")?,
        github_id: r.get("github_id")?,
        number: r.get("number")?,
        title: r.get("title")?,
        state: r.get("state")?,
        draft: r.get("draft")?,
        author_login: r.get("author_login")?,
        html_url: r.get("html_url")?,
        head_ref: r.get("head_ref")?,
        head_sha: r.get("head_sha")?,
        head_repo_full_name: r.get("head_repo_full_name")?,
        base_ref: r.get("base_ref")?,
        base_sha: r.get("base_sha")?,
        mergeable: r.get("mergeable")?,
        mergeable_state: r.get("mergeable_state")?,
        checks: json_col_opt(r, "checks_json")?,
        remote_created_at: r.get("remote_created_at")?,
        remote_updated_at: r.get("remote_updated_at")?,
        synced_at: r.get("synced_at")?,
    })
}

/// Upsert by repository and number. Mergeability and checks are kept when
/// the incoming record does not carry them (list results never do).
pub fn upsert_pull_request(
    conn: &Connection,
    github_repository_id: &str,
    p: &PullUpsert,
) -> AppResult<GithubPullRequest> {
    conn.execute(
        "INSERT INTO github_pull_requests (id, github_repository_id, github_id, number, title, state, draft, author_login,
             html_url, head_ref, head_sha, head_repo_full_name, base_ref, base_sha, mergeable, mergeable_state,
             checks_json, remote_created_at, remote_updated_at, synced_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)
         ON CONFLICT (github_repository_id, number) DO UPDATE SET github_id = excluded.github_id,
             title = excluded.title, state = excluded.state, draft = excluded.draft,
             author_login = excluded.author_login, html_url = excluded.html_url, head_ref = excluded.head_ref,
             head_sha = excluded.head_sha, head_repo_full_name = excluded.head_repo_full_name,
             base_ref = excluded.base_ref, base_sha = COALESCE(excluded.base_sha, github_pull_requests.base_sha),
             mergeable = CASE WHEN excluded.head_sha <> github_pull_requests.head_sha THEN excluded.mergeable
                              ELSE COALESCE(excluded.mergeable, github_pull_requests.mergeable) END,
             mergeable_state = COALESCE(excluded.mergeable_state, github_pull_requests.mergeable_state),
             checks_json = CASE WHEN excluded.head_sha <> github_pull_requests.head_sha THEN excluded.checks_json
                                ELSE COALESCE(excluded.checks_json, github_pull_requests.checks_json) END,
             remote_created_at = excluded.remote_created_at, remote_updated_at = excluded.remote_updated_at,
             synced_at = excluded.synced_at",
        params![
            new_id(),
            github_repository_id,
            p.github_id,
            p.number,
            p.title,
            p.state.unwrap_or(PullState::Open),
            p.draft,
            p.author_login,
            p.html_url,
            p.head_ref,
            p.head_sha,
            p.head_repo_full_name,
            p.base_ref,
            p.base_sha,
            p.mergeable,
            p.mergeable_state,
            p.checks.as_ref().map(serde_json::to_string).transpose()?,
            p.remote_created_at,
            p.remote_updated_at,
            time_util::now()
        ],
    )?;
    Ok(conn.query_row(
        &format!("{PR_COLUMNS} FROM github_pull_requests WHERE github_repository_id = ?1 AND number = ?2"),
        params![github_repository_id, p.number],
        map_pr,
    )?)
}

/// After a full listing of open pull requests, mark previously open ones
/// that GitHub no longer lists as closed (their exact final state is fetched on demand).
pub fn close_missing(
    conn: &Connection,
    github_repository_id: &str,
    open_numbers: &[i64],
) -> AppResult<()> {
    let list = open_numbers
        .iter()
        .map(i64::to_string)
        .collect::<Vec<_>>()
        .join(",");
    conn.execute(
        &format!(
            "UPDATE github_pull_requests SET state = 'closed', synced_at = ?2
             WHERE github_repository_id = ?1 AND state = 'open' AND number NOT IN ({})",
            if list.is_empty() {
                "-1".to_string()
            } else {
                list
            }
        ),
        params![github_repository_id, time_util::now()],
    )?;
    Ok(())
}

pub fn pull_requests(
    conn: &Connection,
    github_repository_id: &str,
    state: Option<PullState>,
) -> AppResult<Vec<GithubPullRequest>> {
    let mut stmt = conn.prepare(&format!(
        "{PR_COLUMNS} FROM github_pull_requests WHERE github_repository_id = ?1 AND (?2 IS NULL OR state = ?2)
         ORDER BY remote_updated_at DESC, number DESC"
    ))?;
    let rows = stmt.query_map(params![github_repository_id, state], map_pr)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use crate::error::ErrorCode;
    use serde_json::json;

    fn account(db: &Database, user_id: i64) -> GithubAccount {
        db.write(|c| {
            connect(
                c,
                &AccountUpsert {
                    host: "github.com".into(),
                    api_base_url: "https://api.github.com".into(),
                    github_user_id: user_id,
                    login: format!("user{user_id}"),
                    name: None,
                    html_url: None,
                    auth_method: AuthMethod::PersonalAccessToken,
                    credential_kind: CredentialKind::GithubPersonalAccessToken,
                    scopes: None,
                    rate_limit: Some(json!({ "remaining": 4999 })),
                },
            )
        })
        .unwrap()
    }

    fn repo_upsert(github_id: i64, owner: &str, name: &str) -> RepositoryUpsert {
        RepositoryUpsert {
            github_id,
            owner: owner.into(),
            name: name.into(),
            full_name: format!("{owner}/{name}"),
            html_url: format!("https://github.com/{owner}/{name}"),
            clone_url: Some(format!("https://github.com/{owner}/{name}.git")),
            default_branch: Some("main".into()),
            ..Default::default()
        }
    }

    #[test]
    fn single_connected_account_and_reconnect_updates_in_place() {
        let db = Database::open_in_memory();
        let a = account(&db, 1);
        assert!(!a.scopes_reported, "fine-grained tokens report no scopes");
        let b = account(&db, 2);
        assert_eq!(
            db.read(|c| get_account(c, &a.id)).unwrap().status,
            AccountStatus::Disconnected
        );
        assert_eq!(db.read(active).unwrap().unwrap().id, b.id);
        let again = account(&db, 2);
        assert_eq!(again.id, b.id);
    }

    #[test]
    fn repositories_upsert_by_stable_id_and_follow_renames() {
        let db = Database::open_in_memory();
        let a = account(&db, 1);
        let r1 = db
            .write(|c| upsert_repository(c, &a.id, &repo_upsert(77, "acme", "api")))
            .unwrap();
        let renamed = db
            .write(|c| upsert_repository(c, &a.id, &repo_upsert(77, "acme", "orders-api")))
            .unwrap();
        assert_eq!(r1.id, renamed.id);
        assert_eq!(renamed.full_name, "acme/orders-api");
    }

    #[test]
    fn metadata_only_import_then_local_discovery_links_the_same_record() {
        use crate::db::repositories::{record_local_discovery, LocalRepositoryRecord};
        use crate::git::inspect::{GitKind, GitMetadata};
        let db = Database::open_in_memory();
        let a = account(&db, 1);
        let gh = db
            .write(|c| upsert_repository(c, &a.id, &repo_upsert(77, "acme", "api")))
            .unwrap();
        let (repo, created) = db.write(|c| import_metadata_only(c, &gh.id)).unwrap();
        assert!(created);
        assert_eq!(repo.source_type, repositories::SourceType::Github);
        assert_eq!(repo.local_path, None);
        assert!(
            !db.write(|c| import_metadata_only(c, &gh.id)).unwrap().1,
            "idempotent"
        );

        // Discovering a local checkout with the same origin attaches its path to the same record.
        let root = db
            .write(|c| {
                crate::db::roots::record_discovery(
                    c,
                    &crate::db::roots::RootSettings {
                        canonical_path: "/work".into(),
                        display_name: "work".into(),
                        root_kind: crate::db::roots::RootKind::Directory,
                        max_depth: 2,
                        follow_internal_symlinks: false,
                    },
                )
            })
            .unwrap();
        let (local, created) = db
            .write(|c| {
                record_local_discovery(
                    c,
                    &root.id,
                    &LocalRepositoryRecord {
                        name: "api".into(),
                        local_path: "/work/api".into(),
                        git_kind: GitKind::Standard,
                        metadata: GitMetadata {
                            current_branch: Some("main".into()),
                            detached: false,
                            head_commit: Some("abc".into()),
                            default_branch: Some("main".into()),
                            origin: Some(crate::git::remote::parse_remote(
                                "git@github.com:acme/api.git",
                            )),
                            last_commit_at: None,
                            working_tree: None,
                            warnings: vec![],
                        },
                        collected_at: "t".into(),
                    },
                )
            })
            .unwrap();
        assert!(!created, "metadata-only record becomes local-linked");
        assert_eq!(local.id, repo.id);
        assert_eq!(local.local_path.as_deref(), Some("/work/api"));
        assert_eq!(local.github_owner.as_deref(), Some("acme"));
        assert!(local.git_warnings.as_array().unwrap().is_empty());
    }

    #[test]
    fn pull_requests_keep_mergeability_until_head_changes_and_close_missing() {
        let db = Database::open_in_memory();
        let a = account(&db, 1);
        let gh = db
            .write(|c| upsert_repository(c, &a.id, &repo_upsert(77, "acme", "api")))
            .unwrap();
        let pr = |sha: &str, mergeable: Option<bool>| PullUpsert {
            github_id: 900,
            number: 12,
            title: "Upgrade Node".into(),
            html_url: "https://github.com/acme/api/pull/12".into(),
            head_ref: "shiftops/node-20".into(),
            head_sha: sha.into(),
            base_ref: "main".into(),
            mergeable,
            checks: mergeable.map(|_| json!({ "total": 3, "success": 3 })),
            ..Default::default()
        };
        db.write(|c| upsert_pull_request(c, &gh.id, &pr("aaa", Some(true))))
            .unwrap();
        let listed = db
            .write(|c| upsert_pull_request(c, &gh.id, &pr("aaa", None)))
            .unwrap();
        assert_eq!(
            listed.mergeable,
            Some(true),
            "list results do not erase known mergeability"
        );
        let moved = db
            .write(|c| upsert_pull_request(c, &gh.id, &pr("bbb", None)))
            .unwrap();
        assert_eq!(
            (moved.mergeable, moved.checks),
            (None, None),
            "new head commit invalidates checks"
        );
        db.write(|c| close_missing(c, &gh.id, &[])).unwrap();
        assert_eq!(
            db.read(|c| pull_requests(c, &gh.id, Some(PullState::Open)))
                .unwrap()
                .len(),
            0
        );
        let err = db
            .write(|c| upsert_pull_request(c, "missing", &pr("x", None)))
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::ValidationFailed);
    }

    #[test]
    fn organizations_are_replaced_on_refresh() {
        let db = Database::open_in_memory();
        let a = account(&db, 1);
        db.write(|c| {
            replace_organizations(
                c,
                &a.id,
                &[
                    (1, "acme".into(), None),
                    (2, "beta".into(), Some("b".into())),
                ],
            )
        })
        .unwrap();
        db.write(|c| replace_organizations(c, &a.id, &[(2, "beta".into(), None)]))
            .unwrap();
        let orgs = db.read(|c| organizations(c, &a.id)).unwrap();
        assert_eq!(
            orgs.iter().map(|o| o.login.as_str()).collect::<Vec<_>>(),
            ["beta"]
        );
    }
}
