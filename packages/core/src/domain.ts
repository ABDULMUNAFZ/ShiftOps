import { z } from "zod";
import { appErrorSchema } from "./errors";

/**
 * Zod schemas for every model returned by the Rust core. Field names and enum
 * values match the serde output in `src-tauri/src/db/*` and `process/*`.
 */

const timestamp = z.string();
const json = z.unknown();

export function pageSchema<T extends z.ZodType>(item: T) {
  return z.object({
    items: z.array(item),
    total: z.number().int().nonnegative(),
    limit: z.number().int().positive(),
    offset: z.number().int().nonnegative(),
  });
}
export interface Page<T> {
  items: T[];
  total: number;
  limit: number;
  offset: number;
}

export interface ListParams {
  limit?: number;
  offset?: number;
  statuses?: string[];
  search?: string;
}

// ---------------------------------------------------------------- repositories

export const trustStatusSchema = z.enum(["untrusted", "trusted", "blocked"]);
export const repositorySchema = z.object({
  id: z.string(),
  sourceType: z.enum(["local", "github"]),
  name: z.string(),
  localPath: z.string().nullable(),
  remoteUrl: z.string().nullable(),
  githubHost: z.string().nullable(),
  githubOwner: z.string().nullable(),
  githubRepo: z.string().nullable(),
  defaultBranch: z.string().nullable(),
  currentBranch: z.string().nullable(),
  currentCommit: z.string().nullable(),
  trustStatus: trustStatusSchema,
  createdAt: timestamp,
  updatedAt: timestamp,
});
export type Repository = z.infer<typeof repositorySchema>;

// ---------------------------------------------------------------- campaigns

export const campaignStatusSchema = z.enum([
  "draft",
  "planning",
  "ready",
  "running",
  "paused",
  "completed",
  "failed",
  "cancelled",
]);
export const campaignSchema = z.object({
  id: z.string(),
  name: z.string(),
  objective: z.string(),
  migrationType: z.string(),
  sourceCondition: json,
  targetCondition: json,
  constraints: json,
  verificationProfile: z.string(),
  maximumParallelTasks: z.number().int(),
  status: campaignStatusSchema,
  repositoryCount: z.number().int().nonnegative(),
  createdAt: timestamp,
  updatedAt: timestamp,
});
export type Campaign = z.infer<typeof campaignSchema>;

export const campaignRepositorySchema = z.object({
  id: z.string(),
  campaignId: z.string(),
  repositoryId: z.string(),
  repositoryName: z.string(),
  classification: z.enum(["unclassified", "affected", "not_affected", "needs_review"]),
  riskScore: z.number().min(0).max(1).nullable(),
  riskExplanation: json,
  waveNumber: z.number().int().nonnegative().nullable(),
  status: z.enum([
    "pending",
    "excluded",
    "queued",
    "in_progress",
    "awaiting_verification",
    "verified",
    "failed",
    "blocked",
  ]),
  exclusionReason: z.string().nullable(),
  createdAt: timestamp,
  updatedAt: timestamp,
});
export type CampaignRepository = z.infer<typeof campaignRepositorySchema>;

// ---------------------------------------------------------------- agents & verification

export const agentTaskStatusSchema = z.enum([
  "pending",
  "queued",
  "running",
  "awaiting_review",
  "completed",
  "failed",
  "cancelled",
]);
export const agentTaskSchema = z.object({
  id: z.string(),
  campaignRepositoryId: z.string(),
  provider: z.enum(["devin"]),
  executionMode: z.enum(["cloud_session", "local_cli"]),
  instructionsMarkdown: z.string(),
  externalSessionId: z.string().nullable(),
  externalSessionUrl: z.string().nullable(),
  worktreePath: z.string().nullable(),
  branchName: z.string().nullable(),
  commitSha: z.string().nullable(),
  pullRequestUrl: z.string().nullable(),
  status: agentTaskStatusSchema,
  startedAt: timestamp.nullable(),
  completedAt: timestamp.nullable(),
  createdAt: timestamp,
  updatedAt: timestamp,
});
export type AgentTask = z.infer<typeof agentTaskSchema>;

export const verificationRunStatusSchema = z.enum(["queued", "running", "passed", "failed", "error", "cancelled"]);
export const verificationRunSchema = z.object({
  id: z.string(),
  campaignRepositoryId: z.string(),
  agentTaskId: z.string().nullable(),
  profile: z.string(),
  verifiedCommitSha: z.string(),
  status: verificationRunStatusSchema,
  startedAt: timestamp,
  completedAt: timestamp.nullable(),
  summary: json,
});
export type VerificationRun = z.infer<typeof verificationRunSchema>;

export const migrationPatternSchema = z.object({
  id: z.string(),
  name: z.string(),
  applicability: json,
  problemDescription: z.string(),
  solutionMarkdown: z.string(),
  verificationRules: json,
  confidence: z.number().min(0).max(1),
  approvalStatus: z.enum(["candidate", "approved", "rejected", "disabled"]),
  sourceCampaignId: z.string().nullable(),
  sourceRepositoryId: z.string().nullable(),
  createdAt: timestamp,
  updatedAt: timestamp,
});
export type MigrationPattern = z.infer<typeof migrationPatternSchema>;

// ---------------------------------------------------------------- activity

