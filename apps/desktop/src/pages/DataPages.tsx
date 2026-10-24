import type { Page as DataPage } from "@shiftops/core";
import { Card, EmptyState, Spinner } from "@shiftops/ui";
import type { UseQueryResult } from "@tanstack/react-query";
import { Bot, ShieldCheck, Workflow } from "lucide-react";
import type { ReactNode } from "react";
import { ErrorDisplay } from "../components/ErrorDisplay";
import { useAgentTasks, useCampaigns, useVerificationRuns } from "../lib/queries";
import { Page } from "./Page";

/*
 * These sections read their tables from the local database. Nothing in this
 * build creates their records, so they show truthful empty states; once later
 * steps write records they appear here without UI changes.
 */

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

export function CampaignsPage() {
  const query = useCampaigns({});
  return (
    <Page title="Campaigns" description="One codebase-wide change, planned across every affected repository.">
      <Card>
        <Listing
          query={query}
          columns={[
            { header: "Name", cell: (c) => c.name },
            { header: "Status", cell: (c) => c.status },
            { header: "Repositories", cell: (c) => c.repositoryCount },
          ]}
          empty={
            <EmptyState icon={<Workflow />} title="No campaigns">
              Creating a campaign requires discovered repositories, which are not available in this build.
            </EmptyState>
          }
        />
      </Card>
    </Page>
  );
}

export function AgentRunsPage() {
  const query = useAgentTasks({});
  return (
    <Page title="Agent Runs" description="Repository-level implementation sessions delegated to Devin.">
      <Card>
        <Listing
          query={query}
          columns={[
            { header: "Task", cell: (t) => t.id },
            { header: "Status", cell: (t) => t.status },
            { header: "Branch", cell: (t) => t.branchName ?? "" },
          ]}
          empty={
            <EmptyState icon={<Bot />} title="No agent runs">
              No Devin sessions have been started from this workstation. Delegation is not implemented in this build.
            </EmptyState>
          }
        />
      </Card>
    </Page>
  );
}

export function VerificationPage() {
  const query = useVerificationRuns({});
  return (
    <Page
      title="Verification"
      description="Independent re-runs of each repository's checks against the agent's branch."
    >
      <Card>
        <Listing
          query={query}
          columns={[
            { header: "Run", cell: (v) => v.id },
            { header: "Status", cell: (v) => v.status },
            { header: "Commit", cell: (v) => v.verifiedCommitSha },
          ]}
          empty={
            <EmptyState icon={<ShieldCheck />} title="No verification results">
              Verification runs after an agent produces a branch. No branches have been produced yet.
            </EmptyState>
          }
        />
      </Card>
    </Page>
  );
}
