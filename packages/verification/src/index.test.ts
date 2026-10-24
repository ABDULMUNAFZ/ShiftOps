import { describe, expect, it } from "vitest";
import { evaluatePolicy, verificationPolicySchema } from "./index";

const policy = verificationPolicySchema.parse({
  name: "node",
  steps: [
    { name: "install", command: "pnpm install --frozen-lockfile" },
    { name: "test", command: "pnpm test" },
    { name: "lint", command: "pnpm lint", required: false },
  ],
});

const ok = (name: string) => ({ name, exitCode: 0, timedOut: false, durationMs: 1 });

describe("evaluatePolicy", () => {
  it("applies schema defaults", () => {
    expect(policy.steps[0]).toMatchObject({ required: true, timeoutSeconds: 600 });
  });

  it("passes when all required steps exit 0", () => {
    const result = evaluatePolicy(policy, [ok("install"), ok("test"), { ...ok("lint"), exitCode: 1 }]);
    expect(result).toEqual({ verdict: "passed", failedRequired: [], failedOptional: ["lint"], missing: [] });
  });

  it("fails on a required non-zero exit or timeout", () => {
    const result = evaluatePolicy(policy, [ok("install"), { ...ok("test"), timedOut: true, exitCode: null }, ok("lint")]);
    expect(result.verdict).toBe("failed");
    expect(result.failedRequired).toEqual(["test"]);
  });

  it("is incomplete when a step never ran", () => {
    expect(evaluatePolicy(policy, [ok("install")]).verdict).toBe("incomplete");
  });
});
