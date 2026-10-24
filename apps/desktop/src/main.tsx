import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./app/App";
import { applyBootThemeHint } from "./lib/theme";
import "./styles.css";

applyBootThemeHint();

const queryClient = new QueryClient({
  defaultOptions: {
    // Native calls are local; retrying hides real failures.
    queries: { retry: false, refetchOnWindowFocus: false },
  },
});

const root = document.getElementById("root");
if (!root) throw new Error("#root element missing from index.html");

createRoot(root).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <App />
    </QueryClientProvider>
  </StrictMode>,
);
