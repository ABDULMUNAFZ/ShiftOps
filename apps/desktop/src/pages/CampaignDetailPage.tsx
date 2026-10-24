import {
  planCampaign,
  type AgentTask,
  type CampaignRepository,
  type CampaignPlanEvidence,
  type MemberEvidence,
  type PlanEdge,
  type PlannedMember,
} from "@shiftops/core";
import { Badge, Button, Card, CardHeader, EmptyState, Spinner } from "@shiftops/ui";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { ArrowLeft, Play, RefreshCw } from "lucide-react";
import { Fragment, useMemo, useState } from "react";
import { Link, useParams } from "react-router";
import { ErrorDisplay } from "../components/ErrorDisplay";
import { VerificationPanel } from "../features/verification/VerificationPanel";
import { invalidateAfter } from "../lib/invalidation";
import { native } from "../lib/native";
import { useAgentTasks, useCampaign, useCampaignMembers } from "../lib/queries";
import { DefinitionList, Page } from "./Page";

const EDITABLE = new Set(["draft", "planning"]);
const NEXT: Record<string, string> = {
  draft: "planning",
  planning: "ready",
  ready: "running",
  running: "paused",
  paused: "running",
};

function toEvidence(e: CampaignPlanEvidence) {
  const members: MemberEvidence[] = e.members.map((m) => ({
    repositoryId: m.repositoryId,
    name: m.name,
    trustStatus: m.trustStatus,
    ownership: m.ownership,
    hasLocalCheckout: m.hasLocalCheckout,
    uncommittedChanges: m.uncommittedChanges,
    lastScannedAt: m.lastScannedAt,
    facts: m.facts,
    conflicts: m.conflicts,
  }));
  const edges: PlanEdge[] = e.edges.map((d) => ({
    from: d.from,
    to: d.to,
    confidence: d.confidence,
    confirmationStatus: d.confirmationStatus,
    detectionMethod: d.detectionMethod,
  }));
  const cond = (e.campaign.targetCondition ?? {}) as Record<string, unknown>;
  const src = (e.campaign.sourceCondition ?? {}) as Record<string, unknown>;
  return {
    members,
    edges,
    goal: {
      runtime: (cond.runtime ?? src.runtime) as string | undefined,
      sourceVersion: src.major as string | undefined,
      targetVersion: cond.major as string | undefined,
    },
  };
}

