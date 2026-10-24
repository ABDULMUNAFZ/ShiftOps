//! Credential storage in the OS keychain (macOS Keychain, Windows Credential
//! Manager, Secret Service on Linux) via the `keyring` crate.
//!
//! Rules enforced here:
//! - Secret values are written to and read from the keychain only. SQLite
//!   holds metadata (`db::credentials`), never the value.
//! - Nothing in this module returns a secret to a Tauri command. `retrieve` is
//!   `pub(crate)` for native integration code only.
//! - Secrets are wrapped in `Secret`, which zeroes its memory on drop and
//!   never prints its contents.

use keyring::{Entry, Error as KeyringError};
use serde::Serialize;
use zeroize::Zeroize;

use crate::db::activity::{self, NewEvent, Severity};
use crate::db::credentials::{self as meta, CredentialKind, Provider, ValidationStatus};
use crate::db::Database;
use crate::error::{AppError, AppResult, ErrorCode};

pub const PRODUCTION_SERVICE: &str = "dev.shiftops.desktop";
const PROBE_ACCOUNT: &str = "shiftops-keychain-probe";
const MAX_SECRET_BYTES: usize = 8 * 1024;

/// Credential slots the UI may manage. OAuth tokens are written by the
/// device flow in a later step, so they are reported but not manually stored.
pub const SLOTS: &[(Provider, CredentialKind, bool)] = &[
    (
        Provider::Github,
        CredentialKind::GithubPersonalAccessToken,
        true,
    ),
    (Provider::Github, CredentialKind::GithubOauthToken, false),
    (Provider::Devin, CredentialKind::DevinApiToken, true),
];

pub struct Secret(String);

impl Secret {
    pub fn new(value: String) -> Self {
        Self(value)
    }

    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret([REDACTED])")
    }
}

/// Handle on one keychain service name. Production uses
/// `dev.shiftops.desktop`; tests use a unique `dev.shiftops.desktop.test.*`.
#[derive(Debug, Clone)]
pub struct CredentialStore {
    service: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeychainStatus {
    pub available: bool,
    pub backend: &'static str,
    pub service: String,
    pub error: Option<AppError>,
}

/// Everything the frontend may know about a credential.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialMetadata {
    pub provider: Provider,
    pub kind: CredentialKind,
    /// Keychain account name: `<provider>:<kind>`.
    pub identifier: String,
    pub exists: bool,
    pub manageable: bool,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub last_validated_at: Option<String>,
    pub last_validation_status: ValidationStatus,
}

pub fn backend_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "macOS Keychain"
    } else if cfg!(target_os = "windows") {
        "Windows Credential Manager"
    } else {
        "Secret Service (D-Bus)"
    }
}

fn map_err(code: ErrorCode, action: &str, e: KeyringError) -> AppError {
    let code = match &e {
        KeyringError::NoStorageAccess(_) | KeyringError::PlatformFailure(_) => {
            ErrorCode::CredentialStoreUnavailable
        }
        _ => code,
    };
    // keyring errors never contain the secret value; redact anyway.
    AppError::new(
        code,
        format!(
            "Could not {action} the credential in the {}.",
            backend_name()
        ),
    )
    .with_details(e.to_string())
}

pub fn account(provider: Provider, kind: CredentialKind) -> String {
    format!("{}:{}", provider.as_str(), kind.as_str())
}

