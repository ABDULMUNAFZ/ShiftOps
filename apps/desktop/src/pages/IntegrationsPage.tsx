import type { CheckId } from "@shiftops/core";
import { Badge, Button, Card, CardHeader } from "@shiftops/ui";
import type { ReactNode } from "react";
import { CredentialBadge } from "../features/integrations/CredentialBadge";
import { useEnvironment } from "../lib/queries";
import { DefinitionList, Page } from "./Page";

function ToolStatus({ id }: { id: CheckId }) {
  const env = useEnvironment();
  const check = env.data?.checks.find((c) => c.id === id);
  if (!check) return <span className="text-muted">{env.isPending ? "checking…" : "not checked"}</span>;
  return check.status === "available" ? (
    <span>
      {check.version ?? "available"} <span className="text-muted">({check.path})</span>
    </span>
  ) : (
    <span className="text-warn">{check.detail}</span>
  );
}

function IntegrationCard({
  name,
  provider,
  description,
  rows,
}: {
  name: string;
  provider: "github" | "devin";
  description: string;
  rows: { term: string; value: ReactNode }[];
}) {
  return (
    <Card>
      <CardHeader
        title={
          <span className="flex items-center gap-2">
            {name} <CredentialBadge provider={provider} />
          </span>
        }
        description={description}
        actions={
          <Button size="sm" disabled title="Connecting accounts is not implemented in this build">
            Connect
          </Button>
        }
      />
      <DefinitionList rows={rows} />
    </Card>
  );
}

export function IntegrationsPage() {
  return (
    <Page title="Integrations" description="Accounts and tools ShiftOps delegates to. Secrets live only in the OS keychain.">
      <IntegrationCard
        name="GitHub"
        provider="github"
        description="Repository discovery, branches and pull requests."
        rows={[
          { term: "Keychain entry", value: "dev.shiftops.desktop / github" },
          { term: "Local git", value: <ToolStatus id="git" /> },
          { term: "Account connection", value: <Badge>Not implemented in this build</Badge> },
        ]}
      />
      <IntegrationCard
        name="Devin"
        provider="devin"
        description="Repository-level implementation sessions."
        rows={[
          { term: "Keychain entry", value: "dev.shiftops.desktop / devin" },
          { term: "Devin CLI", value: <ToolStatus id="devin" /> },
          { term: "Account connection", value: <Badge>Not implemented in this build</Badge> },
        ]}
      />
    </Page>
  );
}
