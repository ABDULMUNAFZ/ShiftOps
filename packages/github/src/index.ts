/**
 * GitHub provider. Step 1 ships the contract and remote-URL parsing only;
 * API calls are implemented in a later step and will read the token from the
 * OS keychain through the desktop native layer, never from JavaScript storage.
 */

export interface GitHubRepositoryRef {
  host: string;
  owner: string;
  name: string;
}

export interface GitHubPullRequestRef {
  repository: GitHubRepositoryRef;
  number: number;
  url: string;
}

export interface GitHubProvider {
  readonly id: "github";
  getAuthenticatedLogin(): Promise<string>;
  listOrganizationRepositories(org: string): Promise<GitHubRepositoryRef[]>;
  getPullRequest(ref: GitHubPullRequestRef): Promise<{ state: "open" | "closed" | "merged"; headSha: string }>;
}

const SCP_LIKE = /^(?:[\w.-]+@)?([\w.-]+):([\w.-]+)\/([\w.-]+?)(?:\.git)?\/?$/;

/**
 * Parse a Git remote URL into owner/name. Supports https, ssh:// and
 * scp-style (`git@host:owner/repo.git`) remotes. Returns null for anything
 * that is not a two-segment owner/repository path.
 */
export function parseGitRemote(remote: string): GitHubRepositoryRef | null {
  const value = remote.trim();
  if (value.length === 0) return null;

  if (!value.includes("://")) {
    const match = SCP_LIKE.exec(value);
    if (!match) return null;
    const [, host, owner, name] = match;
    return host && owner && name ? { host, owner, name } : null;
  }

  let url: URL;
  try {
    url = new URL(value);
  } catch {
    return null;
  }
  if (!["https:", "http:", "ssh:", "git:"].includes(url.protocol)) return null;
  const segments = url.pathname.replace(/^\/+|\/+$/g, "").split("/");
  if (segments.length !== 2) return null;
  const [owner, rawName] = segments;
  const name = rawName?.replace(/\.git$/, "");
  if (!owner || !name) return null;
  return { host: url.hostname, owner, name };
}

export function isGitHubHost(ref: GitHubRepositoryRef): boolean {
  return ref.host === "github.com" || ref.host.endsWith(".ghe.com");
}
