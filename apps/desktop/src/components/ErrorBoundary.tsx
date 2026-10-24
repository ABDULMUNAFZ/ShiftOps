import { Button } from "@shiftops/ui";
import { Component, type ErrorInfo, type ReactNode } from "react";
import { useRouteError } from "react-router";
import { ErrorDisplay } from "./ErrorDisplay";

interface State {
  error: unknown;
}

/** Catches render errors so one broken view never blanks the whole window. */
export class ErrorBoundary extends Component<{ children: ReactNode; title?: string }, State> {
  override state: State = { error: null };

  static getDerivedStateFromError(error: unknown): State {
    return { error };
  }

  override componentDidCatch(error: unknown, info: ErrorInfo) {
    console.error("Unhandled render error", error, info.componentStack);
  }

  override render() {
    if (this.state.error === null) return this.props.children;
    return (
      <div className="p-6">
        <ErrorDisplay
          title={this.props.title ?? "This view failed to render"}
          error={this.state.error}
          action={
            <Button size="sm" onClick={() => this.setState({ error: null })}>
              Try again
            </Button>
          }
        />
      </div>
    );
  }
}

/** Router-level error element. */
export function RouteError() {
  const error = useRouteError();
  return (
    <div className="p-6">
      <ErrorDisplay title="This page failed to load" error={error} />
    </div>
  );
}
