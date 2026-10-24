//! GitHub integration: OAuth Device Authorization Flow (no client secret),
//! fine-grained personal-access-token fallback, and read-only provider
//! operations. Tokens live only in the OS keychain and are used only here.

pub mod client;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::credentials::{self, CredentialStore, Secret};
use crate::db::activity::{self, NewEvent, Severity};
use crate::db::credentials::{CredentialKind, Provider};
use crate::db::github::{
    self as store, AccountStatus, AccountUpsert, AuthMethod, GithubAccount, GithubPullRequest,
    GithubRepository, PullState, PullUpsert, RepositoryUpsert,
};
use crate::db::Database;
use crate::error::{AppError, AppResult, ErrorCode};
use client::{ApiResponse, GithubClient, GithubConfig, RateLimit};

/// Scopes requested by the OAuth device flow: read repository contents and
/// pull requests (`repo` is GitHub's only scope covering private repositories)
/// and read organisation membership.
pub const OAUTH_SCOPES: &str = "repo read:org";
const MAX_FILE_BYTES: u64 = 512 * 1024;

// ---------------------------------------------------------------- cancellable requests

/// Cancellation for long-running provider requests, keyed by a caller-chosen id.
#[derive(Clone, Default)]
pub struct Requests {
    inner: Arc<Mutex<HashMap<String, tokio::sync::watch::Sender<bool>>>>,
}

impl Requests {
    pub async fn run<T>(
        &self,
        request_id: Option<String>,
        fut: impl std::future::Future<Output = AppResult<T>>,
    ) -> AppResult<T> {
        let Some(id) = request_id else {
            return fut.await;
        };
        let (tx, mut rx) = tokio::sync::watch::channel(false);
        self.inner
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(id.clone(), tx);
        let result = tokio::select! {
            r = fut => r,
            _ = rx.wait_for(|c| *c) => Err(AppError::new(ErrorCode::CommandCancelled, "The GitHub request was cancelled.")),
        };
        self.inner
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&id);
        result
    }

    pub fn cancel(&self, request_id: &str) -> bool {
        match self
            .inner
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(request_id)
        {
            Some(tx) => tx.send(true).is_ok(),
            None => false,
        }
    }
}

// ---------------------------------------------------------------- status

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubStatus {
    pub host: String,
    pub api_base_url: String,
    pub oauth_configured: bool,
    pub account: Option<GithubAccount>,
    pub credential_stored: bool,
}

pub fn status(
    config: &GithubConfig,
    db: &Database,
    keychain: &CredentialStore,
) -> AppResult<GithubStatus> {
    let account = db.read(store::active)?;
    let credential_stored = match &account {
        Some(a) => keychain.exists(Provider::Github, a.credential_kind)?,
        None => false,
    };
    Ok(GithubStatus {
        host: config.host.to_string(),
        api_base_url: config.api_base.to_string(),
        oauth_configured: config.oauth_client_id.is_some(),
        account,
        credential_stored,
    })
}

fn rate_json(r: &Option<RateLimit>) -> Option<Value> {
    r.as_ref().and_then(|r| serde_json::to_value(r).ok())
}

/// Validate a token against `GET /user` and the organisation list, then
/// store it in the keychain and record the account. Nothing is stored when
/// validation fails.
async fn connect_with_token(
    config: &GithubConfig,
    db: &Database,
    keychain: &CredentialStore,
    token: Secret,
    method: AuthMethod,
) -> AppResult<GithubAccount> {
    let kind = match method {
        AuthMethod::OauthDevice => CredentialKind::GithubOauthToken,
        AuthMethod::PersonalAccessToken => CredentialKind::GithubPersonalAccessToken,
    };
    let client = GithubClient::new(config, Secret::new(token.expose().to_string()))?;
    let user: ApiResponse<client::User> = client.get("user", &[]).await?;
    let (orgs, rate, _) = client.get_all::<client::Org>("user/orgs", &[], 5).await?;

    // Keychain writes are brief; validation above already succeeded.
    credentials::store_internal(db, keychain, Provider::Github, kind, token)?;

    let account = db.write(|c| {
        let account = store::connect(
            c,
            &AccountUpsert {
                host: config.host_name(),
                api_base_url: config.api_base.to_string(),
                github_user_id: user.data.id,
                login: user.data.login.clone(),
                name: user.data.name.clone(),
                html_url: Some(user.data.html_url.clone()),
                auth_method: method,
                credential_kind: kind,
                scopes: user.scopes.clone(),
                rate_limit: rate_json(&rate.or(user.rate_limit.clone())),
            },
        )?;
        store::replace_organizations(c, &account.id, &orgs.iter().map(|o| (o.id, o.login.clone(), o.description.clone())).collect::<Vec<_>>())?;
        activity::record(
            c,
            NewEvent::new(
                "github_account",
                "github.connected",
                Severity::Notice,
                format!("Connected GitHub account {} on {} ({})", account.login, account.host, method.as_str().replace('_', " ")),
            )
            .entity(account.id.clone())
            .metadata(json!({ "login": account.login, "host": account.host, "authMethod": method.as_str(), "organizations": orgs.len(), "scopes": user.scopes })),
        )?;
        Ok(account)
    })?;
    Ok(account)
}

