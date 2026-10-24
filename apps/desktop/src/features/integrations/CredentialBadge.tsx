import { Badge, Spinner } from "@shiftops/ui";
import { useCredentialStatus } from "../../lib/queries";

/** Reports whether a credential exists in the OS keychain. Never reads its value into the UI. */
export function CredentialBadge({ provider }: { provider: "github" | "devin" }) {
  const status = useCredentialStatus(provider);
  if (status.isPending) return <Spinner className="size-3" label="Checking keychain" />;
  if (status.isError) return <Badge tone="danger" title={status.error.message}>Keychain error</Badge>;
  if (status.data.error) return <Badge tone="danger" title={status.data.error}>Keychain error</Badge>;
  return status.data.present ? <Badge tone="ok">Credential stored</Badge> : <Badge>Not connected</Badge>;
}
