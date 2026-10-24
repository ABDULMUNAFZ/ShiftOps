//! The single structured error contract shared by Rust and TypeScript.
//!
//! Every Tauri command returns `AppResult<T>`. Errors serialize as
//! `{ code, message, technicalDetails, suggestedAction, recoverable, context }`
//! and are validated by `appErrorSchema` in `packages/core`. Technical details
//! and context values pass through redaction before they leave this module.

use serde::Serialize;
use serde_json::{Map, Value};

use crate::logging;
use crate::redact::redact;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    DatabaseUnavailable,
    DatabaseMigrationFailed,
    RecordNotFound,
    ValidationFailed,
    CredentialStoreUnavailable,
    CredentialWriteFailed,
    CredentialReadFailed,
    ExecutableNotFound,
    CommandNotAllowed,
    CommandTimedOut,
    CommandCancelled,
    CommandFailed,
    OutputLimitExceeded,
    PathNotAllowed,
    ExternalUrlRejected,
    NetworkFailed,
    GithubNotConfigured,
    GithubAuthenticationFailed,
    GithubPermissionDenied,
    GithubRateLimited,
    GithubAuthorizationExpired,
    GithubAuthorizationDenied,
    GitAuthenticationRequired,
    DevinNotConfigured,
    InternalError,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DatabaseUnavailable => "DATABASE_UNAVAILABLE",
            Self::DatabaseMigrationFailed => "DATABASE_MIGRATION_FAILED",
            Self::RecordNotFound => "RECORD_NOT_FOUND",
            Self::ValidationFailed => "VALIDATION_FAILED",
            Self::CredentialStoreUnavailable => "CREDENTIAL_STORE_UNAVAILABLE",
            Self::CredentialWriteFailed => "CREDENTIAL_WRITE_FAILED",
            Self::CredentialReadFailed => "CREDENTIAL_READ_FAILED",
            Self::ExecutableNotFound => "EXECUTABLE_NOT_FOUND",
            Self::CommandNotAllowed => "COMMAND_NOT_ALLOWED",
            Self::CommandTimedOut => "COMMAND_TIMED_OUT",
            Self::CommandCancelled => "COMMAND_CANCELLED",
            Self::CommandFailed => "COMMAND_FAILED",
            Self::OutputLimitExceeded => "OUTPUT_LIMIT_EXCEEDED",
            Self::PathNotAllowed => "PATH_NOT_ALLOWED",
            Self::ExternalUrlRejected => "EXTERNAL_URL_REJECTED",
            Self::NetworkFailed => "NETWORK_FAILED",
            Self::GithubNotConfigured => "GITHUB_NOT_CONFIGURED",
            Self::GithubAuthenticationFailed => "GITHUB_AUTHENTICATION_FAILED",
            Self::GithubPermissionDenied => "GITHUB_PERMISSION_DENIED",
            Self::GithubRateLimited => "GITHUB_RATE_LIMITED",
            Self::GithubAuthorizationExpired => "GITHUB_AUTHORIZATION_EXPIRED",
            Self::GithubAuthorizationDenied => "GITHUB_AUTHORIZATION_DENIED",
            Self::GitAuthenticationRequired => "GIT_AUTHENTICATION_REQUIRED",
            Self::DevinNotConfigured => "DEVIN_NOT_CONFIGURED",
            Self::InternalError => "INTERNAL_ERROR",
        }
    }

    /// Whether the user can reasonably retry or correct the input.
    fn default_recoverable(self) -> bool {
        !matches!(
            self,
            Self::DatabaseMigrationFailed | Self::InternalError | Self::DatabaseUnavailable
        )
    }

    fn default_action(self) -> Option<&'static str> {
        Some(match self {
            Self::DatabaseUnavailable => "Restart ShiftOps. If the problem persists, check that the application data directory is writable.",
            Self::DatabaseMigrationFailed => "Your data was not modified. A backup was written before migrating; see the log directory and report this error.",
            Self::RecordNotFound => "Refresh the view; the record may have been removed.",
            Self::ValidationFailed => "Correct the highlighted input and try again.",
            Self::CredentialStoreUnavailable => "Unlock or enable the operating system keychain, then try again.",
            Self::CredentialWriteFailed => "Check that the keychain is unlocked and that ShiftOps is allowed to access it.",
            Self::CredentialReadFailed => "Check that the keychain is unlocked. You may need to store the credential again.",
            Self::ExecutableNotFound => "Install the tool or make sure it is on your login shell PATH, then re-run environment checks.",
            Self::CommandNotAllowed => "This command category is not enabled by the current execution policy.",
            Self::CommandTimedOut => "Increase the execution timeout in Settings or investigate why the command hangs.",
            Self::CommandCancelled => return None,
            Self::CommandFailed => "Inspect the command output for the cause.",
            Self::OutputLimitExceeded => "Increase the output-size limit in Settings if the full output is needed.",
            Self::PathNotAllowed => "Choose a location inside an allowed directory.",
            Self::ExternalUrlRejected => "Only https links (and http to localhost) can be opened.",
            Self::NetworkFailed => "Check your internet connection or proxy, then try again.",
            Self::GithubNotConfigured => "Set VITE_GITHUB_OAUTH_CLIENT_ID in apps/desktop/.env and rebuild, or connect with a personal access token.",
            Self::GithubAuthenticationFailed => "Reconnect GitHub: the stored credential was rejected or has expired.",
            Self::GithubPermissionDenied => "Grant the token access to this repository or organisation, then try again.",
            Self::GithubRateLimited => "Wait until the rate limit resets, then try again.",
            Self::GithubAuthorizationExpired => "The code expired before it was approved. Start the connection again.",
            Self::GithubAuthorizationDenied => "Authorization was declined on GitHub. Start again if this was a mistake.",
            Self::GitAuthenticationRequired => "Run `gh auth login` and then `gh auth setup-git` (or configure another Git credential helper for this host), then try again. ShiftOps never passes its stored token to Git.",
            Self::DevinNotConfigured => "Install the Devin CLI and run `devin auth login`. The CLI keeps its own credentials; ShiftOps never stores them.",
            Self::InternalError => "Restart ShiftOps. If the problem persists, report it with the copied details.",
        })
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppError {
    pub code: ErrorCode,
    pub message: String,
    pub technical_details: Option<String>,
    pub suggested_action: Option<String>,
    pub recoverable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<Map<String, Value>>,
}