export function CampaignDetailPage() {
  const { id = "" } = useParams();
  const client = useQueryClient();
  const campaign = useCampaign(id);
  const members = useCampaignMembers(id, { limit: 200 });
  const tasks = useAgentTasks({ limit: 200 });
  const [plan, setPlan] = useState<ReturnType<typeof planCampaign> | null>(null);
  const [reason, setReason] = useState<Record<string, string>>({});
  const invalidate = () => invalidateAfter(client, "campaigns");

  const useAction = <T,>(fn: () => Promise<T>) =>
    useMutation({
      mutationFn: fn,
      onSuccess: () => invalidate(),
    });

  const setStatus = useAction(() => {
    const next = campaign.data ? NEXT[campaign.data.status] : undefined;
    if (!next) throw new Error("No next status from here.");
    return native.setCampaignStatus(id, next);
  });
  const cancelCampaign = useAction(() => native.setCampaignStatus(id, "cancelled"));
  const compute = useAction(async () => {
    const ev = await native.campaignPlanEvidence(id);
    const input = toEvidence(ev);
    return { ev, p: planCampaign(input.members, input.edges, input.goal) };
  });
  const apply = useAction(async () => {
    if (!plan) throw new Error("Compute a plan first.");
    const mapped = plan.members.map((m: PlannedMember) => ({
      repositoryId: m.repositoryId,
      classification: m.classification,
      riskScore: m.riskScore,
      reasons: [...m.reasons, ...(m.reviewRequired ? ["Requires human review before delegation."] : [])],
      waveNumber: m.blockedReason ? null : m.waveNumber,
      reviewRequired: m.reviewRequired,
    }));
    return native.applyCampaignPlan(id, mapped, true);
  });
  const refreshEdges = useAction(() => native.refreshDependencyEdges());
  const taskByMember = useMemo(() => {
    const map = new Map<string, AgentTask>();
    for (const t of tasks.data?.items ?? []) {
      if (!["completed", "failed", "cancelled"].includes(t.status)) map.set(t.campaignRepositoryId, t);
    }
    return map;
  }, [tasks.data]);

  if (campaign.isPending)
    return (
      <Page title="Campaign" description="">
        <p className="flex items-center gap-2 text-muted">
          <Spinner /> Loading…
        </p>
      </Page>
    );
  if (campaign.isError || !campaign.data)
    return (
      <Page title="Campaign" description="">
        <ErrorDisplay title="Campaign could not be loaded" error={campaign.error} />
      </Page>
    );
  const c = campaign.data;
  const editable = EDITABLE.has(c.status);

  return (
    <Page
      title={c.name}
      description={c.objective}
      actions={
        <div className="flex items-center gap-2">
          <Button
            size="sm"
            variant="ghost"
            onClick={() => void refreshEdges.mutate()}
            disabled={refreshEdges.isPending}
          >
            <RefreshCw /> Refresh edges
          </Button>
          {editable ? (
            <Button
              size="sm"
              onClick={() => void compute.mutateAsync().then((r) => setPlan(r.p))}
              disabled={compute.isPending}
            >
              <Play /> {compute.isPending ? "Computing…" : "Compute plan"}
            </Button>
          ) : null}
          {NEXT[c.status] ? (
            <Button size="sm" onClick={() => void setStatus.mutate()} disabled={setStatus.isPending}>
              → {NEXT[c.status]}
            </Button>
          ) : null}
          {c.status !== "cancelled" && c.status !== "completed" ? (
            <Button size="sm" variant="ghost" className="text-danger" onClick={() => void cancelCampaign.mutate()}>
              Cancel
            </Button>
          ) : null}
        </div>
      }
    >
      <p className="px-1 text-xs text-muted">
        <Link to="/campaigns" className="text-accent hover:underline">
          <ArrowLeft className="inline size-3" /> Campaigns
        </Link>
      </p>
      <Card>
        <CardHeader title="Definition" actions={<Badge>{c.status}</Badge>} />
        <div className="p-4">
          <DefinitionList
            rows={[
              { term: "Migration type", value: c.migrationType },
              { term: "Source condition", value: <code>{JSON.stringify(c.sourceCondition)}</code> },
              { term: "Target condition", value: <code>{JSON.stringify(c.targetCondition)}</code> },
              { term: "Constraints", value: <code>{JSON.stringify(c.constraints)}</code> },
              { term: "Max parallel", value: String(c.maximumParallelTasks) },
              { term: "Repositories", value: String(c.repositoryCount) },
            ]}
          />
        </div>
      </Card>

      {plan ? (
        <Card>
          <CardHeader
            title="Computed rollout plan"
            description={`${plan.members.filter((m) => m.waveNumber > 0).length} orderable, ${plan.members.filter((m) => m.waveNumber === 0).length} blocked, ${plan.usedEdges.length} edges used`}
            actions={
              <Button size="sm" onClick={() => void apply.mutate()} disabled={apply.isPending}>
                Apply plan & mark ready
              </Button>
            }
          />
          {plan.cycle ? (
            <p className="px-4 pt-3 text-sm text-danger">Dependency cycle: {plan.cycle.join(" → ")}</p>
          ) : null}
          {plan.unknownEdges.length ? (
            <p className="px-4 pt-2 text-xs text-muted">
              {plan.unknownEdges.length} inferred edge(s) point outside the campaign and are excluded from ordering.
            </p>
          ) : null}
          <table className="mt-2 w-full text-xs">
            <thead>
              <tr className="border-b border-border text-left text-muted">
                <th className="px-4 py-2 font-medium">Repository</th>
                <th className="px-2 py-2 font-medium">Wave</th>
                <th className="px-2 py-2 font-medium">Class</th>
                <th className="px-2 py-2 font-medium">Risk</th>
                <th className="px-4 py-2 font-medium">Why</th>
              </tr>
            </thead>
            <tbody>
              {plan.members.map((m) => (
                <tr key={m.repositoryId} className="border-b border-border align-top last:border-0">
                  <td className="px-4 py-2 font-medium">
                    {m.name}
                    {m.reviewRequired ? <Badge tone="warn">review</Badge> : null}
                  </td>
                  <td className="px-2 py-2">{m.blockedReason ? "0 (blocked)" : m.waveNumber}</td>
                  <td className="px-2 py-2">{m.classification}</td>
                  <td className="px-2 py-2">{m.riskScore ?? "—"}</td>
                  <td className="px-4 py-2 text-muted">{m.reasons.join(" ")}</td>
                </tr>
              ))}
            </tbody>
          </table>
          {apply.isError ? (
            <div className="p-4">
              <ErrorDisplay title="Plan could not be applied" error={apply.error} />
            </div>
          ) : null}
        </Card>
      ) : null}

      <Card>
        <CardHeader title="Members" />
        {members.isPending ? (
          <p className="flex items-center gap-2 p-4 text-muted">
            <Spinner /> Loading…
          </p>
        ) : members.isError ? (
          <ErrorDisplay title="Members could not be listed" error={members.error} />
        ) : members.data.items.length === 0 ? (
          <EmptyState title="No repositories in this campaign" />
        ) : (
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-border text-left text-xs text-muted">
                <th className="px-4 py-2 font-medium">Repository</th>
                <th className="px-2 py-2 font-medium">Class</th>
                <th className="px-2 py-2 font-medium">Risk</th>
                <th className="px-2 py-2 font-medium">Wave</th>
                <th className="px-2 py-2 font-medium">Status</th>
                <th className="px-4 py-2 font-medium text-right">Actions</th>
              </tr>
            </thead>
            <tbody>
              {members.data.items.map((m) => (
                <MemberRow
                  key={m.id}
                  member={m}
                  task={taskByMember.get(m.id) ?? null}
                  reason={reason[m.id] ?? ""}
                  onReason={(v) => setReason((r) => ({ ...r, [m.id]: v }))}
                  onChanged={invalidate}
                />
              ))}
            </tbody>
          </table>
        )}
      </Card>
      {(setStatus.isError || compute.isError || refreshEdges.isError || cancelCampaign.isError) && (
        <ErrorDisplay
          title="Campaign action failed"
          error={setStatus.error ?? compute.error ?? refreshEdges.error ?? cancelCampaign.error}
        />
      )}
    </Page>
  );
}

