import type { DeviceFlowEvent, GithubAccount, GithubStatus } from "@shiftops/core";
import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { renderWithClient } from "../../test/render";

const native = vi.hoisted(() => ({
  githubStatus: vi.fn(),
  githubStartDeviceFlow: vi.fn(),
  githubCancelDeviceFlow: vi.fn(),
  githubConnectPat: vi.fn(),
  githubRefresh: vi.fn(),
  githubDisconnect: vi.fn(),
  githubListOrganizations: vi.fn(),
  githubBrowse: vi.fn(),
  githubImportRepository: vi.fn(),
  githubCancelRequest: vi.fn(),
  githubListPullRequests: vi.fn(),
  openExternalUrl: vi.fn(),
}));
const events = vi.hoisted(() => ({ handler: null as null | ((e: DeviceFlowEvent) => void) }));
vi.mock("../../lib/native", () => ({
  native,
  onGithubAuthEvent: async (h: (e: DeviceFlowEvent) => void) => {
    events.handler = h;
    return () => undefined;
  },
}));

const { GithubConnection } = await import("./GithubConnection");
const { GithubBrowser, GithubPullRequests } = await import("./GithubBrowser");

const TOKEN = "github_pat_ThisMustNeverBeShown0123456789";

const account: GithubAccount = {
  id: "a1",
  host: "github.com",
  apiBaseUrl: "https://api.github.com/",
  githubUserId: 42,
  login: "octo",
  name: "Octo Cat",
  htmlUrl: "https://github.com/octo",
  authMethod: "personal_access_token",
  credentialKind: "github_personal_access_token",
  scopes: [],
  scopesReported: false,
  status: "connected",
  validatedAt: "2026-10-24T10:00:00.000Z",
  rateLimit: { remaining: 4990, limit: 5000, reset: 1793000000 },
  createdAt: "t",
  updatedAt: "t",
};

function status(overrides: Partial<GithubStatus> = {}): GithubStatus {
  return {
    host: "https://github.com/",
    apiBaseUrl: "https://api.github.com/",
    oauthConfigured: true,
    account: null,
    credentialStored: false,
    ...overrides,
  };
}

beforeEach(() => {
  for (const fn of Object.values(native)) fn.mockReset();
  native.githubListOrganizations.mockResolvedValue([
    { id: "o1", githubId: 7, login: "acme", description: null, syncedAt: "t" },
  ]);
});

