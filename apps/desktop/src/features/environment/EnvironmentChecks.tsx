import type { CheckStatus, EnvironmentCheck, EnvironmentReport } from "@shiftops/core";
import { REQUIRED_CHECKS } from "@shiftops/core";
import { Badge, type BadgeTone, StatusDot } from "@shiftops/ui";

const STATUS: Record<CheckStatus, { tone: BadgeTone; label: string }> = {
  available: { tone: "ok", label: "Available" },
  unavailable: { tone: "warn", label: "Unavailable" },
  error: { tone: "danger", label: "Error" },
};

export function EnvironmentCheckRow({ check }: { check: EnvironmentCheck }) {
  const status = STATUS[check.status];
  const required = REQUIRED_CHECKS.includes(check.id);
  return (
    <li className="grid grid-cols-[1rem_11rem_1fr_auto] items-start gap-3 px-4 py-2.5" data-check={check.id}>
      <span className="pt-1.5">
        <StatusDot tone={status.tone} label={status.label} />
      </span>
      <div className="min-w-0">
        <p className="font-medium">{check.label}</p>
        {required ? <p className="text-[11px] text-muted">Required</p> : null}
      </div>
      <div className="selectable min-w-0 space-y-1">
        <pre className="whitespace-pre-wrap break-words font-mono text-xs text-fg/90">{check.detail}</pre>
        {check.path ? (
          <p className="truncate font-mono text-[11px] text-muted" title={check.path}>
            {check.path}
          </p>
        ) : null}
        {check.command ? (
          <p className="truncate font-mono text-[11px] text-muted" title={check.command}>
            $ {check.command}
          </p>
        ) : null}
      </div>
      <div className="flex flex-col items-end gap-1">
        <Badge tone={status.tone}>{status.label}</Badge>
        {check.version ? <span className="selectable font-mono text-xs">{check.version}</span> : null}
        <span className="text-[11px] tabular-nums text-muted">{check.durationMs} ms</span>
      </div>
    </li>
  );
}

export function EnvironmentCheckList({ report }: { report: EnvironmentReport }) {
  return (
    <ul className="divide-y divide-border" aria-label="Environment checks">
      {report.checks.map((check) => (
        <EnvironmentCheckRow key={check.id} check={check} />
      ))}
    </ul>
  );
}
