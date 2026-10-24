//! GitHub REST client. Runs only in Rust; the token never reaches the web view.
//! Every response is decoded into typed structs (runtime validation) and every
//! failure is mapped to a structured `AppError`. There is no fallback data.

use std::time::Duration;

use reqwest::header::{HeaderMap, ACCEPT, AUTHORIZATION, USER_AGENT};
use reqwest::{Method, StatusCode};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use url::Url;

use crate::credentials::Secret;
use crate::error::{AppError, AppResult, ErrorCode};
use crate::redact::redact;

pub const API_VERSION: &str = "2022-11-28";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Where GitHub lives. Read in Rust from the environment or build-time
/// configuration so the web view cannot redirect tokens elsewhere.
#[derive(Debug, Clone)]
pub struct GithubConfig {
    pub host: Url,
    pub api_base: Url,
    pub oauth_client_id: Option<String>,
    /// Unit for the device-flow polling interval (one second in production).
    pub poll_unit: Duration,
}

fn setting(name: &str, build_time: Option<&'static str>) -> Option<String> {
    std::env::var(name)
        .ok()
        .or(build_time.map(str::to_string))
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// Accept `https`, or `http` to a loopback host (local test servers only).
pub fn validate_base(raw: &str, what: &str) -> AppResult<Url> {
    let mut url = Url::parse(raw.trim()).map_err(|_| {
        AppError::new(
            ErrorCode::GithubNotConfigured,
            format!("{what} is not a valid URL."),
        )
        .with_context("value", raw)
    })?;
    let loopback = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    if !(url.scheme() == "https" || (url.scheme() == "http" && loopback))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
    {
        return Err(AppError::new(
            ErrorCode::GithubNotConfigured,
            format!("{what} must be an https URL without credentials or query."),
        )
        .with_context("value", raw));
    }
    if !url.path().ends_with('/') {
        let path = format!("{}/", url.path());
        url.set_path(&path);
    }
    Ok(url)
}

impl GithubConfig {
    pub fn from_env() -> AppResult<Self> {
        let host = setting(
            "VITE_GITHUB_HOST",
            option_env!("SHIFTOPS_BUILD_VITE_GITHUB_HOST"),
        )
        .unwrap_or_else(|| "https://github.com".into());
        let host = validate_base(&host, "VITE_GITHUB_HOST")?;
        let default_api = if host.host_str() == Some("github.com") {
            "https://api.github.com".to_string()
        } else if host.host_str().is_some_and(|h| h.ends_with(".ghe.com")) {
            format!("https://api.{}", host.host_str().unwrap_or_default())
        } else {
            format!("{}api/v3", host.as_str())
        };
        let api = setting(
            "VITE_GITHUB_API_BASE_URL",
            option_env!("SHIFTOPS_BUILD_VITE_GITHUB_API_BASE_URL"),
        )
        .unwrap_or(default_api);
        Ok(Self {
            host,
            api_base: validate_base(&api, "VITE_GITHUB_API_BASE_URL")?,
            oauth_client_id: setting(
                "VITE_GITHUB_OAUTH_CLIENT_ID",
                option_env!("SHIFTOPS_BUILD_VITE_GITHUB_OAUTH_CLIENT_ID"),
            ),
            poll_unit: Duration::from_secs(1),
        })
    }

    /// The host name stored with repositories, e.g. `github.com`.
    pub fn host_name(&self) -> String {
        self.host
            .host_str()
            .unwrap_or_default()
            .to_ascii_lowercase()
    }

    pub fn http(&self) -> AppResult<reqwest::Client> {
        reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .user_agent(concat!("ShiftOps/", env!("CARGO_PKG_VERSION")))
            .https_only(self.api_base.scheme() == "https")
            .build()
            .map_err(|e| AppError::internal(format!("HTTP client could not be created: {e}")))
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RateLimit {
    pub resource: Option<String>,
    pub limit: Option<u64>,
    pub remaining: Option<u64>,
    pub used: Option<u64>,
    /// Unix seconds.
    pub reset: Option<u64>,
}

impl RateLimit {
    fn from_headers(h: &HeaderMap) -> Option<Self> {
        let num = |k: &str| h.get(k)?.to_str().ok()?.parse::<u64>().ok();
        let r = Self {
            resource: h
                .get("x-ratelimit-resource")
                .and_then(|v| v.to_str().ok())
                .map(str::to_string),
            limit: num("x-ratelimit-limit"),
            remaining: num("x-ratelimit-remaining"),
            used: num("x-ratelimit-used"),
            reset: num("x-ratelimit-reset"),
        };
        (r.limit.is_some() || r.remaining.is_some()).then_some(r)
    }
}

#[derive(Debug)]
pub struct ApiResponse<T> {
    pub data: T,
    pub rate_limit: Option<RateLimit>,
    /// `x-oauth-scopes`: present for OAuth and classic tokens, absent for fine-grained tokens.
    pub scopes: Option<Vec<String>>,
    pub has_next_page: bool,
}

pub struct GithubClient {
    http: reqwest::Client,
    api_base: Url,
    token: Secret,
}

fn next_link(headers: &HeaderMap) -> bool {
    headers
        .get("link")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|l| l.split(',').any(|part| part.contains("rel=\"next\"")))
}

/// Turn a non-success response into a structured error, without exposing headers or tokens.
fn map_status(status: StatusCode, headers: &HeaderMap, body: &str, path: &str) -> AppError {
    let message = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v.get("message").and_then(Value::as_str).map(str::to_string))
        .unwrap_or_default();
    let details = redact(&format!("{} {path}: {message}", status.as_u16()));
    let remaining = headers
        .get("x-ratelimit-remaining")
        .and_then(|v| v.to_str().ok());
    let retry_after = headers
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());
    let reset = headers
        .get("x-ratelimit-reset")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());
    let lower = message.to_ascii_lowercase();
    match status {
        StatusCode::UNAUTHORIZED => AppError::new(
            ErrorCode::GithubAuthenticationFailed,
            "GitHub rejected the stored credential.",
        )
        .with_details(details),
        StatusCode::FORBIDDEN | StatusCode::TOO_MANY_REQUESTS
            if remaining == Some("0") || retry_after.is_some() || lower.contains("rate limit") =>
        {
            let secondary = retry_after.is_some() || lower.contains("secondary");
            let mut e = AppError::new(
                ErrorCode::GithubRateLimited,
                if secondary {
                    "GitHub's secondary rate limit was hit."
                } else {
                    "GitHub's API rate limit is exhausted."
                },
            )
            .with_details(details)
            .with_context("secondary", secondary);
            if let Some(s) = retry_after {
                e = e.with_context("retryAfterSeconds", s);
            }
            if let Some(r) = reset {
                e = e.with_context("resetAt", r);
            }
            e
        }
        StatusCode::FORBIDDEN => AppError::new(
            ErrorCode::GithubPermissionDenied,
            "The GitHub token does not have permission for this.",
        )
        .with_details(details),
        StatusCode::NOT_FOUND => AppError::new(
            ErrorCode::RecordNotFound,
            "GitHub reports this as not found. It may not exist or the token may not have access.",
        )
        .with_details(details),
        StatusCode::UNPROCESSABLE_ENTITY => {
            AppError::validation("GitHub rejected the request.").with_details(details)
        }
        s if s.is_server_error() => AppError::new(
            ErrorCode::NetworkFailed,
            format!("GitHub returned {}.", s.as_u16()),
        )
        .with_details(details),
        _ => AppError::new(
            ErrorCode::NetworkFailed,
            format!("Unexpected GitHub response {}.", status.as_u16()),
        )
        .with_details(details),
    }
}

