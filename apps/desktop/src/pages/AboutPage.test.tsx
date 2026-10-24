import { screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { renderWithClient } from "../test/render";

const native = vi.hoisted(() => ({ appInfo: vi.fn(), openExternalUrl: vi.fn() }));
vi.mock("../lib/native", () => ({ native }));

const { AboutPage } = await import("./AboutPage");

describe("AboutPage", () => {
  it("shows identity, the event credit, independence and real build facts", async () => {
    native.appInfo.mockResolvedValue({
      version: "0.1.0",
      dataDir: "/data",
      logDir: "/logs",
      databasePath: "/data/shiftops.db",
      schemaVersion: 4,
      latestSchemaVersion: 4,
    });
    renderWithClient(<AboutPage />);
    expect(screen.getByText("Define the change once. Ship it everywhere.")).toBeTruthy();
    expect(screen.getByText("Built for Devin Hacks Kochi 2026")).toBeTruthy();
    expect(screen.getByText(/not an official Cognition product/)).toBeTruthy();
    expect(screen.getByText("Devin integration is not available in this build.")).toBeTruthy();
    expect(await screen.findByText("4 (latest 4)")).toBeTruthy();
  });
});
