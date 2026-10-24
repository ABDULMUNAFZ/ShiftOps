import type { GithubBrowsePage, GithubBrowseParams, GithubPullRequest, Repository } from "@shiftops/core";
import { Badge, Button, Card, CardHeader, EmptyState, Spinner } from "@shiftops/ui";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { GitPullRequest, Search, Square } from "lucide-react";
import { useRef, useState } from "react";
import { ErrorDisplay } from "../../components/ErrorDisplay";
import { ExternalLink } from "../../components/ExternalLink";
import { invalidateAfter } from "../../lib/invalidation";
import { native } from "../../lib/native";
import { useGithubOrganizations, useGithubStatus } from "../../lib/queries";
import { queryKeys } from "../../lib/queryKeys";

const PER_PAGE = 25;

function relative(iso: string | null) {
  if (!iso) return "—";
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? iso : d.toLocaleDateString();
}

/** Browse repositories the connected GitHub account can access and import them as metadata. */
export function GithubBrowser({ onClose }: { onClose: () => void }) {
  const client = useQueryClient();
  const status = useGithubStatus();
  const connected = Boolean(status.data?.account && status.data.account.status === "connected");
  const orgs = useGithubOrganizations(connected);
  const [draft, setDraft] = useState("");
  const [params, setParams] = useState<GithubBrowseParams>({ page: 1, perPage: PER_PAGE, visibility: "all" });
  const requestId = useRef<string | null>(null);

  const page = useQuery({
    queryKey: queryKeys.github.browse(params),
    enabled: connected,
    retry: false,
    queryFn: async (): Promise<GithubBrowsePage> => {
      const id = crypto.randomUUID();
      requestId.current = id;
      try {
        return await native.githubBrowse(params, id);
      } finally {
        if (requestId.current === id) requestId.current = null;
      }
    },
  });
  const importRepo = useMutation({
    mutationFn: (id: string) => native.githubImportRepository(id),
    onSettled: () => invalidateAfter(client, "githubImport"),
  });
  const set = (patch: Partial<GithubBrowseParams>) => setParams((p) => ({ ...p, page: 1, ...patch }));

  if (status.isPending) return null;
  return (
    <Card aria-label="Browse GitHub">
      <CardHeader
        title="Browse GitHub"
        description={connected ? `Repositories visible to ${status.data?.account?.login}` : undefined}
        actions={
          <Button size="sm" onClick={onClose}>
            Close
          </Button>
        }
      />
      {!connected ? (
        <EmptyState title="GitHub is not connected">
          Connect an account in Integrations to browse repositories.
        </EmptyState>
      ) : (
        <>
          <form
            className="flex flex-wrap items-end gap-2 border-b border-border px-4 py-2 text-sm"
            onSubmit={(e) => {
              e.preventDefault();
              set({ query: draft.trim() || undefined });
            }}
          >
            <label className="space-y-1">
              <span className="block text-xs text-muted">Owner</span>
              <select
                id="github-owner"
                value={params.owner ?? ""}
                onChange={(e) => set({ owner: e.target.value || undefined })}
                className="h-7 rounded-md border border-border bg-surface px-1"
              >
                <option value="">All accessible</option>
                <option value={status.data?.account?.login}>{status.data?.account?.login} (you)</option>
                {(orgs.data ?? []).map((o) => (
                  <option key={o.id} value={o.login}>
                    {o.login}
                  </option>
                ))}
              </select>
            </label>
            <label className="space-y-1">
              <span className="block text-xs text-muted">Visibility</span>
              <select
                id="github-visibility"
                value={params.visibility ?? "all"}
                onChange={(e) => set({ visibility: e.target.value as GithubBrowseParams["visibility"] })}
                className="h-7 rounded-md border border-border bg-surface px-1"
              >
                <option value="all">All</option>
                <option value="public">Public</option>
                <option value="private">Private</option>
              </select>
            </label>
            <label className="flex h-7 items-center gap-1.5">
              <input
                type="checkbox"
                checked={params.includeArchived ?? false}
                onChange={(e) => set({ includeArchived: e.target.checked })}
              />
              Include archived
            </label>
            <label htmlFor="github-search" className="sr-only">
              Search repository names
            </label>
            <input
              id="github-search"
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
              placeholder="Search names"
              className="h-7 min-w-40 flex-1 rounded-md border border-border bg-surface px-2"
            />
            <Button size="sm" type="submit">
              <Search /> Search
            </Button>
          </form>

          {page.isFetching ? (
            <div className="flex items-center gap-3 px-4 py-3 text-sm text-muted">
              <Spinner /> Loading from GitHub…
              <Button
                size="sm"
                variant="ghost"
                onClick={() => {
                  if (requestId.current) void native.githubCancelRequest(requestId.current);
                }}
              >
                <Square /> Cancel
              </Button>
            </div>
          ) : null}
          {page.isError && !page.isFetching ? (
            <div className="p-4">
              <ErrorDisplay
                title="GitHub repositories could not be loaded"
                error={page.error}
                action={
                  <Button size="sm" onClick={() => void page.refetch()}>
                    Retry
                  </Button>
                }
              />
            </div>
          ) : null}
          {page.data && !page.isFetching ? (
            page.data.items.length === 0 ? (
              <EmptyState title="No repositories match">Change the owner, filters or search text.</EmptyState>
            ) : (
              <table className="w-full text-sm" aria-label="GitHub repositories">
                <thead>
                  <tr className="border-b border-border text-left text-xs text-muted">
                    <th className="px-4 py-2 font-medium">Repository</th>
                    <th className="px-2 py-2 font-medium">Language</th>
                    <th className="px-2 py-2 font-medium">Updated</th>
                    <th className="px-2 py-2 text-right font-medium">ShiftOps</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-border">
                  {page.data.items.map((r) => (
                    <tr key={r.id}>
                      <td className="px-4 py-2">
                        <span className="font-medium">{r.fullName}</span> {r.private ? <Badge>private</Badge> : null}{" "}
                        {r.archived ? <Badge tone="warn">archived</Badge> : null} {r.fork ? <Badge>fork</Badge> : null}
                        {r.description ? <p className="text-xs text-muted">{r.description}</p> : null}
                      </td>
                      <td className="px-2 py-2 text-xs">{r.primaryLanguage ?? "—"}</td>
                      <td className="px-2 py-2 text-xs">{relative(r.pushedAt ?? r.remoteUpdatedAt)}</td>
                      <td className="px-2 py-2 text-right">
                        {r.repositoryId ? (
                          <Badge tone="ok">In ShiftOps</Badge>
                        ) : (
                          <Button
                            size="sm"
                            onClick={() => importRepo.mutate(r.id)}
                            disabled={importRepo.isPending}
                            aria-label={`Import ${r.fullName}`}
                          >
                            Import
                          </Button>
                        )}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )
          ) : null}
          {importRepo.isError ? (
            <div className="p-4">
              <ErrorDisplay title="Import failed" error={importRepo.error} />
            </div>
          ) : null}
          <div className="flex items-center justify-between border-t border-border px-4 py-2 text-xs text-muted">
            <span>
              Page {params.page}
              {page.data?.totalCount !== null && page.data?.totalCount !== undefined
                ? ` · ${page.data.totalCount} matches`
                : ""}
              . Imported repositories are metadata only; discover a local checkout to link it.
            </span>
            <div className="flex gap-2">
              <Button
                size="sm"
                disabled={(params.page ?? 1) <= 1 || page.isFetching}
                onClick={() => setParams((p) => ({ ...p, page: Math.max(1, (p.page ?? 1) - 1) }))}
              >
                Previous
              </Button>
              <Button
                size="sm"
                disabled={!page.data?.hasNextPage || page.isFetching}
                onClick={() => setParams((p) => ({ ...p, page: (p.page ?? 1) + 1 }))}
              >
                Next
              </Button>
            </div>
          </div>
        </>
      )}
    </Card>
  );
}

function checksBadge(pr: GithubPullRequest) {
  const c = pr.checks;
  if (!c) return <span className="text-muted">—</span>;
  if (c.total === 0) return <Badge>no checks</Badge>;
  if (c.failure > 0) return <Badge tone="danger">{c.failure} failing</Badge>;
  if (c.pending > 0) return <Badge tone="warn">{c.pending} pending</Badge>;
  return <Badge tone="ok">{c.success} passing</Badge>;
}

/** Open pull requests for a ShiftOps repository linked to GitHub. */
export function GithubPullRequests({ repo }: { repo: Repository }) {
  const status = useGithubStatus();
  const connected = Boolean(status.data?.account && status.data.account.status === "connected");
  const owner = repo.githubOwner ?? "";
  const name = repo.githubRepo ?? "";
  const pulls = useQuery({
    queryKey: queryKeys.github.pulls(owner, name),
    enabled: connected && Boolean(owner && name),
    retry: false,
    queryFn: () => native.githubListPullRequests(owner, name),
  });

  if (!repo.githubOwner) {
    return (
      <EmptyState icon={<GitPullRequest />} title="Not linked to GitHub">
        This repository has no GitHub origin, so it has no pull requests to show.
      </EmptyState>
    );
  }
  if (status.isPending) return <Spinner />;
  if (!connected) {
    return (
      <EmptyState icon={<GitPullRequest />} title="GitHub is not connected">
        Connect a GitHub account in Integrations to see pull requests for {owner}/{name}.
      </EmptyState>
    );
  }
  if (pulls.isPending)
    return (
      <div className="flex items-center gap-2 p-4 text-muted">
        <Spinner /> Loading pull requests…
      </div>
    );
  if (pulls.isError)
    return (
      <div className="p-4">
        <ErrorDisplay
          title="Pull requests could not be loaded"
          error={pulls.error}
          action={
            <Button size="sm" onClick={() => void pulls.refetch()}>
              Retry
            </Button>
          }
        />
      </div>
    );
  if (pulls.data.length === 0) return <EmptyState icon={<GitPullRequest />} title="No open pull requests" />;
  return (
    <table className="w-full text-sm" aria-label="Pull requests">
      <thead>
        <tr className="border-b border-border text-left text-xs text-muted">
          <th className="px-4 py-2 font-medium">Pull request</th>
          <th className="px-2 py-2 font-medium">Branches</th>
          <th className="px-2 py-2 font-medium">Checks</th>
          <th className="px-2 py-2 font-medium">Updated</th>
        </tr>
      </thead>
      <tbody className="divide-y divide-border">
        {pulls.data.map((pr) => (
          <tr key={pr.id}>
            <td className="px-4 py-2">
              <ExternalLink href={pr.htmlUrl}>
                #{pr.number} {pr.title}
              </ExternalLink>{" "}
              {pr.draft ? <Badge>draft</Badge> : null}
              <p className="text-xs text-muted">{pr.authorLogin ?? "unknown author"}</p>
            </td>
            <td className="px-2 py-2 font-mono text-xs">
              {pr.headRef} → {pr.baseRef}
              <p className="text-muted">{pr.headSha.slice(0, 7)}</p>
            </td>
            <td className="px-2 py-2">{checksBadge(pr)}</td>
            <td className="px-2 py-2 text-xs">{relative(pr.remoteUpdatedAt)}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}
