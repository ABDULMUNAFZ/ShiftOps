//! Secret redaction. Applied to every string that is logged, persisted as
//! command output, placed in an error's technical details or streamed to the
//! web view. Patterns err on the side of over-redaction.

use std::sync::LazyLock;

use regex::Regex;

pub const MASK: &str = "[REDACTED]";

struct Rule {
    pattern: Regex,
    replacement: &'static str,
}

static RULES: LazyLock<Vec<Rule>> = LazyLock::new(|| {
    let rule = |pattern: &str, replacement: &'static str| Rule {
        pattern: Regex::new(pattern).expect("valid redaction pattern"),
        replacement,
    };
    vec![
        // Authorization / Proxy-Authorization headers: mask everything after the separator.
        rule(
            r"(?im)\b((?:proxy-)?authorization)(\s*[:=]\s*)[^\r\n]+",
            "${1}${2}[REDACTED]",
        ),
        // Bearer / token schemes outside headers.
        rule(
            r"(?i)\b(bearer|token)\s+[A-Za-z0-9\-._~+/]{8,}=*",
            "${1} [REDACTED]",
        ),
        // Credentials embedded in URLs: scheme://user:password@host or scheme://token@host.
        rule(r"(?i)\b([a-z][a-z0-9+.\-]*://)[^/\s@]+@", "${1}[REDACTED]@"),
        // GitHub token formats.
        rule(r"\bgh[pousr]_[A-Za-z0-9]{20,}\b", "[REDACTED_GITHUB_TOKEN]"),
        rule(r"\bgithub_pat_[A-Za-z0-9_]{20,}", "[REDACTED_GITHUB_TOKEN]"),
        // Generic key/value secrets in env, JSON, YAML, query strings and CLI flags.
        rule(
            r#"(?i)(["']?[A-Za-z0-9_\-]*(?:api[_\-]?key|apikey|access[_\-]?token|auth[_\-]?token|refresh[_\-]?token|client[_\-]?secret|secret[_\-]?key|private[_\-]?key|password|passwd|secret|token)["']?\s*(?:[:=]|\s--?>\s)\s*["']?)([^\s"',;&]{3,})"#,
            "${1}[REDACTED]",
        ),
    ]
});

/// Redact known secret formats from `input`.
pub fn redact(input: &str) -> String {
    let mut out = std::borrow::Cow::Borrowed(input);
    for rule in RULES.iter() {
        if rule.pattern.is_match(&out) {
            out = std::borrow::Cow::Owned(
                rule.pattern
                    .replace_all(&out, rule.replacement)
                    .into_owned(),
            );
        }
    }
    out.into_owned()
}

/// Redacts pattern matches plus specific secret values known to the caller
/// (for example a token passed to a child process environment).
#[derive(Clone, Default)]
pub struct Redactor {
    literals: Vec<String>,
}

impl Redactor {
    pub fn with_secrets(secrets: impl IntoIterator<Item = String>) -> Self {
        let mut literals: Vec<String> = secrets.into_iter().filter(|s| s.len() >= 4).collect();
        // Longest first so a secret containing another is masked whole.
        literals.sort_by_key(|s| std::cmp::Reverse(s.len()));
        Self { literals }
    }

    pub fn redact(&self, input: &str) -> String {
        let mut out = input.to_string();
        for literal in &self.literals {
            if out.contains(literal.as_str()) {
                out = out.replace(literal.as_str(), MASK);
            }
        }
        redact(&out)
    }
}

impl std::fmt::Debug for Redactor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Redactor({} literals)", self.literals.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_redacted(input: &str, secret: &str) {
        let out = redact(input);
        assert!(!out.contains(secret), "secret survived: {out}");
        assert!(out.contains("REDACTED"), "no marker: {out}");
    }

    #[test]
    fn github_token_formats() {
        assert_redacted(
            "token ghp_0123456789abcdefghijABCDEFGHIJ0123 used",
            "ghp_0123456789",
        );
        assert_redacted("gho_abcdefghijklmnopqrstuvwxyz012345", "gho_abcdefghij");
        assert_redacted("ghs_abcdefghijklmnopqrstuvwxyz012345", "ghs_abcdefghij");
        assert_redacted(
            "github_pat_11ABCDEFG0123456789_abcdefghijklmnopqrstuvwxyzABCDEF",
            "github_pat_11ABCDEFG",
        );
    }

    #[test]
    fn bearer_and_authorization_headers() {
        assert_redacted("Authorization: Bearer abc.def.ghi-123456", "abc.def.ghi");
        assert_redacted(
            "curl -H 'authorization: token s3cr3tvalue99'",
            "s3cr3tvalue99",
        );
        assert_redacted("sending bearer eyJhbGciOiJIUzI1NiJ9.payload", "eyJhbGci");
        assert_redacted("Proxy-Authorization: Basic dXNlcjpwYXNz", "dXNlcjpwYXNz");
    }

    #[test]
    fn generic_api_key_fields() {
        assert_redacted(r#"{"api_key": "sk-live-123456789"}"#, "sk-live-123456789");
        assert_redacted("DEVIN_API_KEY=apk_user_abcdef123", "apk_user_abcdef123");
        assert_redacted("password=hunter22", "hunter22");
        assert_redacted("client_secret: abcdef0123", "abcdef0123");
        assert_redacted("https://x.test/cb?access_token=zzzz9999&ok=1", "zzzz9999");
    }

    #[test]
    fn url_credentials() {
        assert_redacted("https://user:pa55word@github.com/acme/api.git", "pa55word");
        assert_redacted(
            "https://x-access-token:ghs_abc@github.com/acme/api",
            "ghs_abc",
        );
        assert_redacted(
            "fetching https://abcd1234token@example.com/repo",
            "abcd1234token",
        );
    }

    #[test]
    fn leaves_ordinary_output_alone() {
        let text = "git version 2.50.1 (Apple Git-155)\nDone in 3.2s";
        assert_eq!(redact(text), text);
        assert_eq!(
            redact("https://github.com/acme/api"),
            "https://github.com/acme/api"
        );
    }

    #[test]
    fn literal_secrets() {
        let r = Redactor::with_secrets(["plain-secret-value".to_string(), "ab".to_string()]);
        assert_eq!(r.redact("x plain-secret-value y ab"), "x [REDACTED] y ab");
    }
}
