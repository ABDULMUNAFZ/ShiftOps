import type { Repository } from "@shiftops/core";
import { Badge, Button, Card, EmptyState, Spinner } from "@shiftops/ui";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Plus, Workflow } from "lucide-react";
import { useState, type FormEvent } from "react";
import { Link, useNavigate } from "react-router";
import { ErrorDisplay } from "../components/ErrorDisplay";
import { invalidateAfter } from "../lib/invalidation";
import { native } from "../lib/native";
import { useCampaigns, useRepositories } from "../lib/queries";
import { Page } from "./Page";

const TYPES = ["runtime_upgrade", "dependency_replace", "vulnerability_fix", "platform_standard"];

function CreateForm({ repositories, onDone }: { repositories: Repository[]; onDone: () => void }) {
  const client = useQueryClient();
  const navigate = useNavigate();
  const [name, setName] = useState("");
  const [objective, setObjective] = useState("Upgrade Node.js 18 to Node.js 22");
  const [migrationType, setMigrationType] = useState("runtime_upgrade");
  const [source, setSource] = useState("18");
  const [target, setTarget] = useState("22");
  const [runtime, setRuntime] = useState("node");
  const [deadline, setDeadline] = useState("");
  const [maxParallel, setMaxParallel] = useState(1);
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const create = useMutation({
    mutationFn: () =>
      native.createCampaign(
        {
          name,
          objective,
          migrationType,
          sourceCondition: runtime ? { runtime, major: source } : {},
          targetCondition: runtime ? { runtime, major: target } : {},
          constraints: { ...(deadline ? { deadline } : {}) },
          verificationProfile: "default",
          maximumParallelTasks: maxParallel,
        },
        [...picked],
      ),
    onSuccess: async (c) => {
      await invalidateAfter(client, "campaigns");
      onDone();
      void navigate(`/campaigns/${c.id}`);
    },
  });
  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (!create.isPending) create.mutate();
  };
  const field = "h-8 w-full rounded-md border border-border bg-surface px-2 text-sm text-fg";
  return (
    <Card>
      <form onSubmit={submit} className="grid gap-3 p-4 text-sm md:grid-cols-2">
        <label className="grid gap-1">
          Name
          <input
            required
            maxLength={200}
            className={field}
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="Node 22 rollout"
          />
        </label>
        <label className="grid gap-1">
          Migration type
          <select className={field} value={migrationType} onChange={(e) => setMigrationType(e.target.value)}>
            {TYPES.map((t) => (
              <option key={t} value={t}>
                {t}
              </option>
            ))}
          </select>
        </label>
        <label className="grid gap-1 md:col-span-2">
          Objective
          <input required className={field} value={objective} onChange={(e) => setObjective(e.target.value)} />
        </label>
        <label className="grid gap-1">
          Runtime key
          <input className={field} value={runtime} onChange={(e) => setRuntime(e.target.value)} placeholder="node" />
        </label>
        <div className="grid grid-cols-2 gap-3">
          <label className="grid gap-1">
            From major
            <input className={field} value={source} onChange={(e) => setSource(e.target.value)} placeholder="18" />
          </label>
          <label className="grid gap-1">
            To major
            <input className={field} value={target} onChange={(e) => setTarget(e.target.value)} placeholder="22" />
          </label>
        </div>
        <label className="grid gap-1">
          Deadline (optional)
          <input type="date" className={field} value={deadline} onChange={(e) => setDeadline(e.target.value)} />
        </label>
        <label className="grid gap-1">
          Max parallel tasks
          <input
            type="number"
            min={1}
            max={100}
            className={field}
            value={maxParallel}
            onChange={(e) => setMaxParallel(Number(e.target.value) || 1)}
          />
        </label>
        <fieldset className="md:col-span-2">
          <legend className="mb-1 text-xs text-muted">Repositories ({picked.size} selected)</legend>
          <div className="max-h-56 overflow-auto rounded-md border border-border">
            {repositories.map((r) => (
              <label
                key={r.id}
                className="flex items-center gap-2 border-b border-border px-3 py-1.5 text-xs last:border-0"
              >
                <input
                  type="checkbox"
                  checked={picked.has(r.id)}
                  onChange={(e) => {
                    const next = new Set(picked);
                    if (e.target.checked) next.add(r.id);
                    else next.delete(r.id);
                    setPicked(next);
                  }}
                />
                <span className="font-medium">{r.name}</span>
                <span className="text-muted">{r.localPath ?? r.remoteUrl ?? "metadata only"}</span>
                <Badge tone={r.trustStatus === "trusted" ? "ok" : r.trustStatus === "blocked" ? "danger" : "warn"}>
                  {r.trustStatus}
                </Badge>
              </label>
            ))}
          </div>
        </fieldset>
        {create.isError ? (
          <div className="md:col-span-2">
            <ErrorDisplay title="Campaign could not be created" error={create.error} />
          </div>
        ) : null}
        <div className="flex gap-2 md:col-span-2">
          <Button
            type="submit"
            size="sm"
            disabled={!name.trim() || !objective.trim() || picked.size === 0 || create.isPending}
          >
            Create campaign
          </Button>
          <Button type="button" size="sm" variant="ghost" onClick={onDone}>
            Cancel
          </Button>
        </div>
      </form>
    </Card>
  );
}

export function CampaignsPage() {
  const campaigns = useCampaigns({});
  const repositories = useRepositories({ limit: 200 });
  const [creating, setCreating] = useState(false);
  const repos = repositories.data?.items ?? [];
  return (
    <Page
      title="Campaigns"
      description="One codebase-wide change, planned across every affected repository."
      actions={
        <Button size="sm" onClick={() => setCreating((v) => !v)} disabled={repos.length === 0}>
          <Plus /> New campaign
        </Button>
      }
    >
      {creating ? <CreateForm repositories={repos} onDone={() => setCreating(false)} /> : null}
      <Card>
        {campaigns.isPending ? (
          <p className="flex items-center gap-2 p-4 text-muted">
            <Spinner /> Loading…
          </p>
        ) : campaigns.isError ? (
          <ErrorDisplay title="Campaigns could not be listed" error={campaigns.error} />
        ) : campaigns.data.items.length === 0 ? (
          <EmptyState icon={<Workflow />} title="No campaigns">
            Select repositories and a migration goal; ShiftOps plans waves and drafts Devin tasks.
          </EmptyState>
        ) : (
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-border text-left text-xs text-muted">
                <th className="px-4 py-2 font-medium">Name</th>
                <th className="px-2 py-2 font-medium">Status</th>
                <th className="px-2 py-2 font-medium">Type</th>
                <th className="px-2 py-2 font-medium">Repositories</th>
              </tr>
            </thead>
            <tbody>
              {campaigns.data.items.map((c) => (
                <tr key={c.id} className="border-b border-border last:border-0">
                  <td className="px-4 py-2">
                    <Link className="font-medium text-accent hover:underline" to={`/campaigns/${c.id}`}>
                      {c.name}
                    </Link>
                  </td>
                  <td className="px-2 py-2">
                    <Badge>{c.status}</Badge>
                  </td>
                  <td className="px-2 py-2 font-mono text-xs">{c.migrationType}</td>
                  <td className="px-2 py-2">{c.repositoryCount}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </Card>
    </Page>
  );
}
