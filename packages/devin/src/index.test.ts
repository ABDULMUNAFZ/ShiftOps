import { describe, expect, it } from "vitest";
import { devinSessionSchema, isTerminalStatus } from "./index";

describe("devin session contract", () => {
  it("rejects sessions without a valid url", () => {
    const result = devinSessionSchema.safeParse({
      sessionId: "abc",
      url: "nope",
      status: "running",
      pullRequestUrl: null,
      updatedAt: "2026-01-01T00:00:00Z",
    });
    expect(result.success).toBe(false);
  });

  it("classifies terminal statuses", () => {
    expect(isTerminalStatus("finished")).toBe(true);
    expect(isTerminalStatus("failed")).toBe(true);
    expect(isTerminalStatus("running")).toBe(false);
    expect(isTerminalStatus("blocked")).toBe(false);
  });
});