pub async fn connect_pat(
    config: &GithubConfig,
    db: &Database,
    keychain: &CredentialStore,
    token: Secret,
) -> AppResult<GithubAccount> {
    let value = token.expose();
    if value.trim().is_empty() || value.trim() != value || value.chars().any(char::is_whitespace) {
        return Err(AppError::validation(
            "Paste only the token, without spaces or line breaks.",
        ));
    }
    connect_with_token(config, db, keychain, token, AuthMethod::PersonalAccessToken).await
}

/// The active account and a client using its keychain token.
pub fn active_client(
    config: &GithubConfig,
    db: &Database,
    keychain: &CredentialStore,
) -> AppResult<(GithubAccount, GithubClient)> {
    let account = db.read(store::active)?.ok_or_else(|| {
        AppError::new(
            ErrorCode::GithubAuthenticationFailed,
            "GitHub is not connected.",
        )
        .with_action("Connect a GitHub account in Integrations.")
    })?;
    if account.host != config.host_name() {
        return Err(AppError::new(
            ErrorCode::GithubNotConfigured,
            format!(
                "The connected account is for {}, but ShiftOps is configured for {}.",
                account.host,
                config.host_name()
            ),
        ));
    }
    let token = keychain
        .retrieve(Provider::Github, account.credential_kind)
        .map_err(|e| {
            if e.code == ErrorCode::RecordNotFound {
                AppError::new(
                    ErrorCode::GithubAuthenticationFailed,
                    "The GitHub token is missing from the keychain.",
                )
                .with_action("Reconnect GitHub in Integrations.")
            } else {
                e
            }
        })?;
    Ok((account.clone(), GithubClient::new(config, token)?))
}

/// Run a provider call; a 401 marks the account invalid so the UI asks for reconnection.
async fn guarded<T>(
    db: &Database,
    account: &GithubAccount,
    fut: impl std::future::Future<Output = AppResult<T>>,
) -> AppResult<T> {
    let result = fut.await;
    if let Err(e) = &result {
        if e.code == ErrorCode::GithubAuthenticationFailed {
            let _ = db.write(|c| store::set_status(c, &account.id, AccountStatus::Invalid));
        }
    }
    result
}

pub async fn refresh(
    config: &GithubConfig,
    db: &Database,
    keychain: &CredentialStore,
) -> AppResult<GithubAccount> {
    let (account, client) = active_client(config, db, keychain)?;
    guarded(db, &account, async {
        let user: ApiResponse<client::User> = client.get("user", &[]).await?;
        let (orgs, rate, _) = client.get_all::<client::Org>("user/orgs", &[], 5).await?;
        db.write(|c| {
            store::record_validation(
                c,
                &account.id,
                user.scopes.as_deref(),
                rate_json(&rate.or(user.rate_limit)).as_ref(),
            )?;
            store::replace_organizations(
                c,
                &account.id,
                &orgs
                    .iter()
                    .map(|o| (o.id, o.login.clone(), o.description.clone()))
                    .collect::<Vec<_>>(),
            )?;
            store::get_account(c, &account.id)
        })
    })
    .await
}

