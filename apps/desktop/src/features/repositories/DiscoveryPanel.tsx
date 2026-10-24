import type { DiscoveryCandidate, DiscoveryResult, ImportSummary } from "@shiftops/core";
import { Badge, Button, Card, CardHeader, EmptyState, Spinner } from "@shiftops/ui";
import { FolderSearch, Square, TriangleAlert } from "lucide-react";
import { useMemo, useState } from "react";
import { ErrorDisplay } from "../../components/ErrorDisplay";
import { useImportDiscovered } from "../../lib/queries";
import type { DiscoveryState } from "./useDiscovery";

const WARNING_LABEL: Record<string, string> = {
  inaccessible: "Unreadable folder",
  symlink_escape: "Link outside root",
  broken_symlink: "Broken link",
  invalid_git_marker: "Invalid .git",
  entry_limit: "Entry limit",
};

function relative(root: string, path: string) {
  if (path === root) return ".";
  return path.startsWith(root + "/") || path.startsWith(root + "\\") ? path.slice(root.length + 1) : path;
}

function originLabel(c: DiscoveryCandidate) {
  const o = c.metadata.origin;
  if (!o) return <span className="text-muted">no origin</span>;
  if (o.owner && o.repo) return `${o.host === "github.com" ? "" : `${o.host}:`}${o.owner}/${o.repo}`;
  return <span className="text-muted">{o.sanitizedUrl}</span>;
}

function workingTree(c: DiscoveryCandidate) {
  const wt = c.metadata.workingTree;
  if (!wt) return <Badge tone="warn">not collected</Badge>;
  if (wt.staged + wt.modified + wt.untracked === 0) return <Badge tone="ok">clean</Badge>;
  return (
    <span className="font-mono text-xs" title="staged · modified · untracked">
      {wt.staged}·{wt.modified}·{wt.untracked}
    </span>
  );
}

