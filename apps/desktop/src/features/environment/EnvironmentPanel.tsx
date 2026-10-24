import { summarizeReport } from "@shiftops/core";
import { Button, Card, CardHeader, ErrorState, Spinner } from "@shiftops/ui";
import { RefreshCw } from "lucide-react";
import { useEnvironment } from "../../lib/queries";
import { EnvironmentCheckList } from "./EnvironmentChecks";

export function EnvironmentPanel({ className }: { className?: string }) {
  const env = useEnvironment();
  const summary = env.data ? summarizeReport(env.data) : null;

  return (
    <Card className={className}>
      <CardHeader
        title="Environment"
        description={
          env.data && summary
            ? `${summary.available} available · ${summary.unavailable} unavailable · ${summary.errors} errors · collected ${new Date(env.data.collectedAt).toLocaleString()}`
            : "Native checks run on this machine through the Tauri backend."
        }
        actions={
          <Button size="sm" onClick={() => void env.refetch()} disabled={env.isFetching}>
            {env.isFetching ? <Spinner className="size-3" /> : <RefreshCw />}
            {env.isFetching ? "Checking" : "Re-run checks"}
          </Button>
        }
      />
      {env.isPending ? (
        <div className="flex items-center gap-3 px-4 py-6 text-muted">
          <Spinner /> Running environment checks…
        </div>
      ) : env.isError ? (
        <div className="p-4">
          <ErrorState title="Environment checks could not run" error={env.error} />
        </div>
      ) : (
        <EnvironmentCheckList report={env.data} />
      )}
    </Card>
  );
}