pub fn transport_error(e: reqwest::Error, path: &str) -> AppError {
    let what = if e.is_timeout() {
        "timed out"
    } else if e.is_connect() {
        "could not connect"
    } else {
        "failed"
    };
    AppError::new(
        ErrorCode::NetworkFailed,
        format!("The request to GitHub {what}."),
    )
    .with_details(redact(&format!("{path}: {e}")))
}

impl GithubClient {
    pub fn new(config: &GithubConfig, token: Secret) -> AppResult<Self> {
        Ok(Self {
            http: config.http()?,
            api_base: config.api_base.clone(),
            token,
        })
    }

    fn url(&self, path: &str) -> AppResult<Url> {
        self.api_base
            .join(path.trim_start_matches('/'))
            .map_err(|e| AppError::internal(format!("invalid GitHub path {path}: {e}")))
    }

    pub async fn send<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, String)],
    ) -> AppResult<ApiResponse<T>> {
        let url = self.url(path)?;
        let response = self
            .http
            .request(method, url)
            .query(query)
            .header(ACCEPT, "application/vnd.github+json")
            .header("X-GitHub-Api-Version", API_VERSION)
            .header(USER_AGENT, concat!("ShiftOps/", env!("CARGO_PKG_VERSION")))
            .header(AUTHORIZATION, format!("Bearer {}", self.token.expose()))
            .send()
            .await
            .map_err(|e| transport_error(e, path))?;
        let status = response.status();
        let headers = response.headers().clone();
        let body = response
            .text()
            .await
            .map_err(|e| transport_error(e, path))?;
        if !status.is_success() {
            return Err(map_status(status, &headers, &body, path));
        }
        let data = serde_json::from_str::<T>(&body).map_err(|e| {
            AppError::new(
                ErrorCode::InternalError,
                "GitHub returned data in an unexpected shape.",
            )
            .with_details(format!("{path}: {e}"))
        })?;
        Ok(ApiResponse {
            data,
            rate_limit: RateLimit::from_headers(&headers),
            scopes: headers
                .get("x-oauth-scopes")
                .and_then(|v| v.to_str().ok())
                .map(|s| {
                    s.split(',')
                        .map(str::trim)
                        .filter(|x| !x.is_empty())
                        .map(str::to_string)
                        .collect()
                }),
            has_next_page: next_link(&headers),
        })
    }

    pub async fn get<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> AppResult<ApiResponse<T>> {
        self.send(Method::GET, path, query).await
    }

    /// Follow `Link: rel="next"` pages, up to `max_pages`.
    pub async fn get_all<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, String)],
        max_pages: u32,
    ) -> AppResult<(Vec<T>, Option<RateLimit>, bool)> {
        let mut items = Vec::new();
        let mut rate = None;
        for page in 1..=max_pages {
            let mut q = query.to_vec();
            q.push(("per_page", "100".into()));
            q.push(("page", page.to_string()));
            let resp: ApiResponse<Vec<T>> = self.get(path, &q).await?;
            rate = resp.rate_limit.or(rate);
            items.extend(resp.data);
            if !resp.has_next_page {
                return Ok((items, rate, false));
            }
        }
        Ok((items, rate, true))
    }
}