function Results({ result, onImported }: { result: DiscoveryResult; onImported: (s: ImportSummary) => void }) {
  const importer = useImportDiscovered();
  const [selected, setSelected] = useState<Set<string>>(
    () => new Set(result.candidates.filter((c) => !c.alreadyImported).map((c) => c.path)),
  );
  const toggle = (path: string) =>
    setSelected((s) => {
      const next = new Set(s);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  const allSelected = result.candidates.length > 0 && selected.size === result.candidates.length;

  if (result.candidates.length === 0) {
    return (
      <EmptyState icon={<FolderSearch />} title="No Git repositories found">
        Searched {result.stats.directoriesVisited} folders to depth {result.maxDepth}. Increase the depth or choose a
        different folder.
      </EmptyState>
    );
  }

  return (
    <div>
      <div className="overflow-x-auto">
        <table className="w-full text-sm" aria-label="Discovered repositories">
          <thead>
            <tr className="border-b border-border text-left text-xs text-muted">
              <th className="w-8 px-4 py-2">
                <input
                  type="checkbox"
                  aria-label="Select all repositories"
                  checked={allSelected}
                  onChange={() => setSelected(allSelected ? new Set() : new Set(result.candidates.map((c) => c.path)))}
                />
              </th>
              <th className="px-2 py-2 font-medium">Repository</th>
              <th className="px-2 py-2 font-medium">Branch</th>
              <th className="px-2 py-2 font-medium">Commit</th>
              <th className="px-2 py-2 font-medium">Origin</th>
              <th className="px-2 py-2 font-medium">Working tree</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-border">
            {result.candidates.map((c) => (
              <tr key={c.path} data-candidate={c.path}>
                <td className="px-4 py-2 align-top">
                  <input
                    type="checkbox"
                    aria-label={`Select ${c.name}`}
                    checked={selected.has(c.path)}
                    onChange={() => toggle(c.path)}
                  />
                </td>
                <td className="px-2 py-2 align-top">
                  <div className="flex flex-wrap items-center gap-1.5">
                    <span className="font-medium">{c.name}</span>
                    {c.gitKind !== "standard" ? <Badge>{c.gitKind.replace("_", " ")}</Badge> : null}
                    {c.alreadyImported ? <Badge tone="accent">already imported</Badge> : null}
                  </div>
                  <div className="selectable font-mono text-[11px] text-muted">{relative(result.rootPath, c.path)}</div>
                  {c.metadata.warnings.map((w) => (
                    <p key={w} className="mt-1 flex items-start gap-1 text-[11px] text-warn">
                      <TriangleAlert className="mt-0.5 size-3 shrink-0" aria-hidden /> {w}
                    </p>
                  ))}
                </td>
                <td className="px-2 py-2 align-top font-mono text-xs">
                  {c.metadata.detached ? <Badge tone="warn">detached</Badge> : (c.metadata.currentBranch ?? "—")}
                </td>
                <td className="px-2 py-2 align-top font-mono text-xs">
                  {c.metadata.headCommit?.slice(0, 10) ?? <span className="text-muted">no commits</span>}
                </td>
                <td className="px-2 py-2 align-top font-mono text-xs">{originLabel(c)}</td>
                <td className="px-2 py-2 align-top">{workingTree(c)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <div className="flex flex-wrap items-center gap-3 border-t border-border px-4 py-3">
        <Button
          variant="primary"
          disabled={selected.size === 0 || importer.isPending}
          onClick={() =>
            importer.mutate(
              { discoveryId: result.discoveryId, paths: [...selected] },
              { onSuccess: (summary) => onImported(summary) },
            )
          }
        >
          {importer.isPending ? "Importing…" : `Import ${selected.size} selected`}
        </Button>
        <span className="text-xs text-muted">
          Imported repositories start as untrusted. Nothing is modified on disk.
        </span>
      </div>
      {importer.isError ? (
        <div className="px-4 pb-4">
          <ErrorDisplay title="Import failed" error={importer.error} />
        </div>
      ) : null}
    </div>
  );
}

export function DiscoveryPanel({
  state,
  onCancel,
  onClose,
}: {
  state: DiscoveryState;
  onCancel: () => void;
  onClose: () => void;
}) {
  const [imported, setImported] = useState<ImportSummary | null>(null);
  const result = state.phase === "done" ? state.result : null;
  const warnings = useMemo(() => result?.warnings ?? [], [result]);

  if (state.phase === "idle") return null;

  return (
    <Card aria-label="Discovery">
      <CardHeader
        title={
          state.phase === "done"
            ? `Discovery ${state.result.status}`
            : state.phase === "error"
              ? "Discovery could not start"
              : "Discovering repositories"
        }
        description={
          state.phase === "starting" || state.phase === "running"
            ? state.path
            : state.phase === "done"
              ? `${state.result.rootPath} · ${state.result.candidates.length} found · ${state.result.stats.directoriesVisited} folders · ${state.result.durationMs} ms`
              : undefined
        }
        actions={
          state.phase === "running" || state.phase === "starting" ? (
            <Button size="sm" variant="danger" onClick={onCancel} disabled={state.phase === "starting"}>
              <Square /> Cancel
            </Button>
          ) : (
            <Button size="sm" onClick={onClose}>
              Close
            </Button>
          )
        }
      />

      {state.phase === "starting" || state.phase === "running" ? (
        <div className="flex items-start gap-3 px-4 py-4 text-sm" aria-live="polite">
          <Spinner />
          {state.phase === "running" && state.progress ? (
            <div className="min-w-0">
              <p>
                {state.progress.phase === "walking"
                  ? `Searching… ${state.progress.stats.directoriesVisited} folders, ${state.progress.repositoriesFound} repositories found`
                  : `Reading Git metadata… ${state.progress.inspected} of ${state.progress.repositoriesFound}`}
              </p>
              <p className="selectable truncate font-mono text-[11px] text-muted" title={state.progress.currentPath}>
                {state.progress.currentPath}
              </p>
            </div>
          ) : (
            <p className="text-muted">Starting…</p>
          )}
        </div>
      ) : null}

      {state.phase === "error" ? (
        <div className="p-4">
          <ErrorDisplay title="Discovery could not start" error={state.error} />
        </div>
      ) : null}

      {result && result.status === "failed" && result.error ? (
        <div className="p-4">
          <ErrorDisplay title="Discovery failed" error={result.error} />
        </div>
      ) : null}

      {result && result.status === "cancelled" ? (
        <p className="px-4 py-4 text-sm text-muted">
          Cancelled. Nothing was imported. Run discovery again to continue.
        </p>
      ) : null}

      {warnings.length > 0 ? (
        <details className="border-b border-border px-4 py-2 text-sm">
          <summary className="cursor-pointer text-warn">
            {warnings.length} warning{warnings.length === 1 ? "" : "s"}
            {result?.truncated ? " · results are partial" : ""}
          </summary>
          <ul className="mt-2 space-y-1">
            {warnings.map((w, i) => (
              <li key={`${w.path}-${i}`} className="text-xs">
                <Badge tone="warn">{WARNING_LABEL[w.code] ?? w.code}</Badge>{" "}
                <span className="selectable font-mono">{w.path}</span> <span className="text-muted">{w.message}</span>
              </li>
            ))}
          </ul>
        </details>
      ) : null}

      {imported ? (
        <p role="status" className="border-b border-border px-4 py-2 text-sm">
          Imported {imported.imported}, refreshed {imported.updated}. Root{" "}
          <span className="font-mono">{imported.root.canonicalPath}</span> is now tracked.
        </p>
      ) : null}

      {result && result.status === "completed" ? <Results result={result} onImported={setImported} /> : null}
    </Card>
  );
}
