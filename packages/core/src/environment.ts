import { z } from "zod";

/**
 * Result of a single native environment probe. Produced by the Rust
 * `run_environment_checks` command and validated here before display.
 */
export const checkStatusSchema = z.enum(["available", "unavailable", "error"]);
export type CheckStatus = z.infer<typeof checkStatusSchema>;

export const checkIdSchema = z.enum([
  "os",
  "app_data_dir",
  "git",
  "node",
  "pnpm",
  "npm",
  "yarn",
  "docker",
  "devin",
  "keychain",
]);
export type CheckId = z.infer<typeof checkIdSchema>;

export const environmentCheckSchema = z.object({
  id: checkIdSchema,
  label: z.string(),
  status: checkStatusSchema,
  /** Parsed version string, when the tool reports one. */
  version: z.string().nullable(),
  /** Human-readable detail: raw command output or the reason it is unavailable. */
  detail: z.string(),
  /** Absolute path of the resolved executable or directory, when applicable. */
  path: z.string().nullable(),
  /** Exact command that was executed, when the check ran a process. */
  command: z.string().nullable(),
  durationMs: z.number().int().nonnegative(),
});
export type EnvironmentCheck = z.infer<typeof environmentCheckSchema>;

export const environmentReportSchema = z.object({
  /** RFC 3339 timestamp produced by the native side. */
  collectedAt: z.string(),
  checks: z.array(environmentCheckSchema),
});
export type EnvironmentReport = z.infer<typeof environmentReportSchema>;

/** Checks ShiftOps cannot operate without. Others are reported but optional. */
export const REQUIRED_CHECKS: readonly CheckId[] = ["git", "app_data_dir", "keychain"];

export interface ReportSummary {
  available: number;
  unavailable: number;
  errors: number;
  missingRequired: CheckId[];
}

export function summarizeReport(report: EnvironmentReport): ReportSummary {
  const summary: ReportSummary = { available: 0, unavailable: 0, errors: 0, missingRequired: [] };
  for (const check of report.checks) {
    if (check.status === "available") summary.available += 1;
    else if (check.status === "unavailable") summary.unavailable += 1;
    else summary.errors += 1;
    if (check.status !== "available" && REQUIRED_CHECKS.includes(check.id)) {
      summary.missingRequired.push(check.id);
    }
  }
  return summary;
}
