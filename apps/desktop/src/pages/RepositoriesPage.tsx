import type { Repository, RepositoryRoot } from "@shiftops/core";
import { Badge, Button, Card, CardHeader, EmptyState, Spinner } from "@shiftops/ui";
import { useMutation } from "@tanstack/react-query";
import { FolderGit2, FolderOpen, FolderSearch, GitBranch } from "lucide-react";
import { useState } from "react";
import { Link } from "react-router";
import { ErrorDisplay } from "../components/ErrorDisplay";
import { GithubBrowser } from "../features/github/GithubBrowser";
import { DiscoveryPanel } from "../features/repositories/DiscoveryPanel";
import { useDiscovery } from "../features/repositories/useDiscovery";
import { native } from "../lib/native";
import { useRemoveRepositoryRoot, useRepositories, useRepositoryRoots } from "../lib/queries";
import { Page } from "./Page";

function formatTime(iso: string | null) {
  if (!iso) return "never";
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? iso : d.toLocaleString();
}

function DiscoverOptions({
  path,
  onStart,
  onCancel,
}: {
  path: string;
  onStart: (o: { maxDepth: number; followInternalSymlinks: boolean }) => void;
  onCancel: () => void;
}) {
  const [maxDepth, setMaxDepth] = useState(4);
  const [follow, setFollow] = useState(false);
  return (
    <Card aria-label="Discovery options">
      <CardHeader title="Discover repositories in folder" description={path} />
      <form
        className="flex flex-wrap items-end gap-4 p-4 text-sm"
        onSubmit={(e) => {
          e.preventDefault();
          onStart({ maxDepth, followInternalSymlinks: follow });
        }}
      >
        <label className="space-y-1">
          <span className="block text-xs font-medium">Maximum depth</span>
          <select
            id="discovery-depth"
            value={maxDepth}
            onChange={(e) => setMaxDepth(Number(e.target.value))}
            className="h-8 rounded-md border border-border bg-surface px-2"
          >
            {Array.from({ length: 13 }, (_, i) => (
              <option key={i} value={i}>
                {i}
              </option>
            ))}
          </select>
        </label>
        <label className="flex h-8 items-center gap-2">
          <input id="discovery-follow" type="checkbox" checked={follow} onChange={(e) => setFollow(e.target.checked)} />
          Follow symbolic links that stay inside this folder
        </label>
        <div className="flex gap-2">
          <Button type="submit" variant="primary">
            Start discovery
          </Button>
          <Button onClick={onCancel}>Cancel</Button>
        </div>
        <p className="basis-full text-xs text-muted">
          Skips dependency, build and cache folders and hidden folders. Links leading outside this folder are never
          followed. Only read-only Git commands run.
        </p>
      </form>
    </Card>
  );
}

function RootsCard() {
  const roots = useRepositoryRoots();
  const remove = useRemoveRepositoryRoot();
  const [confirming, setConfirming] = useState<string | null>(null);

  if (roots.isPending || (roots.data && roots.data.length === 0)) return null;
  return (
    <Card>
      <CardHeader
        title="Tracked folders"
        description="Removing a folder only stops tracking it. No files are deleted."
      />
      {roots.isError ? (
        <div className="p-4">
          <ErrorDisplay title="Tracked folders could not be loaded" error={roots.error} />
        </div>
      ) : (
        <ul className="divide-y divide-border">
          {roots.data.map((r: RepositoryRoot) => (
            <li key={r.id} className="flex flex-wrap items-center justify-between gap-3 px-4 py-2.5">
              <div className="min-w-0">
                <p className="font-medium">{r.displayName}</p>
                <p className="selectable truncate font-mono text-[11px] text-muted">{r.canonicalPath}</p>
                <p className="text-[11px] text-muted">
                  {r.rootKind === "repository" ? "Single repository" : `Depth ${r.maxDepth}`} · {r.repositoryCount}{" "}
                  repositories · last discovered {formatTime(r.lastDiscoveredAt)}
                </p>
              </div>
              {confirming === r.id ? (
                <div
                  role="alertdialog"
                  aria-label={`Stop tracking ${r.displayName}`}
                  className="flex items-center gap-2"
                >
                  <span className="text-xs">Stop tracking? Repositories stay registered.</span>
                  <Button
                    size="sm"
                    variant="danger"
                    disabled={remove.isPending}
                    onClick={() => remove.mutate(r.id, { onSuccess: () => setConfirming(null) })}
                  >
                    Stop tracking
                  </Button>
                  <Button size="sm" onClick={() => setConfirming(null)}>
                    Keep
                  </Button>
                </div>
              ) : (
                <Button size="sm" variant="ghost" onClick={() => setConfirming(r.id)}>
                  Remove…
                </Button>
              )}
            </li>
          ))}
        </ul>
      )}
      {remove.isError ? (
        <div className="p-4">
          <ErrorDisplay title="Folder not removed" error={remove.error} />
        </div>
      ) : null}
    </Card>
  );
}

