import type { Page as DataPage } from "@shiftops/core";
import { Badge, Card, EmptyState, Spinner, type BadgeTone } from "@shiftops/ui";
import type { UseQueryResult } from "@tanstack/react-query";
import { ShieldCheck } from "lucide-react";
import type { ReactNode } from "react";
import { ErrorDisplay } from "../components/ErrorDisplay";
import { useVerificationRuns } from "../lib/queries";
import { Page } from "./Page";

/*
 * These sections read their tables from the local database and show truthful
 * empty states until real records exist.
 */

const RUN_TONE: Record<string, BadgeTone> = {
  queued: "neutral",
  running: "accent",
  passed: "ok",
  failed: "danger",
  error: "danger",
  cancelled: "warn",
};

function Listing<T extends { id: string }>({
  query,
  empty,
  columns,
}: {
  query: UseQueryResult<DataPage<T>>;
  empty: ReactNode;
  columns: { header: string; cell: (row: T) => ReactNode }[];
}) {
  if (query.isPending)
    return (
      <div className="flex items-center gap-2 p-4 text-muted">
        <Spinner /> Loading…
      </div>
    );
  if (query.isError)
    return (
      <div className="p-4">
        <ErrorDisplay title="Could not load records" error={query.error} />
      </div>
    );
  if (query.data.items.length === 0) return <>{empty}</>;
  return (
    <table className="w-full text-sm">
      <thead>
        <tr className="border-b border-border text-left text-xs text-muted">
          {columns.map((c) => (
            <th key={c.header} className="px-4 py-2 font-medium">
              {c.header}
            </th>
          ))}
        </tr>
      </thead>
      <tbody className="divide-y divide-border">
        {query.data.items.map((row) => (
          <tr key={row.id}>
            {columns.map((c) => (
              <td key={c.header} className="px-4 py-2">
                {c.cell(row)}
              </td>
            ))}
          </tr>
        ))}
      </tbody>
    </table>
  );
}

export function VerificationPage() {
  const query = useVerificationRuns({});
  return (
    <Page
      title="Verification"
      description="Runs of each repository's own checks, executed by ShiftOps itself. Start one from a campaign member's Verify panel."
    >
      <Card>
        <Listing
          query={query}
          columns={[
            {
              header: "Status",
              cell: (v) => <Badge tone={RUN_TONE[v.status] ?? "neutral"}>{v.status}</Badge>,
            },
            {
              header: "Commit",
              cell: (v) => <span className="font-mono text-xs">{v.verifiedCommitSha.slice(0, 10)}</span>,
            },
            { header: "Profile", cell: (v) => <span className="font-mono text-xs">{v.profile}</span> },
            {
              header: "Trigger",
              cell: (v) => (v.trigger === "after_repair" ? <Badge tone="accent">after repair</Badge> : "manual"),
            },
            { header: "Started", cell: (v) => new Date(v.startedAt).toLocaleString() },
          ]}
          empty={
            <EmptyState icon={<ShieldCheck />} title="No verification runs yet">
              Open a campaign, choose Verify on a trusted repository, review the exact commands and run them. Results
              appear here with their real exit codes.
            </EmptyState>
          }
        />
      </Card>
    </Page>
  );
}
