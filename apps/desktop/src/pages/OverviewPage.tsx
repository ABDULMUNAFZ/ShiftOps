import { summarizeReport } from "@shiftops/core";
import { Badge, Button, Card, CardHeader, EmptyState, ErrorState, Spinner } from "@shiftops/ui";
import { Workflow } from "lucide-react";
import { Link } from "react-router";
import { CredentialBadge } from "../features/integrations/CredentialBadge";
import { useAppInfo, useEnvironment } from "../lib/queries";
import { DefinitionList, Page } from "./Page";

export function OverviewPage() {
  const env = useEnvironment();
  const info = useAppInfo();
  const summary = env.data ? summarizeReport(env.data) : null;

  return (
    <Page title="Overview" description="State of this workstation and of your migration campaigns.">
      <div className="grid gap-4 lg:grid-cols-2">
        <Card>
          <CardHeader
            title="Environment"
            description="From the most recent native check."
            actions={
              <Button size="sm" asChild>
                <Link to="/settings">Details</Link>
              </Button>
            }
          />
          {env.isPending ? (
            <div className="flex items-center gap-2 p-4 text-muted">
              <Spinner /> Running checks…
            </div>
          ) : env.isError ? (
            <div className="p-4">
              <ErrorState title="Environment checks could not run" error={env.error} />
            </div>
          ) : (
            <ul className="divide-y divide-border">
              {env.data.checks.map((c) => (
                <li key={c.id} className="flex items-center justify-between gap-3 px-4 py-2">
                  <span>{c.label}</span>
                  <span className="flex items-center gap-2">
                    {c.version ? <span className="font-mono text-xs text-muted">{c.version}</span> : null}
                    <Badge tone={c.status === "available" ? "ok" : c.status === "unavailable" ? "warn" : "danger"}>
                      {c.status}
                    </Badge>
                  </span>
                </li>
              ))}
            </ul>
          )}
          {summary && summary.missingRequired.length > 0 ? (
            <p className="border-t border-border px-4 py-2 text-xs text-warn">
              Missing required: {summary.missingRequired.join(", ")}
            </p>
          ) : null}
        </Card>

        <div className="space-y-4">
          <Card>
            <CardHeader title="Integrations" description="Credentials are read from the OS keychain." />
            <ul className="divide-y divide-border">
              <li className="flex items-center justify-between px-4 py-2.5">
                <span>GitHub</span>
                <CredentialBadge provider="github" />
              </li>
              <li className="flex items-center justify-between px-4 py-2.5">
                <span>Devin</span>
                <CredentialBadge provider="devin" />
              </li>
            </ul>
          </Card>

          <Card>
            <CardHeader title="Local state" />
            {info.isError ? (
              <div className="p-4">
                <ErrorState title="Application info unavailable" error={info.error} />
              </div>
            ) : info.data ? (
              <DefinitionList
                rows={[
                  { term: "App version", value: info.data.version },
                  { term: "Database", value: info.data.databasePath },
                  { term: "Schema version", value: info.data.schemaVersion },
                ]}
              />
            ) : (
              <div className="p-4">
                <Spinner />
              </div>
            )}
          </Card>
        </div>
      </div>

      <Card>
        <CardHeader title="Campaigns" />
        <EmptyState icon={<Workflow />} title="No campaigns">
          Campaigns are created from discovered repositories. Repository discovery is not part of this build yet.
        </EmptyState>
      </Card>
    </Page>
  );
}
