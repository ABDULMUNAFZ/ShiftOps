import type { VerificationCheck, VerificationCheckStatus, VerificationRun } from "@shiftops/core";
import { Badge, Button, EmptyState, Spinner, type BadgeTone } from "@shiftops/ui";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Play, ShieldCheck, Square, Wrench } from "lucide-react";
import { useEffect, useId, useState } from "react";
import { ErrorDisplay } from "../../components/ErrorDisplay";
import { invalidateAfter } from "../../lib/invalidation";
import { native, onVerificationEvent } from "../../lib/native";
import { queryKeys } from "../../lib/queryKeys";

const CHECK_TONE: Record<VerificationCheckStatus, BadgeTone> = {
  pending: "neutral",
  running: "accent",
  passed: "ok",
  failed: "danger",
  skipped: "neutral",
  error: "danger",
  cancelled: "warn",
};
const RUN_TONE: Record<VerificationRun["status"], BadgeTone> = {
  queued: "neutral",
  running: "accent",
  passed: "ok",
  failed: "danger",
  error: "danger",
  cancelled: "warn",
};

function evidenceOf(check: VerificationCheck) {
  const e = (check.evidence ?? {}) as { actionable?: boolean; reason?: string; affectedFiles?: string[] };
  return { actionable: e.actionable === true, reason: e.reason ?? null, files: e.affectedFiles ?? [] };
}

function Checks({ run }: { run: VerificationRun }) {
  const checks = useQuery({
    queryKey: queryKeys.verificationRuns.checks(run.id),
    queryFn: () => native.listVerificationChecks(run.id),
  });
  if (checks.isPending)
    return (
      <p className="flex items-center gap-2 text-xs text-muted">
        <Spinner /> Loading checks…
      </p>
    );
  if (checks.isError)
    return (
      <ErrorDisplay
        title="Checks could not be loaded"
        error={checks.error}
        action={
          <Button size="sm" onClick={() => void checks.refetch()}>
            Retry
          </Button>
        }
      />
    );
  if (checks.data.length === 0) return <p className="text-xs text-muted">No checks were recorded for this run.</p>;
  return (
    <ul className="space-y-2" aria-label="Verification checks">
      {checks.data.map((c) => {
        const ev = evidenceOf(c);
        return (
          <li key={c.id} className="rounded-md border border-border p-2 text-xs">
            <div className="flex flex-wrap items-center gap-2">
              <span className="font-mono font-medium">{c.name}</span>
              <Badge tone={CHECK_TONE[c.status]}>{c.status}</Badge>
              {c.exitCode !== null ? <span className="text-muted">exit {c.exitCode}</span> : null}
              {c.durationMs !== null ? <span className="text-muted">{(c.durationMs / 1000).toFixed(1)} s</span> : null}
              {c.status === "failed" ? (
                <Badge tone={ev.actionable ? "accent" : "warn"}>{ev.actionable ? "repairable" : "fix by hand"}</Badge>
              ) : null}
            </div>
            {c.suggestedAction ? <p className="mt-1 text-muted">{c.suggestedAction}</p> : null}
            {ev.files.length > 0 ? (
              <p className="selectable mt-1 font-mono text-[11px]">Files: {ev.files.join(", ")}</p>
            ) : null}
            {c.redactedOutput ? (
              <details className="mt-1">
                <summary className="cursor-pointer text-muted">Output</summary>
                <pre className="selectable mt-1 max-h-64 overflow-auto whitespace-pre-wrap rounded bg-surface-2 p-2 font-mono text-[11px]">
                  {c.redactedOutput}
                </pre>
              </details>
            ) : null}
          </li>
        );
      })}
    </ul>
  );
}

function LiveStatus({ status }: { status: VerificationCheckStatus | undefined }) {
  return status ? <Badge tone={CHECK_TONE[status]}>{status}</Badge> : null;
}

/**
 * Independent verification of one campaign member: review the exact plan,
 * confirm, run, read real results, and draft a bounded repair task.
 */