pub fn disconnect(db: &Database, keychain: &CredentialStore) -> AppResult<Option<GithubAccount>> {
    let Some(account) = db.read(store::active)? else {
        return Ok(None);
    };
    credentials::delete_credential(db, keychain, Provider::Github, account.credential_kind)?;
    db.write(|c| {
        store::set_status(c, &account.id, AccountStatus::Disconnected)?;
        activity::record(
            c,
            NewEvent::new(
                "github_account",
                "github.disconnected",
                Severity::Notice,
                format!("Disconnected GitHub account {}", account.login),
            )
            .entity(account.id.clone()),
        )?;
        store::get_account(c, &account.id).map(Some)
    })
}

pub async fn rate_limit(
    config: &GithubConfig,
    db: &Database,
    keychain: &CredentialStore,
) -> AppResult<Value> {
    let (account, client) = active_client(config, db, keychain)?;
    guarded(db, &account, async {
        let r: ApiResponse<client::RateLimitResponse> = client.get("rate_limit", &[]).await?;
        let res = &r.data.resources;
        let to = |x: &client::RateLimitResource| json!({ "limit": x.limit, "remaining": x.remaining, "reset": x.reset, "used": x.used });
        let value = json!({
            "resource": "core",
            "limit": res.core.limit,
            "remaining": res.core.remaining,
            "reset": res.core.reset,
            "used": res.core.used,
            "search": res.search.as_ref().map(to),
            "graphql": res.graphql.as_ref().map(to),
        });
        db.write(|c| store::record_rate_limit(c, &account.id, &value))?;
        Ok(value)
    })
    .await
}

// ---------------------------------------------------------------- device flow

