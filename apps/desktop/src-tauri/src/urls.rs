//! External URL validation. Every URL ShiftOps opens in the system browser
//! goes through `validate_external_url` first.
//!
//! Allowed:
//! - `https:` URLs with a host and no embedded credentials. This includes
//!   GitHub's device verification page (`https://github.com/login/device`).
//! - `http:` only for loopback hosts (`localhost`, `127.0.0.1`, `[::1]`), for
//!   local development servers such as the website on port 3000.
//!
//! Everything else (`file:`, `javascript:`, `data:`, custom and shell-like
//! schemes, malformed input) is rejected with `EXTERNAL_URL_REJECTED`.

use url::{Host, Url};

use crate::error::{AppError, AppResult, ErrorCode};

const MAX_URL_LEN: usize = 2048;

fn reject(reason: &str, input: &str) -> AppError {
    let shown: String = input.chars().take(200).collect();
    AppError::new(
        ErrorCode::ExternalUrlRejected,
        format!("The link was not opened: {reason}."),
    )
    .with_context("url", shown)
}

pub fn validate_external_url(input: &str) -> AppResult<Url> {
    if input.is_empty() || input.len() > MAX_URL_LEN {
        return Err(reject("the URL is empty or too long", input));
    }
    if input.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err(reject(
            "the URL contains whitespace or control characters",
            input,
        ));
    }
    // Require a literal `scheme://host`. WHATWG parsing would otherwise
    // "repair" inputs such as `https:///host` or `https:\\host`.
    let authority_ok = input
        .split_once(':')
        .and_then(|(_, rest)| rest.strip_prefix("//"))
        .is_some_and(|rest| !rest.is_empty() && !rest.starts_with(['/', '\\']));
    if !authority_ok {
        return Err(reject("the URL is malformed", input));
    }
    let url = Url::parse(input).map_err(|_| reject("the URL is malformed", input))?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err(reject(
            "URLs with embedded credentials are not allowed",
            input,
        ));
    }
    let host = url
        .host()
        .ok_or_else(|| reject("the URL has no host", input))?;
    match url.scheme() {
        "https" => Ok(url),
        "http" if is_loopback(&host) => Ok(url),
        "http" => Err(reject("plain http is only allowed for localhost", input)),
        _ => Err(reject("only https links can be opened", input)),
    }
}

fn is_loopback(host: &Host<&str>) -> bool {
    match host {
        Host::Domain(d) => d.eq_ignore_ascii_case("localhost"),
        Host::Ipv4(ip) => ip.is_loopback(),
        Host::Ipv6(ip) => ip.is_loopback(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_https_and_github_device_flow() {
        assert!(
            validate_external_url("https://github.com/settings/personal-access-tokens").is_ok()
        );
        assert!(validate_external_url("https://github.com/login/device").is_ok());
        assert!(validate_external_url("http://localhost:3000/docs/").is_ok());
        assert!(validate_external_url("http://127.0.0.1:1420").is_ok());
    }

    #[test]
    fn rejects_dangerous_and_malformed_urls() {
        for url in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "data:text/html,<script>alert(1)</script>",
            "vbscript:msgbox",
            "ssh://github.com/acme/api",
            "x-shell://run?cmd=rm",
            "http://example.com",
            "https://user:pass@github.com",
            "https://github.com/a b",
            "not a url",
            "",
            "https:///nohost",
            "https:\\\\evil.example",
        ] {
            let err = validate_external_url(url).unwrap_err();
            assert_eq!(err.code, ErrorCode::ExternalUrlRejected, "{url}");
        }
    }
}
