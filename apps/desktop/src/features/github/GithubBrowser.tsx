import type {
  CloneEvent,
  GithubBrowsePage,
  GithubBrowseParams,
  GithubPullRequest,
  GithubRepository,
  LinkInspection,
  Repository,
} from "@shiftops/core";
import { Badge, Button, Card, CardHeader, EmptyState, Spinner } from "@shiftops/ui";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { GitPullRequest, Search, Square } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { ErrorDisplay } from "../../components/ErrorDisplay";
import { ExternalLink } from "../../components/ExternalLink";
import { invalidateAfter } from "../../lib/invalidation";
import { native, onCloneEvent } from "../../lib/native";
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
  const status = useGithubStatus();
  const connected = Boolean(status.data?.account && status.data.account.status === "connected");
  const orgs = useGithubOrganizations(connected);
  const [draft, setDraft] = useState("");
  const [params, setParams] = useState<GithubBrowseParams>({ page: 1, perPage: PER_PAGE, visibility: "all" });
  const [selected, setSelected] = useState<GithubRepository | null>(null);
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
          {selected ? <RepositoryImportPanel repository={selected} onClose={() => setSelected(null)} /> : null}
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
                          <Button size="sm" onClick={() => setSelected(r)} aria-label={`Add ${r.fullName}`}>
                            Add
                          </Button>
                        )}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )
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

type ImportMode = "choose" | "clone" | "link";