#[derive(Debug, Deserialize)]
struct DeviceCode {
    device_code: String,
    user_code: String,
    verification_uri: String,
    expires_in: u64,
    interval: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct TokenPoll {
    access_token: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
    interval: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceFlowStart {
    pub flow_id: String,
    /// Shown to the user; the device code itself never leaves Rust.
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in_seconds: u64,
    pub interval_seconds: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceFlowState {
    Pending,
    SlowDown,
    Succeeded,
    Expired,
    Denied,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceFlowEvent {
    pub flow_id: String,
    pub state: DeviceFlowState,
    pub interval_seconds: u64,
    pub account: Option<GithubAccount>,
    pub error: Option<AppError>,
}

pub trait AuthSink: Send + Sync {
    fn emit(&self, event: &DeviceFlowEvent);
}

pub const TAURI_EVENT: &str = "shiftops://github-auth";

pub struct TauriAuthSink<R: tauri::Runtime>(pub tauri::AppHandle<R>);

impl<R: tauri::Runtime> AuthSink for TauriAuthSink<R> {
    fn emit(&self, event: &DeviceFlowEvent) {
        use tauri::Emitter;
        if let Err(e) = self.0.emit(TAURI_EVENT, event) {
            crate::logging::warn("github", &format!("failed to emit auth event: {e}"));
        }
    }
}

#[derive(Clone, Default)]
pub struct DeviceFlows {
    flows: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
}

impl DeviceFlows {
    pub fn cancel(&self, flow_id: &str) -> bool {
        match self
            .flows
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(flow_id)
        {
            Some(flag) => {
                flag.store(true, Ordering::Relaxed);
                true
            }
            None => false,
        }
    }

    /// Request a device code and poll for the token in the background.
    pub async fn start(
        &self,
        config: GithubConfig,
        db: Arc<Database>,
        keychain: CredentialStore,
        sink: Arc<dyn AuthSink>,
    ) -> AppResult<(DeviceFlowStart, tokio::task::JoinHandle<DeviceFlowState>)> {
        let client_id = config.oauth_client_id.clone().ok_or_else(|| {
            AppError::new(
                ErrorCode::GithubNotConfigured,
                "No GitHub OAuth client ID is configured, so device sign-in is unavailable.",
            )
        })?;
        let http = config.http()?;
        let url = config
            .host
            .join("login/device/code")
            .map_err(|e| AppError::internal(e.to_string()))?;
        let response = http
            .post(url)
            .header(reqwest::header::ACCEPT, "application/json")
            .form(&[("client_id", client_id.as_str()), ("scope", OAUTH_SCOPES)])
            .send()
            .await
            .map_err(|e| client::transport_error(e, "login/device/code"))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| client::transport_error(e, "login/device/code"))?;
        let parsed: Result<DeviceCode, _> = serde_json::from_str(&body);
        let code = match (status.is_success(), parsed) {
            (true, Ok(code)) => code,
            _ => {
                let reason = serde_json::from_str::<TokenPoll>(&body)
                    .ok()
                    .and_then(|p| p.error_description.or(p.error))
                    .unwrap_or_else(|| format!("HTTP {}", status.as_u16()));
                return Err(AppError::new(
                    ErrorCode::GithubNotConfigured,
                    "GitHub refused to start device sign-in.",
                )
                .with_details(crate::redact::redact(&reason))
                .with_action("Check that the OAuth app exists and has Device Flow enabled."));
            }
        };
        if crate::urls::validate_external_url(&code.verification_uri).is_err() {
            return Err(AppError::internal(
                "GitHub returned an unsafe verification URL.",
            ));
        }
        let flow_id = crate::db::new_id();
        let cancel = Arc::new(AtomicBool::new(false));
        self.flows
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(flow_id.clone(), cancel.clone());
        let interval = code.interval.unwrap_or(5).max(1);
        let start = DeviceFlowStart {
            flow_id: flow_id.clone(),
            user_code: code.user_code.clone(),
            verification_uri: code.verification_uri.clone(),
            expires_in_seconds: code.expires_in,
            interval_seconds: interval,
        };
        let flows = self.clone();
        let device_code = Secret::new(code.device_code);
        let deadline =
            Instant::now() + config.poll_unit * u32::try_from(code.expires_in).unwrap_or(u32::MAX);
        let handle = tokio::spawn(async move {
            let state = poll(
                &config,
                &client_id,
                &db,
                &keychain,
                device_code,
                interval,
                deadline,
                &cancel,
                &flow_id,
                sink.as_ref(),
            )
            .await;
            flows
                .flows
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .remove(&flow_id);
            state
        });
        Ok((start, handle))
    }
}

#[allow(clippy::too_many_arguments)]
async fn poll(
    config: &GithubConfig,
    client_id: &str,
    db: &Database,
    keychain: &CredentialStore,
    device_code: Secret,
    mut interval: u64,
    deadline: Instant,
    cancel: &AtomicBool,
    flow_id: &str,
    sink: &dyn AuthSink,
) -> DeviceFlowState {
    let emit = |state, interval, account: Option<GithubAccount>, error: Option<AppError>| {
        sink.emit(&DeviceFlowEvent {
            flow_id: flow_id.to_string(),
            state,
            interval_seconds: interval,
            account,
            error,
        });
        state
    };
    let Ok(http) = config.http() else {
        return emit(
            DeviceFlowState::Failed,
            interval,
            None,
            Some(AppError::internal("HTTP client unavailable")),
        );
    };
    let Ok(url) = config.host.join("login/oauth/access_token") else {
        return emit(
            DeviceFlowState::Failed,
            interval,
            None,
            Some(AppError::internal("invalid token URL")),
        );
    };
    loop {
        // Sleep in small steps so cancellation is prompt.
        let wake = Instant::now() + config.poll_unit * u32::try_from(interval).unwrap_or(u32::MAX);
        while Instant::now() < wake {
            if cancel.load(Ordering::Relaxed) {
                return emit(DeviceFlowState::Cancelled, interval, None, None);
            }
            tokio::time::sleep(Duration::from_millis(50).min(config.poll_unit)).await;
        }
        if cancel.load(Ordering::Relaxed) {
            return emit(DeviceFlowState::Cancelled, interval, None, None);
        }
        if Instant::now() >= deadline {
            return emit(
                DeviceFlowState::Expired,
                interval,
                None,
                Some(AppError::new(
                    ErrorCode::GithubAuthorizationExpired,
                    "The sign-in code expired before it was approved.",
                )),
            );
        }
        let response = http
            .post(url.clone())
            .header(reqwest::header::ACCEPT, "application/json")
            .form(&[
                ("client_id", client_id),
                ("device_code", device_code.expose()),
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ])
            .send()
            .await;
        let body = match response {
            Ok(r) => r.text().await.unwrap_or_default(),
            Err(e) => {
                // Transient network failure: keep polling until the code expires.
                crate::logging::warn("github", &format!("device flow poll failed: {e}"));
                emit(DeviceFlowState::Pending, interval, None, None);
                continue;
            }
        };
        let parsed: TokenPoll = match serde_json::from_str(&body) {
            Ok(p) => p,
            Err(_) => {
                return emit(
                    DeviceFlowState::Failed,
                    interval,
                    None,
                    Some(AppError::internal(
                        "GitHub returned an unexpected token response.",
                    )),
                )
            }
        };
        if let Some(token) = parsed.access_token {
            return match connect_with_token(
                config,
                db,
                keychain,
                Secret::new(token),
                AuthMethod::OauthDevice,
            )
            .await
            {
                Ok(account) => emit(DeviceFlowState::Succeeded, interval, Some(account), None),
                Err(e) => emit(DeviceFlowState::Failed, interval, None, Some(e)),
            };
        }
        match parsed.error.as_deref() {
            Some("authorization_pending") => {
                emit(DeviceFlowState::Pending, interval, None, None);
            }
            Some("slow_down") => {
                interval = parsed.interval.unwrap_or(interval + 5).max(interval + 1);
                emit(DeviceFlowState::SlowDown, interval, None, None);
            }
            Some("expired_token") => {
                return emit(
                    DeviceFlowState::Expired,
                    interval,
                    None,
                    Some(AppError::new(
                        ErrorCode::GithubAuthorizationExpired,
                        "The sign-in code expired before it was approved.",
                    )),
                )
            }
            Some("access_denied") => {
                return emit(
                    DeviceFlowState::Denied,
                    interval,
                    None,
                    Some(AppError::new(
                        ErrorCode::GithubAuthorizationDenied,
                        "Authorization was declined on GitHub.",
                    )),
                )
            }
            other => {
                let detail = parsed
                    .error_description
                    .or(other.map(str::to_string))
                    .unwrap_or_default();
                return emit(
                    DeviceFlowState::Failed,
                    interval,
                    None,
                    Some(
                        AppError::new(
                            ErrorCode::GithubAuthenticationFailed,
                            "GitHub ended device sign-in with an error.",
                        )
                        .with_details(detail),
                    ),
                );
            }
        }
    }
}

// ---------------------------------------------------------------- browsing and details

fn upsert_from(r: &client::Repo) -> RepositoryUpsert {
    RepositoryUpsert {
        github_id: r.id,
        owner: r.owner.login.clone(),
        name: r.name.clone(),
        full_name: r.full_name.clone(),
        private: r.private,
        archived: r.archived,
        fork: r.fork,
        default_branch: r.default_branch.clone(),
        html_url: r.html_url.clone(),
        clone_url: r
            .clone_url
            .as_deref()
            .map(|u| crate::git::remote::parse_remote(u).sanitized_url),
        ssh_url: r.ssh_url.clone(),
        description: r.description.clone(),
        primary_language: r.language.clone(),
        pushed_at: r.pushed_at.clone(),
        remote_updated_at: r.updated_at.clone(),
        permissions: r.permissions.clone(),
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct BrowseParams {
    /// Organisation or user login; `None` lists every repository the account can access.
    pub owner: Option<String>,
    pub query: Option<String>,
    pub page: Option<u32>,
    pub per_page: Option<u32>,
    /// `all` (default), `public`, `private`.
    pub visibility: Option<String>,
    pub include_archived: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowsePage {
    pub items: Vec<GithubRepository>,
    pub page: u32,
    pub per_page: u32,
    pub has_next_page: bool,
    /// Present for search results.
    pub total_count: Option<u64>,
}

fn safe_segment(s: &str, what: &str) -> AppResult<String> {
    let ok = !s.is_empty()
        && s.len() <= 100
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if ok && s != "." && s != ".." {
        Ok(s.to_string())
    } else {
        Err(AppError::validation(format!("Invalid GitHub {what}.")).with_context(what, s))
    }
}

pub async fn browse(
    config: &GithubConfig,
    db: &Database,
    keychain: &CredentialStore,
    p: &BrowseParams,
) -> AppResult<BrowsePage> {
    let (account, client) = active_client(config, db, keychain)?;
    let page = p.page.unwrap_or(1).max(1);
    let per_page = p.per_page.unwrap_or(30).clamp(1, 100);
    let owner = p
        .owner
        .as_deref()
        .filter(|o| !o.is_empty())
        .map(|o| safe_segment(o, "owner"))
        .transpose()?;
    let visibility = match p.visibility.as_deref() {
        None | Some("all") => "all",
        Some("public") => "public",
        Some("private") => "private",
        Some(other) => {
            return Err(AppError::validation(format!(
                "Unknown visibility filter: {other}"
            )))
        }
    };
    let org_logins: Vec<String> = db
        .read(|c| store::organizations(c, &account.id))?
        .into_iter()
        .map(|o| o.login)
        .collect();
    guarded(db, &account, async {
        let base_q = vec![
            ("per_page", per_page.to_string()),
            ("page", page.to_string()),
        ];
        let (repos, has_next, total, rate) =
            if let Some(query) = p.query.as_deref().map(str::trim).filter(|q| !q.is_empty()) {
                if query.len() > 200 {
                    return Err(AppError::validation("Search text is too long."));
                }
                let mut q = format!("{query} in:name fork:true");
                if let Some(o) = &owner {
                    let qualifier = if org_logins.iter().any(|l| l.eq_ignore_ascii_case(o)) {
                        "org"
                    } else {
                        "user"
                    };
                    q.push_str(&format!(" {qualifier}:{o}"));
                }
                if visibility != "all" {
                    q.push_str(&format!(" is:{visibility}"));
                }
                if !p.include_archived {
                    q.push_str(" archived:false");
                }
                let mut query = base_q.clone();
                query.push(("q", q));
                let r: ApiResponse<client::SearchRepos> =
                    client.get("search/repositories", &query).await?;
                (
                    r.data.items,
                    r.has_next_page,
                    Some(r.data.total_count),
                    r.rate_limit,
                )
            } else {
                let mut query = base_q.clone();
                query.push(("sort", "updated".into()));
                let path = match &owner {
                    Some(o) if org_logins.iter().any(|l| l.eq_ignore_ascii_case(o)) => {
                        query.push((
                            "type",
                            if visibility == "all" {
                                "all".into()
                            } else {
                                visibility.to_string()
                            },
                        ));
                        format!("orgs/{o}/repos")
                    }
                    Some(o) if o.eq_ignore_ascii_case(&account.login) => {
                        query.push(("affiliation", "owner".into()));
                        query.push(("visibility", visibility.into()));
                        "user/repos".into()
                    }
                    Some(o) => format!("users/{o}/repos"),
                    None => {
                        query.push((
                            "affiliation",
                            "owner,collaborator,organization_member".into(),
                        ));
                        query.push(("visibility", visibility.into()));
                        "user/repos".into()
                    }
                };
                let r: ApiResponse<Vec<client::Repo>> = client.get(&path, &query).await?;
                (r.data, r.has_next_page, None, r.rate_limit)
            };
        let repos: Vec<&client::Repo> = repos
            .iter()
            .filter(|r| p.include_archived || !r.archived)
            .collect();
        let items = db.write(|c| {
            if let Some(rate) = rate_json(&rate) {
                store::record_rate_limit(c, &account.id, &rate)?;
            }
            repos
                .iter()
                .map(|r| store::upsert_repository(c, &account.id, &upsert_from(r)))
                .collect::<AppResult<Vec<_>>>()
        })?;
        Ok(BrowsePage {
            items,
            page,
            per_page,
            has_next_page: has_next,
            total_count: total,
        })
    })
    .await
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryDetails {
    pub repository: GithubRepository,
    pub languages: Value,
    pub branches: Vec<client::Branch>,
    pub branches_truncated: bool,
}

pub async fn details(
    config: &GithubConfig,
    db: &Database,
    keychain: &CredentialStore,
    owner: &str,
    name: &str,
) -> AppResult<RepositoryDetails> {
    let owner = safe_segment(owner, "owner")?;
    let name = safe_segment(name, "repository")?;
    let (account, client) = active_client(config, db, keychain)?;
    guarded(db, &account, async {
        let repo: ApiResponse<client::Repo> =
            client.get(&format!("repos/{owner}/{name}"), &[]).await?;
        let languages: ApiResponse<Value> = client
            .get(&format!("repos/{owner}/{name}/languages"), &[])
            .await?;
        let (branches, rate, truncated) = client
            .get_all::<client::Branch>(&format!("repos/{owner}/{name}/branches"), &[], 3)
            .await?;
        let repository = db.write(|c| {
            let stored = store::upsert_repository(c, &account.id, &upsert_from(&repo.data))?;
            store::set_languages(c, &stored.id, &languages.data)?;
            if let Some(rate) = rate_json(&rate) {
                store::record_rate_limit(c, &account.id, &rate)?;
            }
            store::get_repository(c, &stored.id)
        })?;
        Ok(RepositoryDetails {
            repository,
            languages: languages.data,
            branches,
            branches_truncated: truncated,
        })
    })
    .await
}

fn pull_state(p: &client::Pull) -> PullState {
    if p.merged_at.is_some() {
        PullState::Merged
    } else if p.state == "closed" {
        PullState::Closed
    } else {
        PullState::Open
    }
}

fn pull_upsert(p: &client::Pull, checks: Option<Value>) -> PullUpsert {
    PullUpsert {
        github_id: p.id,
        number: p.number,
        title: p.title.clone(),
        state: Some(pull_state(p)),
        draft: p.draft,
        author_login: p.user.as_ref().map(|u| u.login.clone()),
        html_url: p.html_url.clone(),
        head_ref: p.head.ref_name.clone(),
        head_sha: p.head.sha.clone(),
        head_repo_full_name: p.head.repo.as_ref().map(|r| r.full_name.clone()),
        base_ref: p.base.ref_name.clone(),
        base_sha: Some(p.base.sha.clone()),
        mergeable: p.mergeable,
        mergeable_state: p.mergeable_state.clone(),
        checks,
        remote_created_at: p.created_at.clone(),
        remote_updated_at: p.updated_at.clone(),
    }
}

async fn ensure_repository(
    client: &GithubClient,
    db: &Database,
    account: &GithubAccount,
    owner: &str,
    name: &str,
) -> AppResult<GithubRepository> {
    if let Some(r) = db.read(|c| store::find_repository(c, &account.id, owner, name))? {
        return Ok(r);
    }
    let repo: ApiResponse<client::Repo> = client.get(&format!("repos/{owner}/{name}"), &[]).await?;
    db.write(|c| store::upsert_repository(c, &account.id, &upsert_from(&repo.data)))
}

/// Open pull requests with a check summary for each head commit (first 30).
pub async fn pull_requests(
    config: &GithubConfig,
    db: &Database,
    keychain: &CredentialStore,
    owner: &str,
    name: &str,
) -> AppResult<Vec<GithubPullRequest>> {
    let owner = safe_segment(owner, "owner")?;
    let name = safe_segment(name, "repository")?;
    let (account, client) = active_client(config, db, keychain)?;
    guarded(db, &account, async {
        let repo = ensure_repository(&client, db, &account, &owner, &name).await?;
        let (pulls, rate, truncated) = client
            .get_all::<client::Pull>(
                &format!("repos/{owner}/{name}/pulls"),
                &[("state", "open".into()), ("sort", "updated".into())],
                3,
            )
            .await?;
        let mut with_checks = Vec::with_capacity(pulls.len());
        for (i, p) in pulls.iter().enumerate() {
            let checks = if i < 30 {
                let runs: ApiResponse<client::CheckRuns> = client
                    .get(
                        &format!("repos/{owner}/{name}/commits/{}/check-runs", p.head.sha),
                        &[("per_page", "100".into())],
                    )
                    .await?;
                Some(runs.data.summary())
            } else {
                None
            };
            with_checks.push(pull_upsert(p, checks));
        }
        db.write(|c| {
            for p in &with_checks {
                store::upsert_pull_request(c, &repo.id, p)?;
            }
            if !truncated {
                store::close_missing(
                    c,
                    &repo.id,
                    &pulls.iter().map(|p| p.number).collect::<Vec<_>>(),
                )?;
            }
            if let Some(rate) = rate_json(&rate) {
                store::record_rate_limit(c, &account.id, &rate)?;
            }
            store::pull_requests(c, &repo.id, Some(PullState::Open))
        })
    })
    .await
}

/// One pull request, including mergeability (computed by GitHub on demand) and checks.
pub async fn pull_request(
    config: &GithubConfig,
    db: &Database,
    keychain: &CredentialStore,
    owner: &str,
    name: &str,
    number: i64,
) -> AppResult<GithubPullRequest> {
    let owner = safe_segment(owner, "owner")?;
    let name = safe_segment(name, "repository")?;
    if number <= 0 {
        return Err(AppError::validation(
            "Pull request number must be positive.",
        ));
    }
    let (account, client) = active_client(config, db, keychain)?;
    guarded(db, &account, async {
        let repo = ensure_repository(&client, db, &account, &owner, &name).await?;
        let pull: ApiResponse<client::Pull> = client
            .get(&format!("repos/{owner}/{name}/pulls/{number}"), &[])
            .await?;
        let runs: ApiResponse<client::CheckRuns> = client
            .get(
                &format!(
                    "repos/{owner}/{name}/commits/{}/check-runs",
                    pull.data.head.sha
                ),
                &[("per_page", "100".into())],
            )
            .await?;
        db.write(|c| {
            store::upsert_pull_request(
                c,
                &repo.id,
                &pull_upsert(&pull.data, Some(runs.data.summary())),
            )
        })
    })
    .await
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteFile {
    pub path: String,
    pub sha: String,
    pub size: u64,
    pub text: String,
}

/// Fetch one explicitly requested configuration file. Secret files and
/// files over 512 KiB are refused; the content is returned redacted.
pub async fn file(
    config: &GithubConfig,
    db: &Database,
    keychain: &CredentialStore,
    owner: &str,
    name: &str,
    path: &str,
    git_ref: Option<&str>,
) -> AppResult<RemoteFile> {
    let owner = safe_segment(owner, "owner")?;
    let name = safe_segment(name, "repository")?;
    let clean = path.trim().trim_start_matches('/');
    if clean.is_empty()
        || clean.len() > 300
        || clean
            .split('/')
            .any(|s| s.is_empty() || s == "." || s == "..")
        || clean.contains('\\')
    {
        return Err(AppError::validation("Invalid file path."));
    }
    let file_name = clean.rsplit('/').next().unwrap_or(clean);
    if crate::scanner::files::is_secret_file(file_name) {
        return Err(AppError::new(
            ErrorCode::PathNotAllowed,
            "ShiftOps never reads secret files such as .env or keys.",
        ));
    }
    let encoded: String = clean
        .split('/')
        .map(|s| url::form_urlencoded::byte_serialize(s.as_bytes()).collect::<String>())
        .collect::<Vec<_>>()
        .join("/");
    let (account, client) = active_client(config, db, keychain)?;
    guarded(db, &account, async {
        let mut query = Vec::new();
        if let Some(r) = git_ref.filter(|r| !r.is_empty()) {
            query.push(("ref", r.to_string()));
        }
        let c: ApiResponse<client::Contents> = client
            .get(&format!("repos/{owner}/{name}/contents/{encoded}"), &query)
            .await?;
        if c.data.kind != "file" {
            return Err(AppError::validation("The path is not a file."));
        }
        if c.data.size > MAX_FILE_BYTES {
            return Err(AppError::validation(format!(
                "The file is larger than {} KiB.",
                MAX_FILE_BYTES / 1024
            )));
        }
        let raw = c.data.content.unwrap_or_default().replace('\n', "");
        if c.data.encoding.as_deref() != Some("base64") {
            return Err(AppError::internal(
                "GitHub returned file content in an unexpected encoding.",
            ));
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(raw.as_bytes())
            .map_err(|e| AppError::internal(format!("file content could not be decoded: {e}")))?;
        let text = String::from_utf8(bytes)
            .map_err(|_| AppError::validation("The file is not UTF-8 text."))?;
        Ok(RemoteFile {
            path: c.data.path,
            sha: c.data.sha,
            size: c.data.size,
            text: crate::redact::redact(&text),
        })
    })
    .await
}

pub fn import(
    db: &Database,
    github_repository_id: &str,
) -> AppResult<(crate::db::repositories::Repository, bool)> {
    db.write(|c| {
        let (repo, created) = store::import_metadata_only(c, github_repository_id)?;
        activity::record(
            c,
            NewEvent::new(
                "repository",
                if created {
                    "github.repository_imported"
                } else {
                    "github.repository_linked"
                },
                Severity::Notice,
                format!(
                    "{} {}/{} from GitHub",
                    if created { "Imported" } else { "Linked" },
                    repo.github_owner.as_deref().unwrap_or(""),
                    repo.github_repo.as_deref().unwrap_or("")
                ),
            )
            .entity(repo.id.clone())
            .metadata(
                json!({ "githubRepositoryId": github_repository_id, "mode": "metadata_only" }),
            ),
        )?;
        Ok((repo, created))
    })
}

#[cfg(test)]
mod tests;
