//! OS keychain access. Secrets are only ever read on the native side; the
//! frontend can learn whether a credential exists, never its value.

use keyring::{Entry, Error as KeyringError};

pub const SERVICE: &str = "dev.shiftops.desktop";
const PROBE_ACCOUNT: &str = "shiftops-keychain-probe";

pub const PROVIDERS: &[&str] = &["github", "devin"];

/// Write, read back and delete a throwaway entry to prove the platform
/// credential store is usable. Returns the backend name on success.
pub fn probe() -> Result<String, String> {
    let entry = Entry::new(SERVICE, PROBE_ACCOUNT).map_err(describe)?;
    let nonce = format!("probe-{}", std::process::id());
    entry.set_password(&nonce).map_err(describe)?;
    let read = entry.get_password().map_err(describe);
    let cleanup = entry.delete_credential().map_err(describe);
    match read {
        Ok(value) if value == nonce => cleanup.map(|_| backend_name().to_string()),
        Ok(_) => Err("keychain returned a different value than was written".into()),
        Err(e) => Err(e),
    }
}

pub fn credential_present(provider: &str) -> Result<bool, String> {
    let entry = Entry::new(SERVICE, provider).map_err(describe)?;
    match entry.get_password() {
        Ok(_) => Ok(true),
        Err(KeyringError::NoEntry) => Ok(false),
        Err(e) => Err(describe(e)),
    }
}

fn backend_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "macOS Keychain"
    } else if cfg!(target_os = "windows") {
        "Windows Credential Manager"
    } else {
        "Secret Service (D-Bus)"
    }
}

fn describe(e: KeyringError) -> String {
    match e {
        KeyringError::PlatformFailure(inner) => format!("platform failure: {inner}"),
        KeyringError::NoStorageAccess(inner) => format!("no storage access: {inner}"),
        other => other.to_string(),
    }
}
