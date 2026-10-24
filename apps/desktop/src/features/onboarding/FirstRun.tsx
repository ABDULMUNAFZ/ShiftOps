import { summarizeReport } from "@shiftops/core";
import { brand, Button, Logo } from "@shiftops/ui";
import { ArrowRight } from "lucide-react";
import { useEnvironment, useSetSetting } from "../../lib/queries";
import { EnvironmentPanel } from "../environment/EnvironmentPanel";

/**
 * First-run screen. Shown until the user acknowledges the environment report.
 * Missing required tools are surfaced but do not block continuing: the rest
 * of the app shows its own unavailable states.
 */
export function FirstRun() {
  const env = useEnvironment();
  const setSetting = useSetSetting();
  const summary = env.data ? summarizeReport(env.data) : null;

  const finish = () => setSetting.mutate({ key: "onboardingCompletedAt", value: new Date().toISOString() });

  return (
    <main className="h-full overflow-y-auto">
      <div className="mx-auto flex max-w-4xl flex-col gap-6 px-8 py-10">
        <header className="flex items-center gap-3">
          <Logo className="size-8" />
          <div>
            <h1 className="text-lg font-semibold">Welcome to {brand.name}</h1>
            <p className="text-muted">{brand.tagline}</p>
          </div>
        </header>

        <p className="max-w-2xl text-sm text-muted">
          ShiftOps runs Git, package managers and agents on this machine. These checks show exactly what is installed.
          Nothing is uploaded; results stay in the local application.
        </p>

        <EnvironmentPanel />

        <footer className="flex items-center justify-between gap-4">
          <p className="text-xs text-muted" aria-live="polite">
            {summary && summary.missingRequired.length > 0
              ? `Missing required: ${summary.missingRequired.join(", ")}. You can continue, but related features will be unavailable.`
              : summary
                ? "All required checks passed."
                : null}
          </p>
          <Button variant="primary" size="lg" onClick={finish} disabled={!env.data || setSetting.isPending}>
            Continue to ShiftOps <ArrowRight />
          </Button>
        </footer>
        {setSetting.isError ? (
          <p role="alert" className="text-sm text-danger">
            Could not save onboarding state: {setSetting.error.message}
          </p>
        ) : null}
      </div>
    </main>
  );
}
