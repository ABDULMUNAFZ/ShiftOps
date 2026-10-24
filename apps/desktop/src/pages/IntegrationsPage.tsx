import type { CheckId } from "@shiftops/core";
import { Badge, Card, CardHeader, Spinner } from "@shiftops/ui";
import { CredentialSecurity } from "../features/credentials/CredentialSecurity";
import { useCredentials, useEnvironment } from "../lib/queries";
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

function CredentialState({
  provider,
  kinds,
  missing,
}: {
  provider: "github" | "devin";
  kinds: string[];
  missing: string;
}) {
  const credentials = useCredentials();
  if (credentials.isPending) return <Spinner className="size-3" />;
  if (credentials.isError) return <Badge tone="danger">Keychain error</Badge>;
  const stored = credentials.data.filter((c) => c.provider === provider && kinds.includes(c.kind) && c.exists);
  return stored.length > 0 ? <Badge tone="ok">Credential stored · not validated</Badge> : <Badge>{missing}</Badge>;
}

export function IntegrationsPage() {
  return (
    <Page
      title="Integrations"
      description="Accounts and tools ShiftOps delegates to. No integration makes network calls in this build."
    >
      <div className="grid gap-4 lg:grid-cols-2">
        <Card>
          <CardHeader
            title="GitHub"
            description="Repository discovery, branches and pull requests."
            actions={
              <CredentialState
                provider="github"
                kinds={["github_personal_access_token", "github_oauth_token"]}
                missing="Not configured"
              />
            }
          />
          <DefinitionList
            rows={[
              { term: "Local git", value: <ToolStatus id="git" missing="Git not detected" /> },
              { term: "API access", value: "Not implemented in this build; a stored token is not used yet." },
            ]}
          />
        </Card>
        <Card>
          <CardHeader
            title="Devin"
            description="Repository-level implementation sessions."
            actions={
              <CredentialState provider="devin" kinds={["devin_api_token"]} missing="API credential not stored" />
            }
          />
          <DefinitionList
            rows={[
              { term: "Devin CLI", value: <ToolStatus id="devin" missing="Not detected" /> },
              { term: "Sessions", value: "Not implemented in this build; ShiftOps is not connected to Devin." },
            ]}
          />
        </Card>
      </div>
      <CredentialSecurity />
    </Page>
  );
}