impl CredentialStore {
    pub fn new(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
        }
    }

    pub fn production() -> Self {
        Self::new(PRODUCTION_SERVICE)
    }

    pub fn service(&self) -> &str {
        &self.service
    }

    fn entry(&self, account: &str) -> AppResult<Entry> {
        Entry::new(&self.service, account)
            .map_err(|e| map_err(ErrorCode::CredentialStoreUnavailable, "access", e))
    }

    /// Write, read back and delete a throwaway entry.
    pub fn probe(&self) -> KeychainStatus {
        let result = (|| -> AppResult<()> {
            let entry = self.entry(PROBE_ACCOUNT)?;
            let nonce = format!("probe-{}", uuid::Uuid::new_v4());
            entry
                .set_password(&nonce)
                .map_err(|e| map_err(ErrorCode::CredentialWriteFailed, "write", e))?;
            let read = entry
                .get_password()
                .map_err(|e| map_err(ErrorCode::CredentialReadFailed, "read", e));
            let _ = entry.delete_credential();
            if read? != nonce {
                return Err(AppError::new(
                    ErrorCode::CredentialReadFailed,
                    "The keychain returned a different value than was written.",
                ));
            }
            Ok(())
        })();
        KeychainStatus {
            available: result.is_ok(),
            backend: backend_name(),
            service: self.service.clone(),
            error: result.err(),
        }
    }

    /// Store or replace a secret. Verifies the write by reading it back.
    pub fn put(&self, provider: Provider, kind: CredentialKind, secret: &Secret) -> AppResult<()> {
        let entry = self.entry(&account(provider, kind))?;
        entry
            .set_password(secret.expose())
            .map_err(|e| map_err(ErrorCode::CredentialWriteFailed, "store", e))?;
        let mut stored = entry
            .get_password()
            .map_err(|e| map_err(ErrorCode::CredentialReadFailed, "verify", e))?;
        let matches = stored == secret.expose();
        stored.zeroize();
        if !matches {
            return Err(AppError::new(
                ErrorCode::CredentialWriteFailed,
                "The keychain did not store the credential correctly.",
            ));
        }
        Ok(())
    }

    pub fn exists(&self, provider: Provider, kind: CredentialKind) -> AppResult<bool> {
        match self.entry(&account(provider, kind))?.get_password() {
            Ok(mut value) => {
                value.zeroize();
                Ok(true)
            }
            Err(KeyringError::NoEntry) => Ok(false),
            Err(e) => Err(map_err(ErrorCode::CredentialReadFailed, "read", e)),
        }
    }

    /// Native integration code only. Never return the result to the frontend.
    #[allow(dead_code)] // First used by the GitHub and Devin integrations.
    pub(crate) fn retrieve(&self, provider: Provider, kind: CredentialKind) -> AppResult<Secret> {
        match self.entry(&account(provider, kind))?.get_password() {
            Ok(value) => Ok(Secret::new(value)),
            Err(KeyringError::NoEntry) => Err(AppError::new(
                ErrorCode::RecordNotFound,
                "No credential is stored.",
            )),
            Err(e) => Err(map_err(ErrorCode::CredentialReadFailed, "read", e)),
        }
    }

    /// Returns whether a credential existed.
    pub fn remove(&self, provider: Provider, kind: CredentialKind) -> AppResult<bool> {
        match self.entry(&account(provider, kind))?.delete_credential() {
            Ok(()) => Ok(true),
            Err(KeyringError::NoEntry) => Ok(false),
            Err(e) => Err(map_err(ErrorCode::CredentialWriteFailed, "delete", e)),
        }
    }
}

fn slot(provider: Provider, kind: CredentialKind) -> AppResult<(Provider, CredentialKind, bool)> {
    SLOTS
        .iter()
        .copied()
        .find(|(p, k, _)| *p == provider && *k == kind)
        .ok_or_else(|| {
            AppError::validation(format!(
                "{} credentials of kind {} are not supported.",
                provider.as_str(),
                kind.as_str()
            ))
        })
}

fn validate_secret(value: &str) -> AppResult<()> {
    if value.trim().is_empty() {
        return Err(AppError::validation("The credential is empty."));
    }
    if value.len() > MAX_SECRET_BYTES {
        return Err(AppError::validation("The credential is too long."));
    }
    if value.chars().any(char::is_control) || value != value.trim() {
        return Err(AppError::validation(
            "The credential contains whitespace or control characters. Paste only the token.",
        ));
    }
    Ok(())
}