describe("GithubConnection", () => {
  it("shows Not configured and disables device sign-in without a client ID", async () => {
    native.githubStatus.mockResolvedValue(status({ oauthConfigured: false }));
    renderWithClient(<GithubConnection />);
    expect(await screen.findByText("Not configured")).toBeTruthy();
    expect((screen.getByRole("button", { name: "Sign in with GitHub" }) as HTMLButtonElement).disabled).toBe(true);
    expect(screen.getByText(/VITE_GITHUB_OAUTH_CLIENT_ID/)).toBeTruthy();
  });

  it("runs the device flow: code, copy, open, slow down, then success", async () => {
    native.githubStatus.mockResolvedValue(status());
    native.githubStartDeviceFlow.mockResolvedValue({
      flowId: "f1",
      userCode: "ABCD-1234",
      verificationUri: "https://github.com/login/device",
      expiresInSeconds: 900,
      intervalSeconds: 5,
    });
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.assign(navigator, { clipboard: { writeText } });
    renderWithClient(<GithubConnection />);
    fireEvent.click(await screen.findByRole("button", { name: "Sign in with GitHub" }));
    expect(await screen.findByText("ABCD-1234")).toBeTruthy();
    expect(screen.getByText("15:00")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Copy code" }));
    expect(writeText).toHaveBeenCalledWith("ABCD-1234");
    fireEvent.click(screen.getByRole("button", { name: /Open GitHub/ }));
    await waitFor(() => expect(native.openExternalUrl).toHaveBeenCalledWith("https://github.com/login/device"));

    act(() => events.handler?.({ flowId: "f1", state: "slow_down", intervalSeconds: 10, account: null, error: null }));
    expect(screen.getByText(/every 10 s/)).toBeTruthy();
    act(() =>
      events.handler?.({
        flowId: "f1",
        state: "succeeded",
        intervalSeconds: 10,
        account: { ...account, authMethod: "oauth_device" },
        error: null,
      }),
    );
    expect(await screen.findByText(/Connected as/)).toBeTruthy();
  });

  it("reports denial with the structured error and allows cancel", async () => {
    native.githubStatus.mockResolvedValue(status());
    native.githubStartDeviceFlow.mockResolvedValue({
      flowId: "f2",
      userCode: "WXYZ-0000",
      verificationUri: "https://github.com/login/device",
      expiresInSeconds: 900,
      intervalSeconds: 5,
    });
    native.githubCancelDeviceFlow.mockResolvedValue({ discoveryId: "f2", outcome: "requested" });
    renderWithClient(<GithubConnection />);
    fireEvent.click(await screen.findByRole("button", { name: "Sign in with GitHub" }));
    fireEvent.click(await screen.findByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(native.githubCancelDeviceFlow).toHaveBeenCalledWith("f2"));
    act(() =>
      events.handler?.({
        flowId: "f2",
        state: "denied",
        intervalSeconds: 5,
        account: null,
        error: {
          code: "GITHUB_AUTHORIZATION_DENIED",
          message: "Authorization was declined on GitHub.",
          technicalDetails: null,
          suggestedAction: "Start again if this was a mistake.",
          recoverable: true,
        },
      }),
    );
    expect((await screen.findByRole("alert")).textContent).toContain("GITHUB_AUTHORIZATION_DENIED");
  });

  it("validates a PAT through the bridge, clears it, and never displays it", async () => {
    native.githubStatus.mockResolvedValue(status({ oauthConfigured: false }));
    native.githubConnectPat.mockResolvedValue(account);
    renderWithClient(<GithubConnection />);
    const input = (await screen.findByLabelText("Personal access token")) as HTMLInputElement;
    expect(input.type).toBe("password");
    fireEvent.change(input, { target: { value: TOKEN } });
    fireEvent.click(screen.getByRole("button", { name: "Validate and connect" }));
    await waitFor(() => expect(native.githubConnectPat).toHaveBeenCalledWith(TOKEN));
    await waitFor(() => expect(input.value).toBe(""));
    expect(document.body.textContent).not.toContain(TOKEN);
  });

  it("shows real account metadata and disconnects only after confirmation", async () => {
    native.githubStatus.mockResolvedValue(status({ account, credentialStored: true }));
    native.githubDisconnect.mockResolvedValue({ ...account, status: "disconnected" });
    renderWithClient(<GithubConnection />);
    expect(await screen.findByText("Connected as octo")).toBeTruthy();
    expect(screen.getByText(/Not reported \(fine-grained/)).toBeTruthy();
    expect(screen.getByText(/4990 of 5000 requests left/)).toBeTruthy();
    expect(await screen.findByText("acme")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Disconnect…" }));
    expect(native.githubDisconnect).not.toHaveBeenCalled();
    fireEvent.click(within(screen.getByRole("alertdialog")).getByRole("button", { name: "Disconnect" }));
    await waitFor(() => expect(native.githubDisconnect).toHaveBeenCalled());
  });
});

describe("GithubBrowser", () => {
  const repo = (id: string, name: string, repositoryId: string | null) => ({
    id,
    accountId: "a1",
    githubId: Number(id.slice(1)),
    owner: "acme",
    name,
    fullName: `acme/${name}`,
    private: true,
    archived: false,
    fork: false,
    defaultBranch: "main",
    htmlUrl: `https://github.com/acme/${name}`,
    cloneUrl: null,
    sshUrl: null,
    description: null,
    primaryLanguage: "TypeScript",
    languages: null,
    pushedAt: "2026-10-20T10:00:00Z",
    remoteUpdatedAt: null,
    permissions: null,
    repositoryId,
    syncedAt: "t",
  });

  it("filters by owner, paginates and imports", async () => {
    native.githubStatus.mockResolvedValue(status({ account }));
    native.githubBrowse.mockResolvedValue({
      items: [repo("g1", "api", null), repo("g2", "web", "r9")],
      page: 1,
      perPage: 25,
      hasNextPage: true,
      totalCount: null,
    });
    native.githubImportRepository.mockResolvedValue({ repository: {}, created: true });
    renderWithClient(<GithubBrowser onClose={() => undefined} />);
    const table = await screen.findByRole("table", { name: "GitHub repositories" });
    expect(within(table).getByText("In ShiftOps")).toBeTruthy();
    fireEvent.click(within(table).getByRole("button", { name: "Import acme/api" }));
    await waitFor(() => expect(native.githubImportRepository).toHaveBeenCalledWith("g1"));

    fireEvent.change(screen.getByLabelText("Owner"), { target: { value: "acme" } });
    await waitFor(() =>
      expect(native.githubBrowse).toHaveBeenLastCalledWith(
        expect.objectContaining({ owner: "acme", page: 1 }),
        expect.any(String),
      ),
    );
    fireEvent.click(await screen.findByRole("button", { name: "Next" }));
    await waitFor(() =>
      expect(native.githubBrowse).toHaveBeenLastCalledWith(expect.objectContaining({ page: 2 }), expect.any(String)),
    );
  });

  it("shows a structured rate-limit error with retry", async () => {
    native.githubStatus.mockResolvedValue(status({ account }));
    native.githubBrowse.mockRejectedValue({
      code: "GITHUB_RATE_LIMITED",
      message: "GitHub's API rate limit is exhausted.",
      technicalDetails: "403 user/repos: API rate limit exceeded",
      suggestedAction: "Wait until the rate limit resets, then try again.",
      recoverable: true,
      context: { resetAt: 1793000000 },
    });
    renderWithClient(<GithubBrowser onClose={() => undefined} />);
    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("GITHUB_RATE_LIMITED");
    expect(within(alert).getByRole("button", { name: "Retry" })).toBeTruthy();
  });
});

describe("GithubPullRequests", () => {
  it("lists open pull requests with check summaries", async () => {
    native.githubStatus.mockResolvedValue(status({ account }));
    native.githubListPullRequests.mockResolvedValue([
      {
        id: "p1",
        githubRepositoryId: "g1",
        githubId: 900,
        number: 12,
        title: "Upgrade Node",
        state: "open",
        draft: false,
        authorLogin: "octo",
        htmlUrl: "https://github.com/acme/api/pull/12",
        headRef: "shiftops/node-20",
        headSha: "abc1234def",
        headRepoFullName: "acme/api",
        baseRef: "main",
        baseSha: null,
        mergeable: null,
        mergeableState: null,
        checks: { total: 3, success: 2, failure: 1, pending: 0, neutral: 0 },
        remoteCreatedAt: null,
        remoteUpdatedAt: "2026-10-21T10:00:00Z",
        syncedAt: "t",
      },
    ]);
    // Only GitHub fields are read by the component.
    const repo = { githubOwner: "acme", githubRepo: "api" } as Parameters<typeof GithubPullRequests>[0]["repo"];
    renderWithClient(<GithubPullRequests repo={repo} />);
    const table = await screen.findByRole("table", { name: "Pull requests" });
    expect(within(table).getByText("1 failing")).toBeTruthy();
    expect(within(table).getByText(/shiftops\/node-20 → main/)).toBeTruthy();
    expect(native.githubListPullRequests).toHaveBeenCalledWith("acme", "api");
  });
});
