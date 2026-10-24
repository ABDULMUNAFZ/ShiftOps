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

// ------------------------------------------------------------ campaign plan

import { planCampaign, type MemberEvidence } from "./planning";

function member(over: Partial<MemberEvidence>): MemberEvidence {
  return {
    repositoryId: over.repositoryId ?? "r1",
    name: over.name ?? over.repositoryId ?? "r1",
    trustStatus: "trusted",
    ownership: "external",
    hasLocalCheckout: true,
    uncommittedChanges: 0,
    lastScannedAt: "2026-10-25T00:00:00.000Z",
    facts: [],
    conflicts: [],
    ...over,
  };
}

const node = (value: string, file = "package.json"): MemberEvidence["facts"][number] => ({
  category: "runtime",
  key: "node",
  value,
  file,
  pointer: "/engines/node",
});
const goal = { runtime: "node", sourceVersion: "18", targetVersion: "22" };

describe("planCampaign", () => {
  it("classifies members against the migration goal", () => {
    const plan = planCampaign(
      [
        member({ repositoryId: "a", facts: [node("18.x")] }),
        member({ repositoryId: "b", facts: [node("22.x")] }),
        member({ repositoryId: "c", facts: [] }),
        member({ repositoryId: "d", facts: [{ category: "runtime", key: "python", value: "3.12" }] }),
      ],
      [],
      goal,
    );
    const byId = new Map(plan.members.map((m) => [m.repositoryId, m]));
    expect(byId.get("a")?.classification).toBe("affected");
    expect(byId.get("b")?.classification).toBe("not_affected");
    expect(byId.get("c")?.classification).toBe("needs_review");
    expect(byId.get("d")?.classification).toBe("not_affected");
  });

  it("puts dependencies before dependents and blockers in wave 0", () => {
    const plan = planCampaign(
      [
        member({ repositoryId: "app", facts: [node("18")] }),
        member({ repositoryId: "sdk", facts: [node("18")] }),
        member({ repositoryId: "nocheckout", hasLocalCheckout: false }),
      ],
      [{ from: "app", to: "sdk", confidence: 0.6, confirmationStatus: "inferred", detectionMethod: "name_match" }],
      goal,
    );
    const byId = new Map(plan.members.map((m) => [m.repositoryId, m]));
    expect(byId.get("nocheckout")?.waveNumber).toBe(0);
    expect(byId.get("nocheckout")?.blockedReason).toContain("checkout");
    expect(byId.get("sdk")?.waveNumber).toBe(1);
    expect(byId.get("app")?.waveNumber).toBe(2);
    expect(plan.dependents["sdk"]).toEqual(["app"]);
    expect(plan.cycle).toBeNull();
  });

  it("blocks cycle members at wave 0 and orders the rest", () => {
    const plan = planCampaign(
      [
        member({ repositoryId: "a", facts: [node("18")] }),
        member({ repositoryId: "b", facts: [node("18")] }),
        member({ repositoryId: "c", facts: [node("18")] }),
        member({ repositoryId: "free", facts: [node("18")] }),
      ],
      [
        { from: "a", to: "b", confidence: 1, confirmationStatus: "confirmed", detectionMethod: "t" },
        { from: "b", to: "a", confidence: 1, confirmationStatus: "confirmed", detectionMethod: "t" },
      ],
      goal,
    );
    const byId = new Map(plan.members.map((m) => [m.repositoryId, m]));
    expect(plan.cycle).not.toBeNull();
    expect(byId.get("a")?.waveNumber).toBe(0);
    expect(byId.get("b")?.waveNumber).toBe(0);
    expect(byId.get("c")?.waveNumber).toBe(1);
    expect(byId.get("free")?.waveNumber).toBe(1);
  });

  it("scores risk from evidence and requires review for high risk", () => {
    const plan = planCampaign(
      [
        member({
          repositoryId: "hot",
          facts: [node("18"), { category: "manifest", key: "package.json", value: "x" }],
          conflicts: [
            { subject: "node", message: "18 vs 22" },
            { subject: "pm", message: "npm vs pnpm" },
          ],
          uncommittedChanges: 7,
        }),
      ],
      [],
      goal,
    );
    const [m] = plan.members;
    if (!m) throw new Error("missing member");
    expect(m.riskScore).toBeGreaterThanOrEqual(0.5);
    expect(m.reviewRequired).toBe(true);
    expect(m.reasons.join(" ")).toMatch(/uncommitted|conflict|lockfile|CI/);
  });

  it("reports edges outside the campaign as unknown, not certain", () => {
    const plan = planCampaign(
      [member({ repositoryId: "a", facts: [node("18")] })],
      [{ from: "a", to: "outside", confidence: 0.5, confirmationStatus: "inferred", detectionMethod: "name_match" }],
      goal,
    );
    expect(plan.usedEdges).toHaveLength(0);
    expect(plan.unknownEdges).toEqual([{ from: "a", to: "outside", detectionMethod: "name_match" }]);
    expect(plan.members[0]?.reviewRequired).toBe(true);
    expect(plan.members[0]?.reasons.join(" ")).toContain("outside this campaign");
  });

  it("ignores rejected edges", () => {
    const plan = planCampaign(
      [member({ repositoryId: "a", facts: [node("18")] }), member({ repositoryId: "b", facts: [node("18")] })],
      [{ from: "a", to: "b", confidence: 0.9, confirmationStatus: "rejected", detectionMethod: "t" }],
      goal,
    );
    expect(plan.usedEdges).toHaveLength(0);
    expect(plan.members.every((m) => m.waveNumber === 1)).toBe(true);
  });

  it("is deterministic for the same evidence", () => {
    const ms = [member({ repositoryId: "z", facts: [node("18")] }), member({ repositoryId: "y", facts: [node("18")] })];
    const es = [{ from: "z", to: "y", confidence: 0.5, confirmationStatus: "inferred" as const, detectionMethod: "t" }];
    expect(planCampaign(ms, es, goal)).toEqual(planCampaign(ms, es, goal));
  });

  it("handles a member with no evidence at all", () => {
    const plan = planCampaign([member({ repositoryId: "empty" })], [], goal);
    expect(plan.members[0]?.classification).toBe("needs_review");
    expect(plan.members[0]?.waveNumber).toBe(1);
    expect(plan.members[0]?.reviewRequired).toBe(true);
  });
});
