import { ShiftOpsError } from "@shiftops/core";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ErrorBoundary } from "./ErrorBoundary";
import { ErrorDisplay } from "./ErrorDisplay";

const error = new ShiftOpsError({
  code: "COMMAND_TIMED_OUT",
  message: "git --version timed out after 10 s.",
  technicalDetails: "killed process group 4242",
  suggestedAction: "Increase the execution timeout in Settings.",
  recoverable: true,
  context: { timeoutSeconds: 10 },
});

describe("ErrorDisplay", () => {
  it("shows code, message, suggested action and expandable details", () => {
    render(<ErrorDisplay title="Diagnostic failed" error={error} />);
    const alert = screen.getByRole("alert");
    expect(alert.textContent).toContain("COMMAND_TIMED_OUT");
    expect(alert.textContent).toContain("git --version timed out after 10 s.");
    expect(alert.textContent).toContain("Increase the execution timeout in Settings.");
    expect(screen.getByText("Technical details").closest("details")?.textContent).toContain(
      "killed process group 4242",
    );
  });

  it("copies the full structured error", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.assign(navigator, { clipboard: { writeText } });
    render(<ErrorDisplay title="Diagnostic failed" error={error} />);
    fireEvent.click(screen.getByRole("button", { name: "Copy details" }));
    expect(JSON.parse(writeText.mock.calls[0]?.[0] as string)).toMatchObject({
      code: "COMMAND_TIMED_OUT",
      context: { timeoutSeconds: 10 },
    });
    await waitFor(() => expect(screen.getByText("Copied")).toBeTruthy());
  });

  it("normalises unexpected throwables", () => {
    render(<ErrorDisplay title="Oops" error={new TypeError("x is undefined")} />);
    expect(screen.getByRole("alert").textContent).toContain("INTERNAL_ERROR");
  });
});

describe("ErrorBoundary", () => {
  it("renders the structured error and recovers on retry", () => {
    let fail = true;
    function Flaky() {
      if (fail) throw new Error("render exploded");
      return <p>recovered</p>;
    }
    const spy = vi.spyOn(console, "error").mockImplementation(() => undefined);
    render(
      <ErrorBoundary>
        <Flaky />
      </ErrorBoundary>,
    );
    expect(screen.getByRole("alert").textContent).toContain("render exploded");
    fail = false;
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    expect(screen.getByText("recovered")).toBeTruthy();
    spy.mockRestore();
  });
});
