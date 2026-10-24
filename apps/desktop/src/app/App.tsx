import { brand, Logo, Spinner } from "@shiftops/ui";
import { useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";
import { createHashRouter, RouterProvider } from "react-router";
import { ErrorBoundary, RouteError } from "../components/ErrorBoundary";
import { ErrorDisplay } from "../components/ErrorDisplay";
import { FirstRun } from "../features/onboarding/FirstRun";
import { invalidateAfter } from "../lib/invalidation";
import { onProcessEvent } from "../lib/native";
import { processStore } from "../lib/processStore";
import { useSettings } from "../lib/queries";
import { useApplyTheme } from "../lib/theme";
import { AboutPage } from "../pages/AboutPage";
import { AgentRunsPage, CampaignsPage, VerificationPage } from "../pages/DataPages";
import { RepositoriesPage } from "../pages/RepositoriesPage";
import { RepositoryDetailPage } from "../pages/RepositoryDetailPage";
import { IntegrationsPage } from "../pages/IntegrationsPage";
import { OverviewPage } from "../pages/OverviewPage";
import { SettingsPage } from "../pages/SettingsPage";
import { Shell } from "./Shell";

// Hash routing: the production bundle is served from the Tauri asset
// protocol, which has no server-side fallback for deep links.
const router = createHashRouter([
  {
    path: "/",
    element: <Shell />,
    errorElement: <RouteError />,
    children: [
      { index: true, element: <OverviewPage /> },
      { path: "repositories", element: <RepositoriesPage /> },
      { path: "repositories/:id", element: <RepositoryDetailPage /> },
      { path: "campaigns", element: <CampaignsPage /> },
      { path: "agent-runs", element: <AgentRunsPage /> },
      { path: "verification", element: <VerificationPage /> },
      { path: "integrations", element: <IntegrationsPage /> },
      { path: "settings", element: <SettingsPage /> },
      { path: "about", element: <AboutPage /> },
    ],
  },
]);

/** Feed native process events into the store and refresh dependent queries. */
function useProcessEvents() {
  const client = useQueryClient();
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let disposed = false;
    void onProcessEvent((event) => {
      processStore.apply(event);
      if (event.type === "started" || event.type === "completed") void invalidateAfter(client, "process");
    }).then((fn) => {
      if (disposed) fn();
      else unlisten = fn;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [client]);
}

export function App() {
  const settings = useSettings();
  useApplyTheme(settings.data?.theme);
  useProcessEvents();

  if (settings.isPending) {
    return (
      <div className="flex h-full items-center justify-center gap-3 text-muted">
        <Spinner /> Loading {brand.name}…
      </div>
    );
  }

  if (settings.isError) {
    return (
      <div className="mx-auto flex h-full max-w-xl flex-col justify-center gap-4 p-8">
        <div className="flex items-center gap-2">
          <Logo />
          <span className="font-semibold">{brand.name}</span>
        </div>
        <ErrorDisplay title="Local application state could not be loaded" error={settings.error} />
      </div>
    );
  }

  return (
    <ErrorBoundary title="ShiftOps hit an unexpected error">
      {settings.data.onboardingCompletedAt === null ? <FirstRun /> : <RouterProvider router={router} />}
    </ErrorBoundary>
  );
}
