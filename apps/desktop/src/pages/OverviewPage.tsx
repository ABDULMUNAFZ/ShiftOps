import { summarizeReport } from "@shiftops/core";
import { Badge, Button, Card, CardHeader, EmptyState, Spinner } from "@shiftops/ui";
import { Workflow } from "lucide-react";
import { Link } from "react-router";
import { ErrorDisplay } from "../components/ErrorDisplay";
import { KeychainStatusLine } from "../features/credentials/CredentialSecurity";
import { DatabaseHealthCard } from "../features/health/DatabaseHealthCard";
import { useCampaigns, useCredentials, useEnvironment, useGithubStatus } from "../lib/queries";
import { Page } from "./Page";

function EnvironmentCard() {
  const env = useEnvironment();
  const summary = env.data ? summarizeReport(env.data) : null;
  return (
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
          <ErrorDisplay title="Environment checks could not run" error={env.error} />
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
  );
}

function CredentialsCard() {
  const credentials = useCredentials();
  const github = useGithubStatus();
  const githubAccount = github.data?.account;
  const stored = (provider: "github" | "devin") =>
    credentials.data?.some((c) => c.provider === provider && c.exists) ?? false;
  return (
    <Card>
      <CardHeader
        title="Keychain"
        description="Credential presence only; values never leave the keychain."
        actions={<KeychainStatusLine />}
      />
      {credentials.isError ? (
        <div className="p-4">
          <ErrorDisplay title="Credentials could not be read" error={credentials.error} />
        </div>
      ) : (
        <ul className="divide-y divide-border">
          {(["github", "devin"] as const).map((p) => (
            <li key={p} className="flex items-center justify-between px-4 py-2.5">
              <span>{p === "github" ? "GitHub" : "Devin"}</span>
              {credentials.isPending ? (
                <Spinner className="size-3" />
              ) : p === "github" && githubAccount ? (
                <Badge tone={githubAccount.status === "connected" ? "ok" : "danger"}>
                  {githubAccount.status === "connected" ? `Connected as ${githubAccount.login}` : "Credential rejected"}
                </Badge>
              ) : stored(p) ? (
                <Badge tone="ok">Credential stored</Badge>
              ) : (
                <Badge>Not configured</Badge>
              )}
            </li>
          ))}
        </ul>
      )}
    </Card>
  );
}

export function OverviewPage() {
  const campaigns = useCampaigns({ limit: 1 });
  return (
    <Page title="Overview" description="State of this workstation and of your migration campaigns.">
      <div className="grid gap-4 lg:grid-cols-2">
        <EnvironmentCard />
        <div className="space-y-4">
          <DatabaseHealthCard />
          <CredentialsCard />
        </div>
      </div>

      <Card>
        <CardHeader title="Campaigns" />
        {campaigns.isError ? (
          <div className="p-4">
            <ErrorDisplay title="Campaigns could not be loaded" error={campaigns.error} />
          </div>
        ) : campaigns.data && campaigns.data.total > 0 ? (
          <p className="px-4 py-3 text-sm">
            {campaigns.data.total} campaign{campaigns.data.total === 1 ? "" : "s"}.{" "}
            <Link to="/campaigns" className="text-accent underline underline-offset-2">
              View campaigns
            </Link>
          </p>
        ) : (
          <EmptyState icon={<Workflow />} title="No campaigns">
            Campaigns are created from discovered repositories. Repository discovery is not part of this build yet.
          </EmptyState>
        )}
      </Card>
    </Page>
  );
}