function MemberRow({
  member,
  task,
  reason,
  onReason,
  onChanged,
}: {
  member: CampaignRepository;
  task: AgentTask | null;
  reason: string;
  onReason: (v: string) => void;
  onChanged: () => Promise<void>;
}) {
  const client = useQueryClient();
  const useAction = (fn: () => Promise<unknown>) =>
    useMutation({
      mutationFn: fn,
      onSuccess: async () => {
        await invalidateAfter(client, "campaigns");
        await onChanged();
      },
    });
  const exclude = useAction(() => native.excludeCampaignRepository(member.id, reason || "Excluded by reviewer"));
  const include = useAction(() => native.includeCampaignRepository(member.id));
  const resolveReview = useAction(() => native.resolveMemberReview(member.id));
  const createTask = useAction(() => native.createAgentTask(member.id));
  const [verifying, setVerifying] = useState(false);
  const error = exclude.error ?? include.error ?? resolveReview.error ?? createTask.error;
  return (
    <Fragment>
      <tr className="border-b border-border align-top last:border-0">
        <td className="px-4 py-2 font-medium">
          <Link className="text-accent hover:underline" to={`/repositories/${member.repositoryId}`}>
            {member.repositoryName}
          </Link>
        </td>
        <td className="px-2 py-2 text-xs">{member.classification}</td>
        <td className="px-2 py-2 text-xs">{member.riskScore ?? "—"}</td>
        <td className="px-2 py-2 text-xs">{member.waveNumber ?? "—"}</td>
        <td className="px-2 py-2">
          <Badge tone={member.status === "excluded" ? "danger" : member.status === "verified" ? "ok" : "neutral"}>
            {member.status}
          </Badge>
          {member.reviewRequired ? <Badge tone="warn">review</Badge> : null}
        </td>
        <td className="px-4 py-2 text-right">
          <div className="flex flex-wrap items-center justify-end gap-2 text-xs">
            {member.reviewRequired ? (
              <Button
                size="sm"
                variant="ghost"
                onClick={() => void resolveReview.mutate()}
                disabled={resolveReview.isPending}
              >
                Clear review
              </Button>
            ) : null}
            {member.status === "excluded" ? (
              <Button size="sm" variant="ghost" onClick={() => void include.mutate()} disabled={include.isPending}>
                Include
              </Button>
            ) : (
              <>
                <input
                  className="h-7 w-36 rounded-md border border-border bg-surface px-2 text-xs text-fg"
                  placeholder="Exclusion reason"
                  value={reason}
                  onChange={(e) => onReason(e.target.value)}
                />
                <Button
                  size="sm"
                  variant="ghost"
                  className="text-danger"
                  onClick={() => void exclude.mutate()}
                  disabled={exclude.isPending}
                >
                  Exclude
                </Button>
              </>
            )}
            {member.status !== "excluded" ? (
              <Button size="sm" variant="ghost" aria-expanded={verifying} onClick={() => setVerifying((v) => !v)}>
                {verifying ? "Hide verification" : "Verify"}
              </Button>
            ) : null}
            {task ? (
              <Badge tone="accent">
                {task.taskKind === "repair" ? "repair" : "task"} {task.status}
              </Badge>
            ) : (
              <Button
                size="sm"
                variant="ghost"
                onClick={() => void createTask.mutate()}
                disabled={createTask.isPending}
              >
                Draft task
              </Button>
            )}
          </div>
          {error ? (
            <p className="mt-1 text-xs text-danger">{String((error as { message?: string }).message ?? error)}</p>
          ) : null}
        </td>
      </tr>
      {verifying ? (
        <tr className="border-b border-border bg-surface-2/40">
          <td colSpan={6}>
            <VerificationPanel memberId={member.id} />
          </td>
        </tr>
      ) : null}
    </Fragment>
  );
}
