import type {
  FactCategory,
  Repository,
  RepositoryConflict,
  RepositoryFact,
  RepositoryIntelligence,
} from "@shiftops/core";
import { Badge, Button, Card, EmptyState, Spinner, type BadgeTone } from "@shiftops/ui";
import { ArrowLeft, GitBranch, GitPullRequest, Play, RefreshCw, Square, TriangleAlert } from "lucide-react";
import { useMemo, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { Link, useParams } from "react-router";
import { ErrorDisplay } from "../components/ErrorDisplay";
import { ActivityListForEntity } from "../features/activity/ActivityCentre";
import { useRepositoryScan } from "../features/repositories/useScan";
import { useRepository, useRepositoryBranches, useRepositoryIntelligence } from "../lib/queries";
import { DefinitionList } from "./Page";

const TABS = [
  "Overview",
  "Evidence",
  "Runtimes",
  "Dependencies",
  "Scripts",
  "Docker",
  "CI",
  "Git",
  "Branches",
  "Pull Requests",
  "Activity",
] as const;
type Tab = (typeof TABS)[number];

const TRUST_TONE: Record<Repository["trustStatus"], BadgeTone> = {
  untrusted: "warn",
  trusted: "ok",
  blocked: "danger",
};

function formatTime(iso: string | null | undefined) {
  if (!iso) return "never";
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? iso : d.toLocaleString();
}

function Evidence({ fact }: { fact: Pick<RepositoryFact, "file" | "line" | "pointer"> }) {
  if (!fact.file) return <span className="text-muted">—</span>;
  return (
    <span className="selectable font-mono text-[11px]">
      {fact.file}
      {fact.line ? `:${fact.line}` : ""}
      {fact.pointer ? <span className="text-muted"> {fact.pointer}</span> : null}
    </span>
  );
}

function FactTable({ facts, empty, label }: { facts: RepositoryFact[]; empty: string; label: string }) {
  if (facts.length === 0) return <EmptyState title={empty} />;
  return (
    <div className="overflow-x-auto">
      <table className="w-full text-sm" aria-label={label}>
        <thead>
          <tr className="border-b border-border text-left text-xs text-muted">
            <th className="px-4 py-2 font-medium">Key</th>
            <th className="px-2 py-2 font-medium">Value</th>
            <th className="px-2 py-2 font-medium">Evidence</th>
            <th className="px-2 py-2 font-medium">Detected by</th>
            <th className="px-2 py-2 text-right font-medium">Confidence</th>
          </tr>
        </thead>
        <tbody className="divide-y divide-border">
          {facts.map((f) => (
            <tr key={f.id}>
              <td className="px-4 py-1.5 font-medium">{f.key}</td>
              <td className="selectable max-w-md break-words px-2 py-1.5 font-mono text-xs">{f.value || "—"}</td>
              <td className="px-2 py-1.5">
                <Evidence fact={f} />
              </td>
              <td className="px-2 py-1.5 text-xs text-muted">{f.method}</td>
              <td className="px-2 py-1.5 text-right font-mono text-xs tabular-nums">
                {Math.round(f.confidence * 100)}%
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function Conflicts({ conflicts }: { conflicts: RepositoryConflict[] }) {
  if (conflicts.length === 0) return null;
  return (
    <ul className="divide-y divide-border border-b border-border" aria-label="Conflicts">
      {conflicts.map((c) => (
        <li key={c.id} className="px-4 py-2.5">
          <p className="flex items-start gap-2 text-sm text-danger">
            <TriangleAlert className="mt-0.5 size-4 shrink-0" aria-hidden /> {c.message}
          </p>
          <ul className="mt-1 space-y-0.5 pl-6">
            {c.values.map((v, i) => (
              <li key={i} className="text-xs">
                <span className="font-mono">{v.value}</span> <span className="text-muted">· {v.method} ·</span>{" "}
                <Evidence fact={{ file: v.file, line: v.line, pointer: null }} />
              </li>
            ))}
          </ul>
        </li>
      ))}
    </ul>
  );
}

function byCategory(intel: RepositoryIntelligence, ...cats: FactCategory[]) {
  return intel.facts.filter((f) => cats.includes(f.category));
}

function Overview({ repo, intel }: { repo: Repository; intel: RepositoryIntelligence }) {
  const summary = intel.scan?.result;
  if (!intel.scan || !summary) {
    return (
      <EmptyState title="Not scanned yet">
        Run a scan to detect languages, runtimes, package managers, frameworks, Docker and CI from {repo.name}&apos;s
        own files. The scan is read-only and never runs repository code.
      </EmptyState>
    );
  }
  const total = summary.languages.reduce((n, l) => n + l.files, 0) || 1;
  return (
    <div>
      <Conflicts conflicts={intel.conflicts} />
      <div className="grid gap-x-8 gap-y-4 p-4 text-sm md:grid-cols-2">
        <div>
          <h3 className="mb-2 text-xs font-semibold uppercase tracking-wide text-muted">Languages (files)</h3>
          {summary.languages.length === 0 ? (
            <p className="text-muted">No source files recognised.</p>
          ) : (
            <ul className="space-y-1.5">
              {summary.languages.map((l) => (
                <li key={l.name} className="grid grid-cols-[7rem_1fr_3rem] items-center gap-2">
                  <span>{l.name}</span>
                  <span className="h-1.5 rounded bg-surface-2">
                    <span className="block h-1.5 rounded bg-accent" style={{ width: `${(l.files / total) * 100}%` }} />
                  </span>
                  <span className="text-right font-mono text-xs tabular-nums">{l.files}</span>
                </li>
              ))}
            </ul>
          )}
        </div>
        <DefinitionList
          rows={[
            {
              term: "Runtimes",
              value:
                Object.entries(summary.runtimes)
                  .map(([k, v]) => `${k} ${v.join(" / ")}`)
                  .join(", ") || "none declared",
            },
            { term: "Package managers", value: summary.packageManagers.join(", ") || "none detected" },
            { term: "Frameworks", value: summary.frameworks.join(", ") || "none detected" },
            { term: "Docker", value: summary.hasDocker ? "yes" : "no" },
            { term: "CI", value: summary.ci.join(", ") || "none detected" },
            { term: "Facts / conflicts", value: `${summary.facts} / ${summary.conflicts}` },
          ]}
        />
      </div>
      {summary.warnings.length > 0 ? (
        <details className="border-t border-border px-4 py-2 text-sm">
          <summary className="cursor-pointer text-warn">{summary.warnings.length} scan warnings</summary>
          <ul className="mt-2 space-y-1">
            {summary.warnings.map((w, i) => (
              <li key={i} className="text-xs">
                <span className="font-mono">{w.path || "."}</span> <span className="text-muted">{w.message}</span>
              </li>
            ))}
          </ul>
        </details>
      ) : null}
      <p className="border-t border-border px-4 py-2 text-[11px] text-muted">
        Scanned {formatTime(intel.scan.completedAt)} · commit {intel.scan.scannedCommit?.slice(0, 10) ?? "none"} ·
        scanner v{summary.scannerVersion} · {summary.relevantFiles} files read
        {summary.truncated ? " · limits reached, results partial" : ""}
      </p>
    </div>
  );
}

function EvidenceTab({ intel }: { intel: RepositoryIntelligence }) {
  const [category, setCategory] = useState<FactCategory | "">("");
  const facts = category ? intel.facts.filter((f) => f.category === category) : intel.facts;
  return (
    <div>
      <div className="flex items-center gap-2 border-b border-border px-4 py-2">
        <label htmlFor="evidence-category" className="text-xs text-muted">
          Category
        </label>
        <select
          id="evidence-category"
          value={category}
          onChange={(e) => setCategory(e.target.value as FactCategory | "")}
          className="h-7 rounded-md border border-border bg-surface px-1 text-sm"
        >
          <option value="">All ({intel.facts.length})</option>
          {[...new Set(intel.facts.map((f) => f.category))].map((c) => (
            <option key={c} value={c}>
              {c.replace("_", " ")}
            </option>
          ))}
        </select>
      </div>
      <Conflicts conflicts={intel.conflicts} />
      <FactTable facts={facts} empty="No facts recorded" label="Evidence" />
    </div>
  );
}

function DependenciesTab({ intel }: { intel: RepositoryIntelligence }) {
  const [search, setSearch] = useState("");
  const deps = byCategory(intel, "dependency").filter((f) => f.key.toLowerCase().includes(search.toLowerCase()));
  return (
    <div>
      <div className="border-b border-border px-4 py-2">
        <label htmlFor="dependency-search" className="sr-only">
          Filter dependencies
        </label>
        <input
          id="dependency-search"
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          placeholder="Filter dependencies"
          className="h-7 w-64 rounded-md border border-border bg-surface px-2 text-sm"
        />
      </div>
      <FactTable facts={deps} empty="No dependencies declared" label="Dependencies" />
    </div>
  );
}

function GitTab({ repo }: { repo: Repository }) {
  return (
    <div>
      <DefinitionList
        rows={[
          { term: "Path", value: repo.localPath ?? "—" },
          { term: "Kind", value: repo.gitKind?.replace("_", " ") ?? "unknown" },
          { term: "Current branch", value: repo.isDetached ? "detached HEAD" : (repo.currentBranch ?? "—") },
          { term: "Default branch", value: repo.defaultBranch ?? "unknown (no origin/HEAD)" },
          { term: "HEAD commit", value: repo.currentCommit ?? "no commits" },
          { term: "Last commit", value: formatTime(repo.lastCommitAt) },
          { term: "Origin", value: repo.remoteUrl ?? "none" },
          {
            term: "Working tree",
            value:
              repo.isDirty === null
                ? "not collected"
                : `${repo.stagedCount} staged · ${repo.modifiedCount} modified · ${repo.untrackedCount} untracked`,
          },
          { term: "Collected", value: formatTime(repo.gitCollectedAt) },
        ]}
      />
      {repo.gitWarnings.length > 0 ? (
        <ul className="border-t border-border px-4 py-2 text-xs text-warn">
          {repo.gitWarnings.map((w) => (
            <li key={w}>{w}</li>
          ))}
        </ul>
      ) : null}
    </div>
  );
}

function BranchesTab({ repo }: { repo: Repository }) {
  const branches = useRepositoryBranches(repo.id);
  if (branches.isPending)
    return (
      <div className="flex items-center gap-2 p-4 text-muted">
        <Spinner /> Reading branches…
      </div>
    );
  if (branches.isError)
    return (
      <div className="p-4">
        <ErrorDisplay
          title="Branches could not be read"
          error={branches.error}
          action={
            <Button size="sm" onClick={() => void branches.refetch()}>
              Retry
            </Button>
          }
        />
      </div>
    );
  if (branches.data.length === 0) return <EmptyState icon={<GitBranch />} title="No local branches" />;
  return (
    <table className="w-full text-sm" aria-label="Branches">
      <thead>
        <tr className="border-b border-border text-left text-xs text-muted">
          <th className="px-4 py-2 font-medium">Branch</th>
          <th className="px-2 py-2 font-medium">Commit</th>
          <th className="px-2 py-2 font-medium">Upstream</th>
          <th className="px-2 py-2 font-medium">Committed</th>
        </tr>
      </thead>
      <tbody className="divide-y divide-border">
        {branches.data.map((b) => (
          <tr key={b.name}>
            <td className="px-4 py-1.5 font-mono text-xs">
              {b.name} {b.name === repo.currentBranch ? <Badge tone="accent">current</Badge> : null}
            </td>
            <td className="px-2 py-1.5 font-mono text-xs">{b.commit.slice(0, 10)}</td>
            <td className="px-2 py-1.5 font-mono text-xs">{b.upstream ?? "—"}</td>
            <td className="px-2 py-1.5 text-xs">{formatTime(b.committedAt)}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function TabPanel({ tab, repo, intel }: { tab: Tab; repo: Repository; intel: RepositoryIntelligence }) {
  switch (tab) {
    case "Overview":
      return <Overview repo={repo} intel={intel} />;
    case "Evidence":
      return <EvidenceTab intel={intel} />;
    case "Runtimes":
      return (
        <div>
          <Conflicts conflicts={intel.conflicts.filter((c) => c.subject.startsWith("runtime."))} />
          <FactTable
            facts={byCategory(intel, "runtime", "package_manager", "workspace")}
            empty="No runtime declarations found"
            label="Runtimes"
          />
        </div>
      );
    case "Dependencies":
      return <DependenciesTab intel={intel} />;
    case "Scripts":
      return <FactTable facts={byCategory(intel, "script")} empty="No declared scripts" label="Scripts" />;
    case "Docker":
      return <FactTable facts={byCategory(intel, "docker")} empty="No Dockerfile or Compose file" label="Docker" />;
    case "CI":
      return <FactTable facts={byCategory(intel, "ci")} empty="No CI configuration found" label="CI" />;
    case "Git":
      return <GitTab repo={repo} />;
    case "Branches":
      return <BranchesTab repo={repo} />;
    case "Pull Requests":
      return (
        <EmptyState icon={<GitPullRequest />} title="GitHub is not connected">
          {repo.githubOwner
            ? `Open pull requests for ${repo.githubOwner}/${repo.githubRepo} appear here once a GitHub account is connected.`
            : "This repository is not linked to a GitHub repository."}
        </EmptyState>
      );
    case "Activity":
      return <ActivityListForEntity entityId={repo.id} />;
  }
}

function Tabs({
  active,
  onChange,
  counts,
}: {
  active: Tab;
  onChange: (t: Tab) => void;
  counts: Partial<Record<Tab, number>>;
}) {
  const refs = useRef<(HTMLButtonElement | null)[]>([]);
  const onKey = (e: KeyboardEvent, index: number) => {
    const delta = e.key === "ArrowRight" ? 1 : e.key === "ArrowLeft" ? -1 : 0;
    if (!delta) return;
    e.preventDefault();
    const next = (index + delta + TABS.length) % TABS.length;
    onChange(TABS[next] as Tab);
    refs.current[next]?.focus();
  };
  return (
    <div
      role="tablist"
      aria-label="Repository sections"
      className="flex gap-1 overflow-x-auto border-b border-border px-2"
    >
      {TABS.map((t, i) => (
        <button
          key={t}
          ref={(el) => {
            refs.current[i] = el;
          }}
          role="tab"
          id={`tab-${i}`}
          aria-selected={t === active}
          aria-controls="repository-tabpanel"
          tabIndex={t === active ? 0 : -1}
          onClick={() => onChange(t)}
          onKeyDown={(e) => onKey(e, i)}
          className={`whitespace-nowrap border-b-2 px-2.5 py-2 text-sm ${t === active ? "border-accent text-fg" : "border-transparent text-muted hover:text-fg"}`}
        >
          {t}
          {counts[t] ? <span className="ml-1 font-mono text-[11px] text-muted">{counts[t]}</span> : null}
        </button>
      ))}
    </div>
  );
}

function ScanControls({ repo, intel }: { repo: Repository; intel: RepositoryIntelligence }) {
  const scan = useRepositoryScan(repo.id);
  const running = intel.scanRunning || scan.start.isPending || scan.progress !== null;
  const failed = scan.outcome?.status === "failed" ? scan.outcome.error : null;
  let note: ReactNode = null;
  if (scan.outcome?.status === "cached") note = "Nothing relevant changed since the last scan; reused it.";
  if (scan.outcome?.status === "cancelled") note = "Scan cancelled.";
  return (
    <div className="flex flex-col items-end gap-1">
      <div className="flex gap-2">
        {running ? (
          <Button size="sm" variant="danger" onClick={() => scan.cancel.mutate()}>
            <Square /> Cancel scan
          </Button>
        ) : (
          <>
            <Button size="sm" variant="primary" onClick={() => scan.start.mutate(false)}>
              <Play /> {intel.scan ? "Scan" : "Run first scan"}
            </Button>
            {intel.scan ? (
              <Button size="sm" onClick={() => scan.start.mutate(true)} title="Ignore the cache and scan again">
                <RefreshCw /> Rescan
              </Button>
            ) : null}
          </>
        )}
      </div>
      {running ? (
        <p className="flex items-center gap-1.5 text-xs text-muted" aria-live="polite">
          <Spinner className="size-3" /> Scanning… {scan.progress ? `${scan.progress.entriesSeen} entries` : ""}
        </p>
      ) : note ? (
        <p className="text-xs text-muted" role="status">
          {note}
        </p>
      ) : null}
      {scan.start.isError ? <ErrorDisplay title="Scan could not start" error={scan.start.error} /> : null}
      {failed ? <ErrorDisplay title="Scan failed" error={failed} /> : null}
    </div>
  );
}

export function RepositoryDetailPage() {
  const { id = "" } = useParams();
  const repo = useRepository(id);
  const intel = useRepositoryIntelligence(id);
  const [tab, setTab] = useState<Tab>("Overview");
  const counts = useMemo<Partial<Record<Tab, number>>>(() => {
    const i = intel.data;
    if (!i) return {};
    const n = (...c: FactCategory[]) => byCategory(i, ...c).length;
    return {
      Evidence: i.facts.length,
      Dependencies: n("dependency"),
      Scripts: n("script"),
      Docker: n("docker"),
      CI: n("ci"),
    };
  }, [intel.data]);

  if (repo.isPending || intel.isPending) {
    return (
      <div className="flex items-center gap-2 p-6 text-muted">
        <Spinner /> Loading repository…
      </div>
    );
  }
  if (repo.isError || intel.isError) {
    return (
      <div className="p-6">
        <ErrorDisplay
          title="Repository could not be loaded"
          error={repo.error ?? intel.error}
          action={
            <Button asChild size="sm">
              <Link to="/repositories">Back to repositories</Link>
            </Button>
          }
        />
      </div>
    );
  }
  const r = repo.data;
  return (
    <div className="flex min-h-full flex-col">
      <div className="flex flex-wrap items-start justify-between gap-4 border-b border-border px-6 py-4">
        <div className="min-w-0">
          <Link to="/repositories" className="mb-1 inline-flex items-center gap-1 text-xs text-muted hover:text-fg">
            <ArrowLeft className="size-3" aria-hidden /> Repositories
          </Link>
          <h1 className="flex flex-wrap items-center gap-2 text-base font-semibold">
            {r.name}
            <Badge tone={TRUST_TONE[r.trustStatus]}>{r.trustStatus}</Badge>
            {r.githubOwner ? (
              <Badge tone="accent">
                {r.githubOwner}/{r.githubRepo}
              </Badge>
            ) : null}
            {intel.data.conflicts.length > 0 ? (
              <Badge tone="danger">{intel.data.conflicts.length} conflicts</Badge>
            ) : null}
          </h1>
          <p className="selectable truncate font-mono text-xs text-muted">{r.localPath ?? r.remoteUrl}</p>
        </div>
        <ScanControls repo={r} intel={intel.data} />
      </div>
      <div className="flex-1 p-6">
        <Card>
          <Tabs active={tab} onChange={setTab} counts={counts} />
          <div role="tabpanel" id="repository-tabpanel" aria-labelledby={`tab-${TABS.indexOf(tab)}`}>
            <TabPanel tab={tab} repo={r} intel={intel.data} />
          </div>
        </Card>
        {intel.data.latestAttempt && intel.data.latestAttempt.status === "failed" ? (
          <div className="mt-4">
            <ErrorDisplay
              title="The most recent scan failed"
              error={{
                code: "INTERNAL_ERROR",
                message: intel.data.latestAttempt.errorMessage ?? "Scan failed.",
                technicalDetails: intel.data.latestAttempt.errorCode,
                suggestedAction: "Run the scan again. If it keeps failing, copy these details into a report.",
                recoverable: true,
              }}
            />
          </div>
        ) : null}
      </div>
    </div>
  );
}
