//! Git remote URL parsing. Mirrors `parseGitRemote` in `packages/github`.
//! Credentials embedded in a remote URL are removed before anything is
//! stored or displayed.

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteInfo {
    /// The remote URL with any userinfo removed.
    pub sanitized_url: String,
    pub host: Option<String>,
    pub owner: Option<String>,
    pub repo: Option<String>,
}

impl RemoteInfo {
    /// GitHub.com and GitHub Enterprise Cloud (`*.ghe.com`) hosts.
    pub fn is_github(&self) -> bool {
        self.host.as_deref().is_some_and(|h| {
            h.eq_ignore_ascii_case("github.com") || h.to_ascii_lowercase().ends_with(".ghe.com")
        }) && self.owner.is_some()
            && self.repo.is_some()
    }
}

fn split_owner_repo(path: &str) -> (Option<String>, Option<String>) {
    let trimmed = path.trim_matches('/');
    let trimmed = trimmed.strip_suffix(".git").unwrap_or(trimmed);
    let mut parts = trimmed.split('/');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(owner), Some(repo), None) if !owner.is_empty() && !repo.is_empty() => {
            (Some(owner.to_string()), Some(repo.to_string()))
        }
        _ => (None, None),
    }
}

/// Parse `https://host/owner/repo(.git)`, `ssh://[user@]host[:port]/owner/repo(.git)`,
/// `git://host/owner/repo`, and SCP-style `user@host:owner/repo(.git)`.
/// Unrecognised remotes keep their sanitized URL with no host/owner/repo.
pub fn parse_remote(raw: &str) -> RemoteInfo {
    let raw = raw.trim();
    if let Some((scheme, rest)) = raw.split_once("://") {
        let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
        let host_port = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
        let host = host_port
            .split(':')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        let sanitized = format!("{scheme}://{host_port}/{path}");
        let (owner, repo) = if host.is_empty() || scheme.eq_ignore_ascii_case("file") {
            (None, None)
        } else {
            split_owner_repo(path)
        };
        return RemoteInfo {
            sanitized_url: sanitized,
            host: (!host.is_empty() && !scheme.eq_ignore_ascii_case("file")).then_some(host),
            owner,
            repo,
        };
    }
    // SCP-like: [user@]host:path, where host has no slash before the colon.
    if let Some((left, path)) = raw.split_once(':') {
        if !left.contains('/') && !path.starts_with("//") && !left.is_empty() {
            let host = left
                .rsplit_once('@')
                .map_or(left, |(_, h)| h)
                .to_ascii_lowercase();
            // A single letter before the colon is a Windows drive, not a host.
            if host.len() > 1 {
                let user = left.rsplit_once('@').map(|(u, _)| u);
                let (owner, repo) = split_owner_repo(path);
                // Keep the conventional `git@` user; it is not a credential.
                let sanitized = match user {
                    Some("git") => format!("git@{host}:{path}"),
                    _ => format!("{host}:{path}"),
                };
                return RemoteInfo {
                    sanitized_url: sanitized,
                    host: Some(host),
                    owner,
                    repo,
                };
            }
        }
    }
    RemoteInfo {
        sanitized_url: raw.to_string(),
        host: None,
        owner: None,
        repo: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gh(url: &str) -> (String, String, String) {
        let r = parse_remote(url);
        assert!(r.is_github(), "{url} -> {r:?}");
        (r.host.unwrap(), r.owner.unwrap(), r.repo.unwrap())
    }

    #[test]
    fn parses_github_remote_forms() {
        let expected = (
            "github.com".to_string(),
            "acme".to_string(),
            "api".to_string(),
        );
        assert_eq!(gh("https://github.com/acme/api.git"), expected);
        assert_eq!(gh("https://github.com/acme/api"), expected);
        assert_eq!(gh("git@github.com:acme/api.git"), expected);
        assert_eq!(gh("ssh://git@github.com/acme/api.git"), expected);
        assert_eq!(gh("ssh://git@github.com:22/acme/api"), expected);
        assert_eq!(gh("https://GitHub.com/acme/api/"), expected);
        assert_eq!(gh("git@acme.ghe.com:team/svc.git").0, "acme.ghe.com");
    }

    #[test]
    fn strips_credentials_from_urls() {
        let r = parse_remote("https://x-access-token:ghs_secretvalue@github.com/acme/api.git");
        assert_eq!(r.sanitized_url, "https://github.com/acme/api.git");
        assert!(r.is_github());
        let r = parse_remote("https://ghp_abcdef123456@github.com/acme/api");
        assert!(!r.sanitized_url.contains("ghp_"));
        let r = parse_remote("deploy@github.com:acme/api.git");
        assert_eq!(r.sanitized_url, "github.com:acme/api.git");
    }

    #[test]
    fn keeps_non_github_remotes_without_github_coordinates() {
        let r = parse_remote("https://gitlab.com/group/sub/repo.git");
        assert!(!r.is_github());
        assert_eq!(r.host.as_deref(), Some("gitlab.com"));
        assert_eq!(r.sanitized_url, "https://gitlab.com/group/sub/repo.git");
        let r = parse_remote("git@bitbucket.org:team/repo.git");
        assert!(!r.is_github());
        assert_eq!(r.owner.as_deref(), Some("team"));
        let local = parse_remote("/srv/git/repo.git");
        assert_eq!(local.host, None);
        let file = parse_remote("file:///srv/git/repo.git");
        assert_eq!(file.host, None);
        let windows = parse_remote("C:/repos/thing.git");
        assert_eq!(windows.host, None);
    }
}
