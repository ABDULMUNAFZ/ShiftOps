import { describe, expect, it } from "vitest";
import { planWaves } from "./planning";

describe("planWaves", () => {
  it("puts independent repositories in a single wave, sorted", () => {
    expect(planWaves(["web", "api", "cli"], [])).toEqual({ ok: true, waves: [["api", "cli", "web"]] });
  });

  it("orders dependencies before dependents", () => {
    const plan = planWaves(
      ["app", "sdk", "proto", "docs"],
      [
        { from: "app", to: "sdk" },
        { from: "sdk", to: "proto" },
        { from: "docs", to: "sdk" },
      ],
    );
    expect(plan).toEqual({ ok: true, waves: [["proto"], ["sdk"], ["app", "docs"]] });
  });

  it("reports a cycle instead of a partial plan", () => {
    const plan = planWaves(
      ["a", "b", "c", "d"],
      [
        { from: "a", to: "b" },
        { from: "b", to: "c" },
        { from: "c", to: "a" },
        { from: "d", to: "a" },
      ],
    );
    expect(plan.ok).toBe(false);
    if (!plan.ok) expect(plan.cycle).toEqual(["a", "b", "c", "a"]);
  });

  it("reports self-dependency as a cycle", () => {
    expect(planWaves(["a"], [{ from: "a", to: "a" }])).toEqual({ ok: false, cycle: ["a", "a"] });
  });

  it("rejects edges to unknown repositories", () => {
    expect(() => planWaves(["a"], [{ from: "a", to: "ghost" }])).toThrow(/unknown repository/);
  });
});
