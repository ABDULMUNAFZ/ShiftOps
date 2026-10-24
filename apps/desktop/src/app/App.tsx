import { brand, ErrorState, Logo, Spinner } from "@shiftops/ui";
import { createHashRouter, RouterProvider } from "react-router";
import { FirstRun } from "../features/onboarding/FirstRun";
import { useSettings } from "../lib/queries";
import { useApplyTheme } from "../lib/theme";
import { IntegrationsPage } from "../pages/IntegrationsPage";
import { OverviewPage } from "../pages/OverviewPage";
import { AgentRunsPage, CampaignsPage, RepositoriesPage, VerificationPage } from "../pages/PlaceholderPages";
import { SettingsPage } from "../pages/SettingsPage";
import { Shell } from "./Shell";

// Hash routing: the production bundle is served from the Tauri asset
// protocol, which has no server-side fallback for deep links.
const router = createHashRouter([
  {
    path: "/",
    element: <Shell />,
    children: [
      { index: true, element: <OverviewPage /> },
      { path: "repositories", element: <RepositoriesPage /> },
      { path: "campaigns", element: <CampaignsPage /> },
      { path: "agent-runs", element: <AgentRunsPage /> },
      { path: "verification", element: <VerificationPage /> },
      { path: "integrations", element: <IntegrationsPage /> },
      { path: "settings", element: <SettingsPage /> },
    ],
  },
]);

export function App() {
  const settings = useSettings();
  useApplyTheme(settings.data?.theme);

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
        <ErrorState title="Local application state could not be loaded" error={settings.error} />
      </div>
    );
  }

  if (settings.data.onboardingCompletedAt === null) return <FirstRun />;
  return <RouterProvider router={router} />;
}