function workingTree(r: Repository) {
  if (r.isDirty === null) return <Badge tone="warn">unknown</Badge>;
  if (!r.isDirty) return <Badge tone="ok">clean</Badge>;
  return (
    <span className="font-mono text-xs" title="staged · modified · untracked">
      {r.stagedCount}·{r.modifiedCount}·{r.untrackedCount}
    </span>
  );
}

const TRUST_TONE = { untrusted: "warn", trusted: "ok", blocked: "danger" } as const;

function RepositoryTable() {
  const [search, setSearch] = useState("");
  const repos = useRepositories(search.trim() ? { search: search.trim(), limit: 200 } : { limit: 200 });
  return (
    <Card>
      <CardHeader
        title="Repositories"
        description={repos.data ? `${repos.data.total} registered` : undefined}
        actions={
          <>
            <label htmlFor="repo-search" className="sr-only">
              Search repositories
            </label>
            <input
              id="repo-search"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder="Search name, path, owner"
              className="h-7 w-56 rounded-md border border-border bg-surface px-2 text-sm"
            />
          </>
        }
      />
      {repos.isPending ? (
        <div className="flex items-center gap-2 p-4 text-muted">
          <Spinner /> Loading…
        </div>
      ) : repos.isError ? (
        <div className="p-4">
          <ErrorDisplay
            title="Repositories could not be loaded"
            error={repos.error}
            action={
              <Button size="sm" onClick={() => void repos.refetch()}>
                Retry
              </Button>
            }
          />
        </div>
      ) : repos.data.items.length === 0 ? (
        <EmptyState icon={<FolderGit2 />} title={search ? "No matching repositories" : "No repositories yet"}>
          {search
            ? "Try a different search."
            : "Open a local repository, discover a folder, or browse GitHub to register repositories."}
        </EmptyState>
      ) : (
        <div className="overflow-x-auto">
          <table className="w-full text-sm" aria-label="Registered repositories">
            <thead>
              <tr className="border-b border-border text-left text-xs text-muted">
                <th className="px-4 py-2 font-medium">Repository</th>
                <th className="px-2 py-2 font-medium">Source</th>
                <th className="px-2 py-2 font-medium">GitHub</th>
                <th className="px-2 py-2 font-medium">Technology</th>
                <th className="px-2 py-2 font-medium">Runtime</th>
                <th className="px-2 py-2 font-medium">Packages</th>
                <th className="px-2 py-2 font-medium">Branch · commit</th>
                <th className="px-2 py-2 font-medium">Working tree</th>
                <th className="px-2 py-2 font-medium">Trust</th>
                <th className="px-2 py-2 font-medium">Scan</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-border">
              {repos.data.items.map((r) => (
                <tr key={r.id} className="hover:bg-surface-2">
                  <td className="px-4 py-2">
                    <Link to={`/repositories/${r.id}`} className="font-medium hover:underline">
                      {r.name}
                    </Link>
                    <p className="selectable font-mono text-[11px] text-muted">{r.localPath ?? r.remoteUrl}</p>
                  </td>
                  <td className="px-2 py-2 text-xs">
                    <Badge>
                      {r.ownership === "shiftops_managed"
                        ? "managed clone"
                        : r.ownership === "external"
                          ? "external clone"
                          : "metadata only"}
                    </Badge>
                    <p className="mt-1 text-[11px] text-muted">{r.sourceType}</p>
                  </td>
                  <td className="px-2 py-2 font-mono text-xs">
                    {r.githubOwner ? `${r.githubOwner}/${r.githubRepo}` : <span className="text-muted">—</span>}
                  </td>
                  <td className="px-2 py-2 text-xs">
                    {r.scanSummary
                      ? [r.scanSummary.primaryLanguage, ...r.scanSummary.frameworks.slice(0, 2)]
                          .filter(Boolean)
                          .join(" · ")
                      : "—"}
                  </td>
                  <td className="px-2 py-2 font-mono text-xs">
                    {r.scanSummary
                      ? Object.entries(r.scanSummary.runtimes)
                          .map(([k, v]) => `${k} ${v[0] ?? ""}`)
                          .join(", ") || "—"
                      : "—"}
                  </td>
                  <td className="px-2 py-2 text-xs">{r.scanSummary?.packageManagers.join(", ") || "—"}</td>
                  <td className="px-2 py-2 font-mono text-xs">
                    {r.isDetached ? <Badge tone="warn">detached</Badge> : (r.currentBranch ?? "—")}{" "}
                    <span className="text-muted">{r.currentCommit?.slice(0, 7) ?? ""}</span>
                  </td>
                  <td className="px-2 py-2">{workingTree(r)}</td>
                  <td className="px-2 py-2">
                    <Badge tone={TRUST_TONE[r.trustStatus]}>{r.trustStatus}</Badge>
                  </td>
                  <td className="px-2 py-2 text-xs">
                    {r.lastScanStatus === null ? (
                      <span className="text-muted">not scanned</span>
                    ) : (
                      <span title={r.lastScannedAt ?? undefined}>
                        <Badge
                          tone={
                            r.lastScanStatus === "completed"
                              ? "ok"
                              : r.lastScanStatus === "failed"
                                ? "danger"
                                : "neutral"
                          }
                        >
                          {r.lastScanStatus}
                        </Badge>{" "}
                        {r.scanSummary && r.scanSummary.conflicts > 0 ? (
                          <Badge tone="danger">{r.scanSummary.conflicts} conflicts</Badge>
                        ) : null}
                        <span className="mt-1 block text-[11px] text-muted">{formatTime(r.lastScannedAt)}</span>
                      </span>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </Card>
  );
}

export function RepositoriesPage() {
  const discovery = useDiscovery();
  const [pendingFolder, setPendingFolder] = useState<string | null>(null);
  const [browsing, setBrowsing] = useState(false);
  const pick = useMutation({
    mutationFn: async (mode: "repository" | "directory") => {
      const path = await native.pickDirectory(
        mode === "repository" ? "Open a local Git repository" : "Choose a folder to search for repositories",
      );
      return { mode, path };
    },
    onSuccess: ({ mode, path }) => {
      if (!path) return;
      if (mode === "repository") {
        void discovery.start({ path, mode, maxDepth: 0, followInternalSymlinks: false });
      } else {
        setPendingFolder(path);
      }
    },
  });
  const busy = discovery.state.phase === "starting" || discovery.state.phase === "running";

  return (
    <Page
      title="Repositories"
      description="Local repositories registered with ShiftOps. Discovery is read-only and never changes files."
      actions={
        <div className="flex gap-2">
          <Button onClick={() => setBrowsing((b) => !b)}>
            <GitBranch /> Browse GitHub
          </Button>
          <Button onClick={() => pick.mutate("repository")} disabled={busy || pick.isPending}>
            <FolderOpen /> Open Local Repository
          </Button>
          <Button variant="primary" onClick={() => pick.mutate("directory")} disabled={busy || pick.isPending}>
            <FolderSearch /> Discover Folder
          </Button>
        </div>
      }
    >
      {pick.isError ? <ErrorDisplay title="Folder picker failed" error={pick.error} /> : null}
      {pendingFolder ? (
        <DiscoverOptions
          path={pendingFolder}
          onCancel={() => setPendingFolder(null)}
          onStart={(o) => {
            const path = pendingFolder;
            setPendingFolder(null);
            void discovery.start({ path, mode: "directory", ...o });
          }}
        />
      ) : null}
      {browsing ? <GithubBrowser onClose={() => setBrowsing(false)} /> : null}
      <DiscoveryPanel state={discovery.state} onCancel={() => void discovery.cancel()} onClose={discovery.reset} />
      <RootsCard />
      <RepositoryTable />
    </Page>
  );
}