export const severitySchema = z.enum(["info", "notice", "warning", "error"]);
export type Severity = z.infer<typeof severitySchema>;
export const activityEventSchema = z.object({
  id: z.string(),
  entityType: z.string(),
  entityId: z.string().nullable(),
  eventType: z.string(),
  severity: severitySchema,
  summary: z.string(),
  metadata: z.record(z.string(), z.unknown()),
  createdAt: timestamp,
});
export type ActivityEvent = z.infer<typeof activityEventSchema>;

export interface ActivityQuery extends ListParams {
  entityType?: string;
  entityId?: string;
}

// ---------------------------------------------------------------- system health

export const appInfoSchema = z.object({
  version: z.string(),
  dataDir: z.string(),
  logDir: z.string(),
  databasePath: z.string(),
  schemaVersion: z.number().int(),
  latestSchemaVersion: z.number().int(),
});
export type AppInfo = z.infer<typeof appInfoSchema>;

export const databaseHealthSchema = z.object({
  path: z.string(),
  sizeBytes: z.number().int().nonnegative().nullable(),
  schemaVersion: z.number().int(),
  latestSchemaVersion: z.number().int(),
  upToDate: z.boolean(),
  appliedMigrations: z.array(z.object({ version: z.number().int(), name: z.string(), appliedAt: timestamp })),
  foreignKeysEnabled: z.boolean(),
  journalMode: z.string(),
  quickCheck: z.string(),
  activityEventCount: z.number().int().nonnegative(),
});
export type DatabaseHealth = z.infer<typeof databaseHealthSchema>;

export const keychainStatusSchema = z.object({
  available: z.boolean(),
  backend: z.string(),
  service: z.string(),
  error: appErrorSchema.nullable(),
});
export type KeychainStatus = z.infer<typeof keychainStatusSchema>;

export const credentialProviderSchema = z.enum(["github", "devin", "other"]);
export const credentialKindSchema = z.enum([
  "github_oauth_token",
  "github_personal_access_token",
  "devin_api_token",
  "provider_token",
]);
export type CredentialProvider = z.infer<typeof credentialProviderSchema>;
export type CredentialKind = z.infer<typeof credentialKindSchema>;

/** Everything the UI may know about a credential. There is no value field by design. */
export const credentialMetadataSchema = z
  .object({
    provider: credentialProviderSchema,
    kind: credentialKindSchema,
    identifier: z.string(),
    exists: z.boolean(),
    manageable: z.boolean(),
    createdAt: timestamp.nullable(),
    updatedAt: timestamp.nullable(),
    lastValidatedAt: timestamp.nullable(),
    lastValidationStatus: z.enum(["not_validated", "valid", "invalid", "error"]),
  })
  .strict();
export type CredentialMetadata = z.infer<typeof credentialMetadataSchema>;

export const executionLimitsSchema = z.object({
  timeoutSeconds: z.number().int().positive(),
  maxOutputBytes: z.number().int().positive(),
});
export type ExecutionLimits = z.infer<typeof executionLimitsSchema>;

// ---------------------------------------------------------------- processes

export const commandCategorySchema = z.enum([
  "environment_check",
  "readonly_git",
  "mutating_git",
  "dependency_install",
  "build",
  "test",
  "lint",
  "docker",
  "devin",
  "custom",
]);
export const processStreamSchema = z.enum(["stdout", "stderr"]);
export const outputLineSchema = z.object({ stream: processStreamSchema, text: z.string() });
export type OutputLine = z.infer<typeof outputLineSchema>;

export const runStatusSchema = z.enum(["succeeded", "failed", "cancelled", "timed_out"]);
export const runResultSchema = z.object({
  runId: z.string(),
  label: z.string(),
  category: commandCategorySchema,
  status: runStatusSchema,
  exitCode: z.number().int().nullable(),
  durationMs: z.number().int().nonnegative(),
  startedAt: timestamp,
  completedAt: timestamp,
  stdout: z.string(),
  stderr: z.string(),
  outputBytes: z.number().int().nonnegative(),
  outputLimitBytes: z.number().int().nonnegative(),
  outputTruncated: z.boolean(),
  logPath: z.string().nullable(),
  error: appErrorSchema.nullable(),
});
export type RunResult = z.infer<typeof runResultSchema>;

export const processEventSchema = z.discriminatedUnion("type", [
  z.object({
    type: z.literal("started"),
    runId: z.string(),
    label: z.string(),
    category: commandCategorySchema,
    program: z.string(),
    args: z.array(z.string()),
    cwd: z.string(),
    pid: z.number().int().nullable(),
    startedAt: timestamp,
  }),
  z.object({ type: z.literal("output"), runId: z.string(), seq: z.number().int(), lines: z.array(outputLineSchema) }),
  z.object({ type: z.literal("truncated"), runId: z.string(), limitBytes: z.number().int() }),
  z.object({ type: z.literal("completed"), result: runResultSchema }),
]);
export type ProcessEvent = z.infer<typeof processEventSchema>;

export const activeProcessSchema = z.object({
  runId: z.string(),
  label: z.string(),
  category: commandCategorySchema,
  pid: z.number().int().nullable(),
  startedAt: timestamp,
});
export type ActiveProcess = z.infer<typeof activeProcessSchema>;

export const diagnosticKindSchema = z.enum(["git_version", "node_version", "pnpm_version", "long_running"]);
export type DiagnosticKind = z.infer<typeof diagnosticKindSchema>;