impl AppError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: redact(&message.into()),
            technical_details: None,
            suggested_action: code.default_action().map(str::to_string),
            recoverable: code.default_recoverable(),
            context: None,
        }
    }

    pub fn validation(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::ValidationFailed, message)
    }

    pub fn not_found(entity: &str, id: &str) -> Self {
        Self::new(
            ErrorCode::RecordNotFound,
            format!("{entity} {id} was not found."),
        )
        .with_context("entity", entity)
        .with_context("id", id)
    }

    pub fn internal(details: impl std::fmt::Display) -> Self {
        let err = Self::new(
            ErrorCode::InternalError,
            "An unexpected internal error occurred.",
        )
        .with_details(details.to_string());
        err.log();
        err
    }

    pub fn with_details(mut self, details: impl Into<String>) -> Self {
        self.technical_details = Some(redact(&details.into()));
        self
    }

    pub fn with_action(mut self, action: impl Into<String>) -> Self {
        self.suggested_action = Some(action.into());
        self
    }

    pub fn with_context(mut self, key: &str, value: impl Into<Value>) -> Self {
        let value = match value.into() {
            Value::String(s) => Value::String(redact(&s)),
            other => other,
        };
        self.context
            .get_or_insert_with(Map::new)
            .insert(key.to_string(), value);
        self
    }

    /// Write this error to the redacted application log.
    pub fn log(&self) {
        logging::error(
            "app_error",
            &format!(
                "{} {}{}",
                self.code.as_str(),
                self.message,
                self.technical_details
                    .as_deref()
                    .map(|d| format!(" | {d}"))
                    .unwrap_or_default()
            ),
        );
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code.as_str(), self.message)
    }
}

impl std::error::Error for AppError {}

impl From<rusqlite::Error> for AppError {
    fn from(err: rusqlite::Error) -> Self {
        use rusqlite::ErrorCode as Sql;
        match &err {
            rusqlite::Error::QueryReturnedNoRows => Self::new(
                ErrorCode::RecordNotFound,
                "The requested record was not found.",
            ),
            rusqlite::Error::SqliteFailure(e, msg) if e.code == Sql::ConstraintViolation => {
                let detail = msg.clone().unwrap_or_else(|| err.to_string());
                let message = if detail.contains("FOREIGN KEY") {
                    "The change references a record that does not exist or is still in use."
                } else if detail.contains("UNIQUE") {
                    "A record with the same identity already exists."
                } else {
                    "The change violates a data integrity rule."
                };
                Self::new(ErrorCode::ValidationFailed, message).with_details(detail)
            }
            _ => {
                let e = Self::new(
                    ErrorCode::DatabaseUnavailable,
                    "The local database could not complete the operation.",
                )
                .with_details(err.to_string());
                e.log();
                e
            }
        }
    }
}

impl From<serde_json::Error> for AppError {
    fn from(err: serde_json::Error) -> Self {
        Self::validation("Invalid JSON value.").with_details(err.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_to_the_shared_contract() {
        let err = AppError::new(ErrorCode::RecordNotFound, "missing")
            .with_details("token=ghp_abcdefghijklmnopqrstuvwxyz0123456789")
            .with_context("id", "abc");
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(json["code"], "RECORD_NOT_FOUND");
        assert_eq!(json["recoverable"], true);
        assert!(json["suggestedAction"].is_string());
        assert_eq!(json["context"]["id"], "abc");
        let details = json["technicalDetails"].as_str().unwrap();
        assert!(!details.contains("ghp_abc"), "{details}");
    }

    #[test]
    fn maps_sqlite_errors_to_stable_codes() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT UNIQUE); INSERT INTO t (v) VALUES ('a');")
            .unwrap();
        let unique: AppError = conn
            .execute("INSERT INTO t (v) VALUES ('a')", [])
            .unwrap_err()
            .into();
        assert_eq!(unique.code, ErrorCode::ValidationFailed);
        let missing: AppError = conn
            .query_row("SELECT v FROM t WHERE id = 99", [], |r| {
                r.get::<_, String>(0)
            })
            .unwrap_err()
            .into();
        assert_eq!(missing.code, ErrorCode::RecordNotFound);
    }
}