pub fn metadata(
    db: &Database,
    store: &CredentialStore,
    provider: Provider,
    kind: CredentialKind,
) -> AppResult<CredentialMetadata> {
    let (_, _, manageable) = slot(provider, kind)?;
    let exists = store.exists(provider, kind)?;
    let record = db.read(|c| meta::get(c, provider, kind))?;
    // Metadata without a keychain entry (removed outside ShiftOps) reports
    // the truth from the keychain and clears the stale timestamps.
    let record = record.filter(|_| exists);
    Ok(CredentialMetadata {
        provider,
        kind,
        identifier: account(provider, kind),
        exists,
        manageable,
        created_at: record.as_ref().map(|r| r.created_at.clone()),
        updated_at: record.as_ref().map(|r| r.updated_at.clone()),
        last_validated_at: record.as_ref().and_then(|r| r.last_validated_at.clone()),
        last_validation_status: record
            .map_or(ValidationStatus::NotValidated, |r| r.last_validation_status),
    })
}

pub fn list(db: &Database, store: &CredentialStore) -> AppResult<Vec<CredentialMetadata>> {
    SLOTS
        .iter()
        .map(|(p, k, _)| metadata(db, store, *p, *k))
        .collect()
}

/// Store or replace a credential. The secret is consumed and zeroed.
pub fn store_credential(
    db: &Database,
    store: &CredentialStore,
    provider: Provider,
    kind: CredentialKind,
    secret: Secret,
) -> AppResult<CredentialMetadata> {
    let (_, _, manageable) = slot(provider, kind)?;
    if !manageable {
        return Err(AppError::validation(
            "This credential is created by its sign-in flow and cannot be entered manually.",
        ));
    }
    validate_secret(secret.expose())?;
    store.put(provider, kind, &secret)?;
    drop(secret);
    let identifier = account(provider, kind);
    let replaced = db.write(|c| {
        let replaced = meta::upsert(c, provider, kind, &identifier)?;
        activity::record(
            c,
            NewEvent::new(
                "credential",
                if replaced { "credential.replaced" } else { "credential.stored" },
                Severity::Notice,
                format!(
                    "{} {} {} in the {}",
                    provider.as_str(),
                    kind.as_str().replace('_', " "),
                    if replaced { "replaced" } else { "stored" },
                    backend_name()
                ),
            )
            .entity(identifier.clone())
            .metadata(serde_json::json!({ "provider": provider.as_str(), "kind": kind.as_str(), "backend": backend_name() })),
        )?;
        Ok(replaced)
    })?;
    crate::logging::info(
        "credentials",
        &format!(
            "{identifier} {}",
            if replaced { "replaced" } else { "stored" }
        ),
    );
    metadata(db, store, provider, kind)
}

pub fn delete_credential(
    db: &Database,
    store: &CredentialStore,
    provider: Provider,
    kind: CredentialKind,
) -> AppResult<CredentialMetadata> {
    slot(provider, kind)?;
    let existed = store.remove(provider, kind)?;
    let identifier = account(provider, kind);
    db.write(|c| {
        meta::remove(c, provider, kind)?;
        if existed {
            activity::record(
                c,
                NewEvent::new(
                    "credential",
                    "credential.deleted",
                    Severity::Notice,
                    format!(
                        "{} {} deleted from the {}",
                        provider.as_str(),
                        kind.as_str().replace('_', " "),
                        backend_name()
                    ),
                )
                .entity(identifier.clone())
                .metadata(
                    serde_json::json!({ "provider": provider.as_str(), "kind": kind.as_str() }),
                ),
            )?;
        }
        Ok(())
    })?;
    if existed {
        crate::logging::info("credentials", &format!("{identifier} deleted"));
    }
    metadata(db, store, provider, kind)
}

#[cfg(test)]
pub(crate) mod tests {
    //! These tests use the real OS keychain under a unique test service name
    //! and never touch `dev.shiftops.desktop`. A guard deletes every test
    //! entry on drop, so cleanup runs even when an assertion panics.

    use super::*;

    pub struct TestKeychain {
        pub store: CredentialStore,
    }