function RepositoryImportPanel({ repository, onClose }: { repository: GithubRepository; onClose: () => void }) {
  const client = useQueryClient();
  const [mode, setMode] = useState<ImportMode>("choose");
  const [cloneEvent, setCloneEvent] = useState<CloneEvent | null>(null);
  const [linkInspection, setLinkInspection] = useState<LinkInspection | null>(null);

  const workspace = useQuery({ queryKey: queryKeys.workspace.active, queryFn: native.getManagedWorkspace });
  const clones = useQuery({ queryKey: queryKeys.workspace.clones, queryFn: () => native.listRepositoryClones(25) });
  const preview = useQuery({
    queryKey: [...queryKeys.workspace.all, "preview", repository.id],
    queryFn: () => native.previewManagedClone(repository.id),
    enabled: mode === "clone",
    retry: false,
  });

  useEffect(() => {
    let dispose: (() => void) | undefined;
    void onCloneEvent((event) => {
      setCloneEvent(event);
      if (event.type === "completed") {
        void invalidateAfter(client, "workspace");
      }
    }).then((fn) => {
      dispose = fn;
    });
    return () => dispose?.();
  }, [client]);

  const metadata = useMutation({
    mutationFn: () => native.githubImportRepository(repository.id),
    onSuccess: async () => {
      await invalidateAfter(client, "githubImport");
      onClose();
    },
  });
  const approve = useMutation({
    mutationFn: async () => {
      const path = await native.pickDirectory("Choose a dedicated ShiftOps managed workspace");
      if (!path) return null;
      return native.approveManagedWorkspace(path);
    },
    onSuccess: async (value) => {
      if (value) await invalidateAfter(client, "workspace");
    },
  });
  const startClone = useMutation({
    mutationFn: async () => {
      if (!preview.data?.destination) throw new Error("No reviewed clone destination is available.");
      return native.startManagedClone(repository.id, preview.data.destination);
    },
    onSuccess: async () => invalidateAfter(client, "workspace"),
  });
  const cleanup = useMutation({
    mutationFn: ({ id, path }: { id: string; path: string }) => native.cleanupManagedClone(id, path),
    onSuccess: async () => invalidateAfter(client, "workspace"),
  });
  const inspectLink = useMutation({
    mutationFn: async () => {
      const path = await native.pickDirectory(`Choose the existing clone for ${repository.fullName}`);
      if (!path) return null;
      return native.inspectExistingClone(repository.id, path);
    },
    onSuccess: (value) => setLinkInspection(value),
  });
  const link = useMutation({
    mutationFn: (inspection: LinkInspection) =>
      native.linkExistingClone(
        repository.id,
        inspection.path,
        inspection.matches ? undefined : inspection.actualDisplay,
      ),
    onSuccess: async () => {
      await invalidateAfter(client, "workspace");
      onClose();
    },
  });

  const lastAttempt = clones.data?.find((attempt) => attempt.githubRepositoryId === repository.id);
  const error = metadata.error ?? approve.error ?? startClone.error ?? cleanup.error ?? inspectLink.error ?? link.error;

  return (
    <div className="border-b border-border bg-elevated/40 p-4" aria-label={`Import ${repository.fullName}`}>
      <div className="flex items-start justify-between gap-3">
        <div>
          <h3 className="font-medium">Add {repository.fullName}</h3>
          <p className="mt-1 text-xs text-muted">Choose what ShiftOps may do with this repository.</p>
        </div>
        <Button size="sm" variant="ghost" onClick={onClose}>
          Close
        </Button>
      </div>

      {mode === "choose" ? (
        <div className="mt-3 grid gap-2 md:grid-cols-3">
          <Button onClick={() => metadata.mutate()} disabled={metadata.isPending}>
            Metadata only
          </Button>
          <Button onClick={() => setMode("clone")}>Clone into managed workspace</Button>
          <Button onClick={() => setMode("link")}>Link existing clone</Button>
          <p className="text-xs text-muted">Browse metadata now; deep analysis needs a checkout.</p>
          <p className="text-xs text-muted">ShiftOps owns the new checkout but never changes its code in Task 3.</p>
          <p className="text-xs text-muted">Your existing checkout remains external and is never deleted.</p>
        </div>
      ) : null}

      {mode === "clone" ? (
        <div className="mt-3 space-y-3 text-sm">
          <Button size="sm" variant="ghost" onClick={() => setMode("choose")}>
            Back
          </Button>
          {workspace.data ? (
            <p>
              Approved workspace: <code>{workspace.data.canonicalPath}</code>
            </p>
          ) : (
            <div className="flex items-center gap-2">
              <span>No managed workspace is approved.</span>
              <Button size="sm" onClick={() => approve.mutate()} disabled={approve.isPending}>
                Choose and approve folder
              </Button>
            </div>
          )}
          {preview.isFetching ? (
            <p className="flex items-center gap-2 text-muted">
              <Spinner /> Preparing preview…
            </p>
          ) : null}
          {preview.data ? (
            <div className="rounded-md border border-border p-3">
              <p className="text-xs text-muted">Reviewed destination</p>
              <code className="break-all">{preview.data.destination ?? "Unavailable"}</code>
              {preview.data.problems.length ? (
                <ul className="mt-2 list-disc pl-5 text-xs text-danger">
                  {preview.data.problems.map((problem) => (
                    <li key={problem}>{problem}</li>
                  ))}
                </ul>
              ) : null}
              <Button
                className="mt-3"
                size="sm"
                disabled={!preview.data.ready || startClone.isPending}
                onClick={() => startClone.mutate()}
              >
                Start managed clone
              </Button>
            </div>
          ) : null}
          {cloneEvent?.type === "progress" ? (
            <p className="text-xs text-muted">
              {cloneEvent.phase ?? "Cloning"}
              {cloneEvent.percent === null ? "" : ` ${cloneEvent.percent}%`} · {cloneEvent.line}
            </p>
          ) : null}
          {cloneEvent?.type === "completed" ? (
            <p className="text-xs">
              Clone {cloneEvent.result.clone.status}. {cloneEvent.result.scanStarted ? "Repository scan started." : ""}
            </p>
          ) : null}
          {lastAttempt && ["running", "failed", "cancelled"].includes(lastAttempt.status) ? (
            <div className="rounded-md border border-border p-3 text-xs">
              <p>
                {lastAttempt.status}: {lastAttempt.destinationPath}
              </p>
              {lastAttempt.errorMessage ? <p className="mt-1 text-danger">{lastAttempt.errorMessage}</p> : null}
              {lastAttempt.status === "running" ? (
                <Button
                  className="mt-2"
                  size="sm"
                  variant="ghost"
                  onClick={() => void native.cancelManagedClone(lastAttempt.id)}
                >
                  Cancel clone
                </Button>
              ) : lastAttempt.partialPathExists ? (
                <Button
                  className="mt-2"
                  size="sm"
                  variant="danger"
                  onClick={() => cleanup.mutate({ id: lastAttempt.id, path: lastAttempt.destinationPath })}
                >
                  Delete this incomplete clone folder
                </Button>
              ) : null}
            </div>
          ) : null}
        </div>
      ) : null}

      {mode === "link" ? (
        <div className="mt-3 space-y-3 text-sm">
          <Button size="sm" variant="ghost" onClick={() => setMode("choose")}>
            Back
          </Button>
          <Button size="sm" onClick={() => inspectLink.mutate()} disabled={inspectLink.isPending}>
            Choose existing clone
          </Button>
          {linkInspection ? (
            <div className="rounded-md border border-border p-3 text-xs">
              <p>
                Folder: <code>{linkInspection.path}</code>
              </p>
              <p>
                Expected: {linkInspection.expected.host}/{linkInspection.expected.owner}/{linkInspection.expected.repo}
              </p>
              <p>Actual: {linkInspection.actualDisplay}</p>
              {!linkInspection.matches ? (
                <p className="mt-2 text-warn">
                  The remote does not match. Linking requires an explicit override and will be recorded.
                </p>
              ) : null}
              <Button
                className="mt-3"
                size="sm"
                variant={linkInspection.matches ? "primary" : "danger"}
                onClick={() => link.mutate(linkInspection)}
                disabled={link.isPending}
              >
                {linkInspection.matches
                  ? "Link and scan"
                  : `Override mismatch and link ${linkInspection.actualDisplay}`}
              </Button>
            </div>
          ) : null}
        </div>
      ) : null}

      {error ? (
        <div className="mt-3">
          <ErrorDisplay title="Repository import failed" error={error} />
        </div>
      ) : null}
    </div>
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
