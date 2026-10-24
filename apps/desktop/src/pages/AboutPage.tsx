import { brand, Card, CardHeader, Logo, Spinner } from "@shiftops/ui";
import { ErrorDisplay } from "../components/ErrorDisplay";
import { ExternalLink } from "../components/ExternalLink";
import { useAppInfo } from "../lib/queries";
import { DefinitionList, Page } from "./Page";

/** Product identity, build facts and the event credit. Nothing here is computed except the build facts. */
export function AboutPage() {
  const info = useAppInfo();

  return (
    <Page title="About" description={`${brand.name} ${info.data ? `v${info.data.version}` : ""}`.trim()}>
      <Card>
        <div className="flex flex-col gap-4 p-6">
          <div className="flex items-center gap-3">
            <Logo className="size-9" />
            <div>
              <h2 className="text-lg font-semibold tracking-tight">{brand.name}</h2>
              <p className="text-muted">{brand.tagline}</p>
            </div>
          </div>
          <p className="max-w-2xl text-sm">{brand.description}</p>
          <p className="font-mono text-xs text-muted">{brand.event}</p>
          <p className="max-w-2xl text-xs text-muted">
            {brand.name} is an independent project built for Devin Hacks Kochi 2026. It is not an official Cognition
            product and is not affiliated with or endorsed by Cognition.
          </p>
          <p className="text-sm">
            <ExternalLink href={brand.repositoryUrl}>Source code on GitHub</ExternalLink>
          </p>
        </div>
      </Card>

      <Card>
        <CardHeader title="This build" description="Read from the running application." />
        {info.isPending ? (
          <div className="p-4">
            <Spinner />
          </div>
        ) : info.isError ? (
          <div className="p-4">
            <ErrorDisplay title="Build information unavailable" error={info.error} />
          </div>
        ) : (
          <DefinitionList
            rows={[
              { term: "Version", value: info.data.version },
              {
                term: "Database schema",
                value: `${info.data.schemaVersion} (latest ${info.data.latestSchemaVersion})`,
              },
              { term: "Application data", value: info.data.dataDir },
              { term: "Logs", value: info.data.logDir },
            ]}
          />
        )}
      </Card>

      <Card>
        <CardHeader title="Division of responsibility" />
        <div className="grid gap-6 p-4 text-sm sm:grid-cols-2">
          <div>
            <h3 className="mb-2 text-xs font-semibold uppercase tracking-wide text-muted">{brand.name}</h3>
            <ul className="list-disc space-y-1 pl-4">
              <li>Discovers affected repositories</li>
              <li>Understands dependencies and risk</li>
              <li>Plans migration waves</li>
              <li>Prepares repository-specific Devin tasks</li>
              <li>Independently verifies results</li>
              <li>Tracks the complete transformation</li>
            </ul>
          </div>
          <div>
            <h3 className="mb-2 text-xs font-semibold uppercase tracking-wide text-muted">Devin</h3>
            <ul className="list-disc space-y-1 pl-4">
              <li>Understands an individual repository</li>
              <li>Implements the requested code changes</li>
              <li>Repairs build and test failures</li>
              <li>Produces a branch or pull request</li>
            </ul>
            <p className="mt-3 text-xs text-muted">Devin integration is not available in this build.</p>
          </div>
        </div>
      </Card>
    </Page>
  );
}
