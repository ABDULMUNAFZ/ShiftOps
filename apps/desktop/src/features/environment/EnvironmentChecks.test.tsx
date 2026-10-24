import type { EnvironmentReport } from "@shiftops/core";
import { render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { EnvironmentCheckList } from "./EnvironmentChecks";

// Test input only: the shape the Rust command returns.
const report: EnvironmentReport = {
  collectedAt: "2026-10-24T12:00:00Z",
  checks: [
    {
      id: "git",
      label: "Git",
      status: "available",
      version: "2.50.1",
      detail: "git version 2.50.1",
      path: "/usr/bin/git",
      command: "/usr/bin/git --version",
      durationMs: 12,
    },
    {
      id: "devin",
      label: "Devin CLI",
      status: "unavailable",
      version: null,
      detail: "`devin` was not found in 14 directories searched (login shell PATH + process PATH).",
      path: null,
      command: null,
      durationMs: 0,
    },
  ],
};

describe("EnvironmentCheckList", () => {
  it("renders each check with its real status, version, path and command", () => {
    render(<EnvironmentCheckList report={report} />);
    const rows = screen.getAllByRole("listitem");
    expect(rows).toHaveLength(2);

    const [gitRow, devinRow] = rows;
    if (!gitRow || !devinRow) throw new Error("expected two rows");
    const git = within(gitRow);
    expect(git.getByText("Available")).toBeTruthy();
    expect(git.getByText("2.50.1")).toBeTruthy();
    expect(git.getByText("$ /usr/bin/git --version")).toBeTruthy();
    expect(git.getByText("Required")).toBeTruthy();

    const devin = within(devinRow);
    expect(devin.getByText("Unavailable")).toBeTruthy();
    expect(devin.getByText(/was not found/)).toBeTruthy();
    expect(devin.queryByText("Required")).toBeNull();
  });
});
