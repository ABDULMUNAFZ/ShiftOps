import { z } from "zod";

/**
 * Verification is independent of the agent that produced a change: ShiftOps
 * re-runs the policy's commands itself against the resulting branch and only
 * then marks a repository as verified.
 */

export const verificationStepSchema = z.object({
  name: z.string().min(1),
  command: z.string().min(1),
  /** Optional steps are reported but do not fail the repository. */
  required: z.boolean().default(true),
  timeoutSeconds: z.number().int().positive().max(3600).default(600),
});
export type VerificationStep = z.infer<typeof verificationStepSchema>;

export const verificationPolicySchema = z.object({
  name: z.string().min(1),
  steps: z.array(verificationStepSchema).min(1),
});
export type VerificationPolicy = z.infer<typeof verificationPolicySchema>;

export interface StepResult {
  name: string;
  exitCode: number | null;
  timedOut: boolean;
  durationMs: number;
}

export type Verdict = "passed" | "failed" | "incomplete";

export interface PolicyEvaluation {
  verdict: Verdict;
  failedRequired: string[];
  failedOptional: string[];
  missing: string[];
}

/** Combine per-step results into a repository-level verdict. */
export function evaluatePolicy(policy: VerificationPolicy, results: readonly StepResult[]): PolicyEvaluation {
  const byName = new Map(results.map((r) => [r.name, r]));
  const evaluation: PolicyEvaluation = { verdict: "passed", failedRequired: [], failedOptional: [], missing: [] };

  for (const step of policy.steps) {
    const result = byName.get(step.name);
    if (!result) {
      evaluation.missing.push(step.name);
      continue;
    }
    const passed = !result.timedOut && result.exitCode === 0;
    if (!passed) (step.required ? evaluation.failedRequired : evaluation.failedOptional).push(step.name);
  }

  if (evaluation.failedRequired.length > 0) evaluation.verdict = "failed";
  else if (evaluation.missing.length > 0) evaluation.verdict = "incomplete";
  return evaluation;
}
