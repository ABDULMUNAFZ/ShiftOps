import type { CheckId } from "@shiftops/core";
import { Badge, Button, Card, CardHeader, Spinner } from "@shiftops/ui";
import { RefreshCw } from "lucide-react";
import { CredentialSecurity } from "../features/credentials/CredentialSecurity";
import { GithubConnection } from "../features/github/GithubConnection";
import { useDevinStatus, useEnvironment } from "../lib/queries";
import { DefinitionList, Page } from "./Page";

function ToolStatus({ id, missing }: { id: CheckId; missing: string }) {
  const env = useEnvironment();
  const check = env.data?.checks.find((c) => c.id === id);
  if (env.isPending) return <Spinner className="size-3" />;
  if (!check) return <span className="text-muted">not checked</span>;
  return check.status === "available" ? (
    <span>
      Detected {check.version ?? ""} <span className="text-muted">({check.path})</span>
    </span>
  ) : (
    <span className="text-warn">{missing}</span>
  );
}

function DevinCard() {
  const status = useDevinStatus();
  const d = status.data;
  const badge = status.isPending ? (
    <Spinner className="size-3" />
  ) : status.isError ? (
    <Badge tone="danger">Error</Badge>
  ) : !d?.installed ? (
    <Badge tone="warn">Not installed</Badge>
  ) : d.authenticated ? (
    <Badge tone="ok">Connected</Badge>
  ) : (
    <Badge tone="warn">Not signed in</Badge>
  );
  return (
    <Card>
      <CardHeader
        title="Devin"
        description="Repository-level implementation sessions via the local Devin CLI."
        actions={
          <span className="flex items-center gap-2">
            {badge}
            <Button size="sm" variant="ghost" onClick={() => void status.refetch()} disabled={status.isFetching}>
              <RefreshCw />
            </Button>
          </span>
        }
      />
      <DefinitionList
        rows={[
          { term: "Devin CLI", value: <ToolStatus id="devin" missing="Not detected" /> },
          { term: "Version", value: d?.version ?? "—" },
          { term: "Account", value: d?.accountEmail ?? "—" },
          {
            term: "Credentials",
            value: "Held by the Devin CLI (`devin auth login`). ShiftOps stores none and never reads them.",
          },
          {
            term: "Dispatch",
            value:
              "`devin -p` with --sandbox and --permission-mode smart, inside the trusted checkout, after approval.",
          },
          { term: "Status", value: status.isError ? String(status.error) : (d?.detail ?? "—") },
        ]}
      />
    </Card>
  );
}

export function IntegrationsPage() {
  return (
    <Page
      title="Integrations"
      description="Accounts and tools ShiftOps works with. Tokens are stored only in the OS keychain."
    >
      <div className="grid gap-4 lg:grid-cols-2">
        <GithubConnection />
        <DevinCard />
      </div>
      <CredentialSecurity />
    </Page>
  );
}
