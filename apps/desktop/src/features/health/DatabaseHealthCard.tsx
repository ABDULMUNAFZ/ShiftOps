import { Badge, Card, CardHeader, Spinner } from "@shiftops/ui";
import { Database } from "lucide-react";
import { ErrorDisplay } from "../../components/ErrorDisplay";
import { useDatabaseHealth } from "../../lib/queries";
import { DefinitionList } from "../../pages/Page";

function formatBytes(n: number | null) {
  if (n === null) return "unknown";
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KiB`;
  return `${(n / 1024 / 1024).toFixed(1)} MiB`;
}

export function DatabaseHealthCard({ detailed = false }: { detailed?: boolean }) {
  const health = useDatabaseHealth();
  const ok = health.data
    ? health.data.upToDate && health.data.foreignKeysEnabled && health.data.quickCheck === "ok"
    : false;

  return (
    <Card>
      <CardHeader
        title={
          <span className="flex items-center gap-2">
            <Database className="size-4" aria-hidden /> Database health
          </span>
        }
        description="Read from the local SQLite database."
        actions={health.data ? <Badge tone={ok ? "ok" : "danger"}>{ok ? "Healthy" : "Needs attention"}</Badge> : null}
      />
      {health.isPending ? (
        <div className="flex items-center gap-2 p-4 text-muted">
          <Spinner /> Checking database…
        </div>
      ) : health.isError ? (
        <div className="p-4">
          <ErrorDisplay title="Database health unavailable" error={health.error} />
        </div>
      ) : (
        <DefinitionList
          rows={[
            {
              term: "Schema version",
              value: `${health.data.schemaVersion} of ${health.data.latestSchemaVersion}${health.data.upToDate ? "" : " (upgrade pending)"}`,
            },
            { term: "Integrity (quick_check)", value: health.data.quickCheck },
            { term: "Foreign keys", value: health.data.foreignKeysEnabled ? "enforced" : "NOT enforced" },
            { term: "Activity events", value: health.data.activityEventCount },
            ...(detailed
              ? [
                  { term: "Journal mode", value: health.data.journalMode },
                  { term: "File size", value: formatBytes(health.data.sizeBytes) },
                  {
                    term: "Applied migrations",
                    value: (
                      <ul>
                        {health.data.appliedMigrations.map((m) => (
                          <li key={m.version}>
                            {m.version} {m.name} · {m.appliedAt}
                          </li>
                        ))}
                      </ul>
                    ),
                  },
                ]
              : []),
          ]}
        />
      )}
    </Card>
  );
}
