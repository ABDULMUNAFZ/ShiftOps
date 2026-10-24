import { summarizeReport } from "@shiftops/core";
import { Badge, Button, Card, CardHeader, EmptyState, Spinner } from "@shiftops/ui";
import { ArrowRight, Check, Workflow } from "lucide-react";
import { Link } from "react-router";
import { ErrorDisplay } from "../components/ErrorDisplay";
import { KeychainStatusLine } from "../features/credentials/CredentialSecurity";
import { DatabaseHealthCard } from "../features/health/DatabaseHealthCard";
import {
  useAgentTasks,
  useCampaigns,
  useCredentials,
  useEnvironment,
  useGithubStatus,
  useRepositories,
  useVerificationRuns,
} from "../lib/queries";
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

function Stat({ label, value, to }: { label: string; value: number | undefined; to: string }) {
  return (
    <Link
      to={to}
      className="group rounded-2xl border border-border bg-surface p-5 transition-colors hover:border-accent/60"
    >
      <p className="text-xs font-medium text-muted">{label}</p>
      <p className="mt-2 text-3xl font-semibold tabular-nums tracking-tight">
        {value === undefined ? <Spinner className="size-5" /> : value}
      </p>
    </Link>
  );
}

/** Live counts from the local database; nothing here is sample data. */
function Stats() {
  const repos = useRepositories({ limit: 1 });
  const trusted = useRepositories({ limit: 1, statuses: ["trusted"] });
  const campaigns = useCampaigns({ limit: 1 });
  const tasks = useAgentTasks({ limit: 1 });
  const verified = useVerificationRuns({ limit: 1, statuses: ["passed"] });
  return (
    <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-5" aria-label="Workspace totals">
      <Stat label="Repositories" value={repos.data?.total} to="/repositories" />
      <Stat label="Trusted" value={trusted.data?.total} to="/repositories" />
      <Stat label="Campaigns" value={campaigns.data?.total} to="/campaigns" />
      <Stat label="Devin tasks" value={tasks.data?.total} to="/agent-runs" />
      <Stat label="Passed verifications" value={verified.data?.total} to="/verification" />
    </div>
  );
}

function GettingStarted() {
  const repos = useRepositories({ limit: 1 });
  const github = useGithubStatus();
  const campaigns = useCampaigns({ limit: 1 });
  const steps = [
    {
      done: (repos.data?.total ?? 0) > 0,
      title: "Add repositories",
      body: "Open a checkout, discover a folder or import from GitHub.",
      to: "/repositories",
    },
    {
      done: github.data?.account?.status === "connected",
      title: "Connect GitHub",
      body: "Optional: browse, clone and read pull requests.",
      to: "/integrations",
    },
    {
      done: (campaigns.data?.total ?? 0) > 0,
      title: "Create a campaign",
      body: "Define the change once, then compute a plan.",
      to: "/campaigns",
    },
  ];
  if (steps.every((s) => s.done)) return null;
  return (
    <Card>
      <CardHeader title="Get started" description="Each step completes from your real data." />
      <ol className="grid gap-px bg-border sm:grid-cols-3">
        {steps.map((s, i) => (
          <li key={s.title} className="bg-surface">
            <Link to={s.to} className="group flex h-full items-start gap-3 p-5 hover:bg-surface-2">
              <span
                className={`grid size-7 shrink-0 place-items-center rounded-full text-xs font-semibold ${s.done ? "bg-accent text-accent-fg" : "border border-border text-muted"}`}
                aria-label={s.done ? "Done" : `Step ${i + 1}`}
              >
                {s.done ? <Check className="size-4" aria-hidden /> : i + 1}
              </span>
              <span className="min-w-0">
                <span className="flex items-center gap-1 font-medium">
                  {s.title}
                  <ArrowRight className="size-3.5 opacity-0 transition group-hover:opacity-100" aria-hidden />
                </span>
                <span className="mt-0.5 block text-sm text-muted">{s.body}</span>
              </span>
            </Link>
          </li>
        ))}
      </ol>
    </Card>
  );
}

export function OverviewPage() {
  const campaigns = useCampaigns({ limit: 1 });
  return (
    <Page title="Overview" description="State of this workstation and of your migration campaigns.">
      <Stats />
      <GettingStarted />
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
          <EmptyState
            icon={<Workflow />}
            title="No campaigns yet"
            action={
              <Button asChild size="sm" variant="primary">
                <Link to="/campaigns">Create a campaign</Link>
              </Button>
            }
          >
            Pick repositories you have added, describe the change once and let ShiftOps plan the order.
          </EmptyState>
        )}
      </Card>
    </Page>
  );
}
