import type { DiagnosticKind } from "@shiftops/core";
import { Badge, Button, Card, CardHeader, EmptyState, type BadgeTone } from "@shiftops/ui";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Play, RefreshCw, Square, Terminal } from "lucide-react";
import { useEffect, useRef } from "react";
import { CopyButton, ErrorDisplay } from "../../components/ErrorDisplay";
import { native } from "../../lib/native";
import { processStore, useProcessRuns, type ProcessStore, type RunView } from "../../lib/processStore";
import { queryKeys } from "../../lib/queryKeys";

const ACTIONS: { kind: DiagnosticKind; label: string }[] = [
  { kind: "git_version", label: "git --version" },
  { kind: "node_version", label: "node --version" },
  { kind: "pnpm_version", label: "pnpm --version" },
  { kind: "long_running", label: "Start long-running diagnostic" },
];

/** Lines rendered per run; the store keeps more, the log file keeps everything up to the limit. */
const RENDERED_LINES = 500;

function statusOf(run: RunView): { label: string; tone: BadgeTone } {
  if (!run.result) return { label: "running", tone: "accent" };
  switch (run.result.status) {
    case "succeeded":
      return { label: "succeeded", tone: "ok" };
    case "cancelled":
      return { label: "cancelled", tone: "warn" };
    case "timed_out":
      return { label: "timed out", tone: "danger" };
    default:
      return { label: "failed", tone: "danger" };
  }
}

export function RunPanel({ run }: { run: RunView }) {
  const cancel = useMutation({ mutationFn: () => native.cancelProcess(run.runId) });
  const outputRef = useRef<HTMLPreElement>(null);
  const status = statusOf(run);
  const visible = run.lines.slice(-RENDERED_LINES);
  const hidden = run.droppedLines + (run.lines.length - visible.length);

  useEffect(() => {
    const el = outputRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [run.lines.length]);

  return (
    <li className="space-y-2 px-4 py-3" data-run={run.runId}>
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="min-w-0">
          <p className="font-medium">{run.label}</p>
          <p className="truncate font-mono text-[11px] text-muted" title={`${run.program} ${run.args.join(" ")}`}>
            run {run.runId} · pid {run.pid ?? "n/a"}
          </p>
        </div>
        <div className="flex items-center gap-2">
          <Badge tone={status.tone}>{status.label}</Badge>
          {!run.result ? (
            <Button
              size="sm"
              variant="danger"
              onClick={() => cancel.mutate()}
              disabled={cancel.isPending}
              aria-label={`Cancel ${run.label}`}
            >
              <Square /> {cancel.isPending ? "Cancelling…" : "Cancel"}
            </Button>
          ) : null}
        </div>
      </div>
      <pre
        ref={outputRef}
        aria-label={`Output of ${run.label}`}
        className="selectable max-h-56 overflow-auto rounded bg-surface-2 p-2 font-mono text-xs leading-5"
      >
        {hidden > 0 ? (
          <span className="text-muted">
            [{hidden} earlier lines not shown; see the run log]{"\n"}
          </span>
        ) : null}
        {visible.map((line, i) => (
          <span key={i} className={line.stream === "stderr" ? "text-warn" : undefined}>
            {line.text}
            {"\n"}
          </span>
        ))}
        {visible.length === 0 && !run.result ? <span className="text-muted">Waiting for output…</span> : null}
      </pre>
      {cancel.isError ? <ErrorDisplay title="Cancellation failed" error={cancel.error} /> : null}
      {run.result ? (
        <details>
          <summary className="cursor-pointer text-xs text-muted">
            Structured result: exit {run.result.exitCode ?? "none"} · {run.result.durationMs} ms ·{" "}
            {run.result.outputBytes} bytes{run.result.outputTruncated ? " (truncated)" : ""}
          </summary>
          <pre className="selectable mt-2 max-h-64 overflow-auto rounded bg-surface-2 p-2 font-mono text-xs">
            {JSON.stringify(run.result, null, 2)}
          </pre>
          <CopyButton text={JSON.stringify(run.result, null, 2)} label="Copy result" />
        </details>
      ) : null}
    </li>
  );
}

/**
 * Fixed, safe diagnostics. There is deliberately no free-form command input:
 * each button maps to one command defined in Rust.
 */
export function NativeDiagnostics({ store = processStore }: { store?: ProcessStore }) {
  const client = useQueryClient();
  const runs = useProcessRuns(store);
  const start = useMutation({ mutationFn: (kind: DiagnosticKind) => native.startDiagnostic(kind) });

  return (
    <Card>
      <CardHeader
        title={
          <span className="flex items-center gap-2">
            <Terminal className="size-4" aria-hidden /> Native diagnostics
          </span>
        }
        description="Runs fixed commands through the native process runner with streaming output, timeout and cancellation."
      />
      <div className="flex flex-wrap gap-2 border-b border-border px-4 py-3">
        <Button size="sm" onClick={() => void client.refetchQueries({ queryKey: queryKeys.environment })}>
          <RefreshCw /> Re-run environment checks
        </Button>
        {ACTIONS.map((a) => (
          <Button key={a.kind} size="sm" onClick={() => start.mutate(a.kind)} disabled={start.isPending}>
            <Play /> {a.label}
          </Button>
        ))}
      </div>
      {start.isError ? (
        <div className="p-4">
          <ErrorDisplay title="Diagnostic not started" error={start.error} />
        </div>
      ) : null}
      {runs.length === 0 ? (
        <EmptyState icon={<Terminal />} title="No diagnostics run in this session">
          Output streams here as the command runs. Each run is also recorded in the Activity Centre.
        </EmptyState>
      ) : (
        <ul className="divide-y divide-border" aria-label="Diagnostic runs">
          {runs.map((run) => (
            <RunPanel key={run.runId} run={run} />
          ))}
        </ul>
      )}
    </Card>
  );
}