export function VerificationPanel({ memberId }: { memberId: string }) {
  const client = useQueryClient();
  const confirmId = useId();
  const [confirmed, setConfirmed] = useState(false);
  const [live, setLive] = useState<Record<string, VerificationCheckStatus>>({});
  const preview = useQuery({
    queryKey: queryKeys.verificationRuns.preview(memberId),
    queryFn: () => native.previewVerification(memberId),
  });

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let active = true;
    void onVerificationEvent((event) => {
      if (event.memberId !== memberId) return;
      if (event.type === "step") setLive((s) => ({ ...s, [event.name]: event.status }));
      else {
        setLive({});
        void invalidateAfter(client, "verification");
      }
    }).then((u) => {
      if (active) unlisten = u;
      else u();
    });
    return () => {
      active = false;
      unlisten?.();
    };
  }, [client, memberId]);

  const start = useMutation({
    mutationFn: (hash: string) => native.startVerification(memberId, hash),
    onSuccess: async () => {
      setConfirmed(false);
      setLive({});
      await invalidateAfter(client, "verification");
    },
  });
  const cancel = useMutation({ mutationFn: () => native.cancelVerification(memberId) });
  const repair = useMutation({
    mutationFn: (runId: string) => native.createRepairTask(memberId, runId),
    onSuccess: () => invalidateAfter(client, "devin"),
  });

  if (preview.isPending)
    return (
      <p className="flex items-center gap-2 p-3 text-xs text-muted">
        <Spinner /> Reading the verification plan…
      </p>
    );
  if (preview.isError)
    return (
      <div className="p-3">
        <ErrorDisplay
          title="Verification plan could not be read"
          error={preview.error}
          action={
            <Button size="sm" onClick={() => void preview.refetch()}>
              Retry
            </Button>
          }
        />
      </div>
    );
  const p = preview.data;
  const running = p.running || start.isPending || Object.keys(live).length > 0;
  const plan = p.plan;
  const latest = p.latestRun;
  const actionable = Array.isArray((latest?.summary as { actionableFailures?: unknown })?.actionableFailures)
    ? ((latest?.summary as { actionableFailures: string[] }).actionableFailures ?? [])
    : [];
  const canRepair =
    latest?.status === "failed" && actionable.length > 0 && p.repairAttempts < p.maxRepairAttempts && !running;

  return (
    <div className="space-y-3 p-3 text-sm" aria-label="Verification">
      {p.problems.length > 0 ? (
        <ul
          className="space-y-1 rounded-md border border-warn/40 bg-warn/8 p-2 text-xs"
          aria-label="Verification blockers"
        >
          {p.problems.map((m) => (
            <li key={m}>{m}</li>
          ))}
        </ul>
      ) : null}

      {plan && plan.supported ? (
        <div>
          <h4 className="mb-1 text-xs font-semibold uppercase tracking-wide text-muted">
            Plan ({plan.packageManager}
            {plan.lockfile ? `, ${plan.lockfile}` : ", no lockfile"}) ·{" "}
            <span className="font-mono normal-case">{plan.hash?.slice(0, 12)}</span>
          </h4>
          <table className="w-full text-xs" aria-label="Verification plan">
            <tbody>
              {plan.steps.map((s) => (
                <tr key={s.name} className="border-b border-border last:border-0">
                  <td className="py-1 pr-2 font-mono">{s.name}</td>
                  <td className="py-1 pr-2 font-mono">{[s.program, ...s.args].join(" ")}</td>
                  <td className="selectable py-1 font-mono text-muted">{s.script ?? "—"}</td>
                  <td className="py-1 pl-2">
                    <LiveStatus status={live[s.name]} />
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          {plan.warnings.length > 0 ? (
            <ul className="mt-1 text-xs text-warn">
              {plan.warnings.map((w) => (
                <li key={w}>{w}</li>
              ))}
            </ul>
          ) : null}
        </div>
      ) : null}

      <div className="flex flex-wrap items-center gap-3">
        {running ? (
          <>
            <span className="flex items-center gap-2 text-xs text-muted" aria-live="polite">
              <Spinner /> Verifying…
            </span>
            <Button size="sm" variant="danger" onClick={() => void cancel.mutate()} disabled={cancel.isPending}>
              <Square /> Cancel
            </Button>
          </>
        ) : plan?.supported && plan.hash && p.problems.length === 0 ? (
          <>
            <label htmlFor={confirmId} className="flex items-center gap-2 text-xs">
              <input
                id={confirmId}
                type="checkbox"
                checked={confirmed}
                onChange={(e) => setConfirmed(e.target.checked)}
              />
              I reviewed these commands. They run this repository&apos;s own scripts on this computer.
            </label>
            <Button
              size="sm"
              variant="primary"
              disabled={!confirmed || start.isPending}
              onClick={() => plan.hash && start.mutate(plan.hash)}
            >
              <Play /> Run verification
            </Button>
          </>
        ) : null}
      </div>
      {start.isError ? <ErrorDisplay title="Verification did not start" error={start.error} /> : null}
      {cancel.isError ? <ErrorDisplay title="Cancellation failed" error={cancel.error} /> : null}

      {latest ? (
        <div className="space-y-2 border-t border-border pt-2">
          <p className="flex flex-wrap items-center gap-2 text-xs">
            <ShieldCheck className="size-3.5" aria-hidden /> Latest run
            <Badge tone={RUN_TONE[latest.status]}>{latest.status}</Badge>
            <span className="font-mono text-muted">{latest.verifiedCommitSha.slice(0, 10)}</span>
            {latest.trigger === "after_repair" ? <Badge tone="accent">after repair</Badge> : null}
            <span className="text-muted">{new Date(latest.startedAt).toLocaleString()}</span>
          </p>
          <Checks run={latest} />
          {latest.status === "failed" ? (
            <div className="flex flex-wrap items-center gap-2 text-xs">
              {canRepair ? (
                <Button size="sm" onClick={() => repair.mutate(latest.id)} disabled={repair.isPending}>
                  <Wrench /> Draft repair task ({p.repairAttempts + 1} of {p.maxRepairAttempts})
                </Button>
              ) : p.repairAttempts >= p.maxRepairAttempts ? (
                <span className="text-muted">Repair limit reached; continue by hand.</span>
              ) : actionable.length === 0 ? (
                <span className="text-muted">No failure is safely repairable; fix it by hand.</span>
              ) : null}
              {repair.isSuccess ? (
                <span role="status" className="text-muted">
                  Repair task drafted. Review, approve and dispatch it on Agent Runs.
                </span>
              ) : null}
            </div>
          ) : null}
          {repair.isError ? <ErrorDisplay title="Repair task not drafted" error={repair.error} /> : null}
        </div>
      ) : (
        <EmptyState title="Not verified yet">
          Verification runs only the plan above, after you confirm it, and records each command&apos;s real exit code.
        </EmptyState>
      )}
    </div>
  );
}