    impl TestKeychain {
        /// `None` (with the reason printed) when the platform has no usable
        /// keychain, e.g. a headless Linux CI runner without Secret Service.
        pub fn new() -> Option<Self> {
            let store = CredentialStore::new(format!(
                "dev.shiftops.desktop.test.{}",
                uuid::Uuid::new_v4()
            ));
            let status = store.probe();
            if !status.available {
                eprintln!(
                    "SKIPPED keychain test: {} unavailable: {:?}",
                    status.backend,
                    status.error.map(|e| e.technical_details)
                );
                return None;
            }
            Some(Self { store })
        }
    }

    impl Drop for TestKeychain {
        fn drop(&mut self) {
            for (p, k, _) in SLOTS {
                let _ = self.store.remove(*p, *k);
            }
        }
    }

    const SECRET: &str = "ghp_TestOnlyShiftOpsSecret0123456789abcd";

    #[test]
    fn store_check_replace_delete_against_real_keychain() {
        let Some(kc) = TestKeychain::new() else {
            return;
        };
        let db = Database::open_in_memory();
        let p = Provider::Github;
        let k = CredentialKind::GithubPersonalAccessToken;

        let before = metadata(&db, &kc.store, p, k).unwrap();
        assert!(!before.exists);

        let stored = store_credential(&db, &kc.store, p, k, Secret::new(SECRET.into())).unwrap();
        assert!(stored.exists);
        assert_eq!(stored.identifier, "github:github_personal_access_token");
        assert!(stored.created_at.is_some());
        assert!(!serde_json::to_string(&stored).unwrap().contains(SECRET));
        assert_eq!(kc.store.retrieve(p, k).unwrap().expose(), SECRET);

        let replaced =
            store_credential(&db, &kc.store, p, k, Secret::new(format!("{SECRET}x"))).unwrap();
        assert_eq!(replaced.created_at, stored.created_at);
        assert_eq!(
            kc.store.retrieve(p, k).unwrap().expose(),
            format!("{SECRET}x")
        );

        let deleted = delete_credential(&db, &kc.store, p, k).unwrap();
        assert!(!deleted.exists);
        assert!(deleted.created_at.is_none());
        assert!(!kc.store.exists(p, k).unwrap());
        // Deleting again is harmless.
        assert!(!delete_credential(&db, &kc.store, p, k).unwrap().exists);

        // Activity records the events without the value.
        let events = db.read(|c| activity::list(c, &Default::default())).unwrap();
        let types: Vec<_> = events.items.iter().map(|e| e.event_type.as_str()).collect();
        assert_eq!(
            types,
            [
                "credential.deleted",
                "credential.replaced",
                "credential.stored"
            ]
        );
        assert!(!serde_json::to_string(&events)
            .unwrap()
            .contains("TestOnlyShiftOps"));
    }

    #[test]
    fn rejects_invalid_secrets_and_unmanageable_slots_before_touching_keychain() {
        let db = Database::open_in_memory();
        let store = CredentialStore::new("dev.shiftops.desktop.test.never-written");
        for bad in ["", "   ", " padded", "line\nbreak", "tab\tchar"] {
            let err = store_credential(
                &db,
                &store,
                Provider::Devin,
                CredentialKind::DevinApiToken,
                Secret::new(bad.into()),
            )
            .unwrap_err();
            assert_eq!(err.code, ErrorCode::ValidationFailed, "{bad:?}");
        }
        let oauth = store_credential(
            &db,
            &store,
            Provider::Github,
            CredentialKind::GithubOauthToken,
            Secret::new("x".repeat(40)),
        );
        assert_eq!(oauth.unwrap_err().code, ErrorCode::ValidationFailed);
        let mismatched = store_credential(
            &db,
            &store,
            Provider::Devin,
            CredentialKind::GithubPersonalAccessToken,
            Secret::new("x".repeat(40)),
        );
        assert_eq!(mismatched.unwrap_err().code, ErrorCode::ValidationFailed);
    }

    #[test]
    fn secret_debug_output_is_redacted() {
        assert_eq!(
            format!("{:?}", Secret::new(SECRET.into())),
            "Secret([REDACTED])"
        );
    }
}