// ---------------------------------------------------------------- response types

#[derive(Debug, Clone, Deserialize)]
pub struct User {
    pub id: i64,
    pub login: String,
    pub name: Option<String>,
    pub html_url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Org {
    pub id: i64,
    pub login: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Owner {
    pub login: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Repo {
    pub id: i64,
    pub name: String,
    pub full_name: String,
    pub owner: Owner,
    pub private: bool,
    #[serde(default)]
    pub archived: bool,
    #[serde(default)]
    pub fork: bool,
    pub default_branch: Option<String>,
    pub html_url: String,
    pub clone_url: Option<String>,
    pub ssh_url: Option<String>,
    pub description: Option<String>,
    pub language: Option<String>,
    pub pushed_at: Option<String>,
    pub updated_at: Option<String>,
    pub permissions: Option<Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SearchRepos {
    pub total_count: u64,
    pub items: Vec<Repo>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchCommit {
    pub sha: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Branch {
    pub name: String,
    pub commit: BranchCommit,
    #[serde(default)]
    pub protected: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PullRef {
    #[serde(rename = "ref")]
    pub ref_name: String,
    pub sha: String,
    pub repo: Option<RepoName>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RepoName {
    pub full_name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Pull {
    pub id: i64,
    pub number: i64,
    pub title: String,
    pub state: String,
    #[serde(default)]
    pub draft: bool,
    pub merged_at: Option<String>,
    pub user: Option<Owner>,
    pub html_url: String,
    pub head: PullRef,
    pub base: PullRef,
    pub mergeable: Option<bool>,
    pub mergeable_state: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CheckRun {
    pub status: String,
    pub conclusion: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CheckRuns {
    pub total_count: u64,
    pub check_runs: Vec<CheckRun>,
}

impl CheckRuns {
    /// Counts by outcome, for list views.
    pub fn summary(&self) -> Value {
        let mut success = 0;
        let mut failure = 0;
        let mut pending = 0;
        let mut neutral = 0;
        for run in &self.check_runs {
            match (run.status.as_str(), run.conclusion.as_deref()) {
                ("completed", Some("success")) => success += 1,
                (
                    "completed",
                    Some(
                        "failure" | "timed_out" | "cancelled" | "action_required"
                        | "startup_failure",
                    ),
                ) => failure += 1,
                ("completed", _) => neutral += 1,
                _ => pending += 1,
            }
        }
        json!({ "total": self.total_count, "success": success, "failure": failure, "pending": pending, "neutral": neutral })
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Contents {
    #[serde(rename = "type")]
    pub kind: String,
    pub encoding: Option<String>,
    pub content: Option<String>,
    pub size: u64,
    pub path: String,
    pub sha: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RateLimitResource {
    pub limit: u64,
    pub remaining: u64,
    pub reset: u64,
    pub used: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RateLimitResources {
    pub core: RateLimitResource,
    pub search: Option<RateLimitResource>,
    pub graphql: Option<RateLimitResource>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RateLimitResponse {
    pub resources: RateLimitResources,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_url_validation() {
        assert_eq!(
            validate_base("https://api.github.com", "x")
                .unwrap()
                .as_str(),
            "https://api.github.com/"
        );
        assert_eq!(
            validate_base("https://ghe.example.com/api/v3", "x")
                .unwrap()
                .as_str(),
            "https://ghe.example.com/api/v3/"
        );
        assert!(validate_base("http://127.0.0.1:9999", "x").is_ok());
        for bad in [
            "http://api.github.com",
            "https://user:pw@api.github.com",
            "https://api.github.com?x=1",
            "ftp://x",
            "nope",
        ] {
            assert_eq!(
                validate_base(bad, "x").unwrap_err().code,
                ErrorCode::GithubNotConfigured,
                "{bad}"
            );
        }
    }

    #[test]
    fn check_summary_counts() {
        let runs = CheckRuns {
            total_count: 4,
            check_runs: vec![
                CheckRun {
                    status: "completed".into(),
                    conclusion: Some("success".into()),
                },
                CheckRun {
                    status: "completed".into(),
                    conclusion: Some("failure".into()),
                },
                CheckRun {
                    status: "in_progress".into(),
                    conclusion: None,
                },
                CheckRun {
                    status: "completed".into(),
                    conclusion: Some("skipped".into()),
                },
            ],
        };
        assert_eq!(
            runs.summary(),
            json!({ "total": 4, "success": 1, "failure": 1, "pending": 1, "neutral": 1 })
        );
    }
}
