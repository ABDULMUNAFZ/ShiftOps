import type { AgentTask } from "@shiftops/core";
import { Badge, Button, Card, EmptyState, Spinner } from "@shiftops/ui";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Bot, ChevronDown, ChevronRight, OctagonX, Play, ShieldCheck } from "lucide-react";
import { useState } from "react";
import { ErrorDisplay } from "../components/ErrorDisplay";
import { invalidateAfter } from "../lib/invalidation";
import { native } from "../lib/native";
import { useAgentRuns, useAgentTasks, useDevinStatus } from "../lib/queries";
import { Page } from "./Page";

const TONE: Record<string, "ok" | "warn" | "danger" | "accent"> = {
  running: "accent",
  awaiting_review: "warn",
  completed: "ok",
  failed: "danger",
  cancelled: "danger",
};

function TaskCard({ task }: { task: AgentTask }) {
  const client = useQueryClient();
  const [open, setOpen] = useState(false);
  const runs = useAgentRuns(open ? task.id : null);
  const invalidate = () => invalidateAfter(client, "devin");
  const approve = useMutation({ mutationFn: () => native.approveAgentTask(task.id), onSuccess: invalidate });
  const dispatch = useMutation({ mutationFn: () => native.dispatchAgentTask(task.id), onSuccess: invalidate });
  const cancel = useMutation({ mutationFn: () => native.cancelAgentTask(task.id), onSuccess: invalidate });
  const error = approve.error ?? dispatch.error ?? cancel.error;
  const pending = approve.isPending || dispatch.isPending || cancel.isPending;
  const [confirming, setConfirming] = useState<"approve" | "dispatch" | null>(null);
  return (
    <Card>
      <button
        type="button"
        className="flex w-full items-center gap-2 px-4 py-2 text-left"
        onClick={() => setOpen((v) => !v)}
      >
        {open ? <ChevronDown className="size-4" /> : <ChevronRight className="size-4" />}
        <span className="font-mono text-xs">{task.id.slice(0, 8)}</span>
        <Badge tone={TONE[task.status] ?? "neutral"}>{task.status}</Badge>
        {task.taskKind === "repair" ? <Badge tone="accent">repair</Badge> : null}
        {task.approvedAt ? <Badge tone="ok">approved</Badge> : <Badge>unapproved</Badge>}
        <span className="ml-auto text-xs text-muted">{task.createdAt.slice(0, 16).replace("T", " ")}</span>
      </button>
      {open ? (
        <div className="border-t border-border p-4">
          <div className="flex flex-wrap items-center gap-2 pb-3">
            {confirming ? (
              <div role="alertdialog" aria-label="Confirm task action" className="flex flex-wrap items-center gap-2">
                <span className="text-xs">
                  {confirming === "approve"
                    ? "Approve exactly these instructions (hash below) for dispatch?"
                    : "Devin will change files in this repository's checkout and use your Devin account. Dispatch now?"}
                </span>
                <Button
                  size="sm"
                  variant="primary"
                  disabled={pending}
                  onClick={() => {
                    const action = confirming === "approve" ? approve : dispatch;
                    setConfirming(null);
                    action.mutate();
                  }}
                >
                  Confirm
                </Button>
                <Button size="sm" onClick={() => setConfirming(null)}>
                  Keep
                </Button>
              </div>
            ) : null}
            {!confirming && !task.approvedAt && task.status === "pending" ? (
              <Button size="sm" onClick={() => setConfirming("approve")} disabled={pending}>
                <ShieldCheck /> Approve
              </Button>
            ) : null}
            {!confirming && task.approvedAt && task.status === "pending" ? (
              <Button size="sm" onClick={() => setConfirming("dispatch")} disabled={pending}>
                <Play /> Dispatch to Devin
              </Button>
            ) : null}
            {task.status === "running" ? (
              <Button
                size="sm"
                variant="ghost"
                className="text-danger"
                onClick={() => void cancel.mutate()}
                disabled={pending}
              >
                <OctagonX /> Cancel
              </Button>
            ) : null}
            <span className="text-[11px] text-muted">hash {task.instructionsHash?.slice(0, 12) ?? "—"}</span>
          </div>
          {task.taskKind === "repair" ? (
            <p className="pb-2 text-xs text-muted">
              Drafted from a failed verification. When the session succeeds, ShiftOps re-runs the same approved checks,
              and only if the plan, including script bodies, is unchanged.
            </p>
          ) : null}
          {error ? <ErrorDisplay title="Task action failed" error={error} /> : null}
          <pre className="max-h-96 overflow-auto whitespace-pre-wrap rounded-md border border-border bg-surface-2 p-3 text-xs">
            {task.instructionsMarkdown}
          </pre>
          {runs.data ? (
            runs.data.items.length ? (
              <ul className="mt-3 text-xs text-muted">
                {runs.data.items.map((r) => (
                  <li key={r.id}>
                    run {r.id.slice(0, 8)} — {r.status}
                    {r.exitCode != null ? ` (exit ${r.exitCode})` : ""} — {r.startedAt.slice(0, 19).replace("T", " ")}
                  </li>
                ))}
              </ul>
            ) : (
              <p className="mt-3 text-xs text-muted">No dispatch runs recorded.</p>
            )
          ) : null}
        </div>
      ) : null}
    </Card>
  );
}

export function AgentRunsPage() {
  const tasks = useAgentTasks({ limit: 200 });
  const devin = useDevinStatus();
  return (
    <Page title="Agent Runs" description="Repository-level implementation sessions delegated to Devin.">
      {devin.data && !(devin.data.installed && devin.data.authenticated) ? (
        <Card>
          <p className="p-4 text-sm text-warn">{devin.data.detail} Task drafts still work; dispatch is disabled.</p>
        </Card>
      ) : null}
      {tasks.isPending ? (
        <p className="flex items-center gap-2 text-muted">
          <Spinner /> Loading…
        </p>
      ) : tasks.isError ? (
        <ErrorDisplay title="Tasks could not be listed" error={tasks.error} />
      ) : tasks.data.items.length === 0 ? (
        <EmptyState icon={<Bot />} title="No agent runs">
          Draft a task from a campaign member; approve and dispatch it here.
        </EmptyState>
      ) : (
        <div className="grid gap-3">
          {tasks.data.items.map((t) => (
            <TaskCard key={t.id} task={t} />
          ))}
        </div>
      )}
    </Page>
  );
}
