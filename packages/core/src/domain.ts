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
  rootId: z.string().nullable(),
  gitKind: z.enum(["standard", "worktree", "submodule", "git_file"]).nullable(),
  isDetached: z.boolean(),
  isDirty: z.boolean().nullable(),
  stagedCount: z.number().int().nonnegative().nullable(),
  modifiedCount: z.number().int().nonnegative().nullable(),
  untrackedCount: z.number().int().nonnegative().nullable(),
  lastCommitAt: z.string().nullable(),
  gitCollectedAt: timestamp.nullable(),
  gitWarnings: z.array(z.string()),
  lastScanStatus: z.enum(["queued", "running", "completed", "failed", "cancelled"]).nullable(),
  lastScannedAt: timestamp.nullable(),
  scanSummary: z.lazy(() => scanSummarySchema).nullable(),
  ownership: z.enum(["external", "shiftops_managed"]).nullable(),
  githubLinkMethod: z.enum(["remote_match", "override", "managed_clone"]).nullable(),
});
export type Repository = z.infer<typeof repositorySchema>;

// ---------------------------------------------------------------- repository intelligence

export const scanWarningSchema = z.object({ path: z.string(), message: z.string() });

export const scanSummarySchema = z.object({
  scannerVersion: z.string(),
  cacheKey: z.string(),
  primaryLanguage: z.string().nullable(),
  languages: z.array(z.object({ name: z.string(), files: z.number().int().nonnegative() })),
  runtimes: z.record(z.string(), z.array(z.string())),
  packageManagers: z.array(z.string()),
  frameworks: z.array(z.string()),
  hasDocker: z.boolean(),
  ci: z.array(z.string()),
  facts: z.number().int().nonnegative(),
  conflicts: z.number().int().nonnegative(),
  relevantFiles: z.number().int().nonnegative(),
  entriesSeen: z.number().int().nonnegative(),
  truncated: z.boolean(),
  warnings: z.array(scanWarningSchema),
  durationMs: z.number().int().nonnegative().optional(),
});
export type ScanSummary = z.infer<typeof scanSummarySchema>;

export const factCategorySchema = z.enum([
  "language",
  "framework",
  "runtime",
  "package_manager",
  "workspace",
  "manifest",
  "lockfile",
  "docker",
  "ci",
  "script",
  "dependency",
  "metadata",
]);
export type FactCategory = z.infer<typeof factCategorySchema>;

export const repositoryFactSchema = z.object({
  id: z.string(),
  category: factCategorySchema,
  key: z.string(),
  value: z.string(),
  file: z.string().nullable(),
  line: z.number().int().positive().nullable(),
  pointer: z.string().nullable(),
  method: z.string(),
  confidence: z.number().min(0).max(1),
});
export type RepositoryFact = z.infer<typeof repositoryFactSchema>;

export const repositoryConflictSchema = z.object({
  id: z.string(),
  subject: z.string(),
  message: z.string(),
  values: z.array(
    z.object({
      value: z.string(),
      file: z.string().nullable(),
      line: z.number().int().positive().nullable(),
      method: z.string(),
    }),
  ),
});
export type RepositoryConflict = z.infer<typeof repositoryConflictSchema>;

export const repositoryScanSchema = z.object({
  id: z.string(),
  repositoryId: z.string(),
  scannedCommit: z.string().nullable(),
  relevantFilesHash: z.string().nullable(),
  status: z.enum(["queued", "running", "completed", "failed", "cancelled"]),
  result: scanSummarySchema.nullable(),
  errorCode: z.string().nullable(),
  errorMessage: z.string().nullable(),
  startedAt: timestamp,
  completedAt: timestamp.nullable(),
});
export type RepositoryScan = z.infer<typeof repositoryScanSchema>;

export const repositoryIntelligenceSchema = z.object({
  scan: repositoryScanSchema.nullable(),
  latestAttempt: repositoryScanSchema.nullable(),
  facts: z.array(repositoryFactSchema),
  conflicts: z.array(repositoryConflictSchema),
  scanRunning: z.boolean(),
});
export type RepositoryIntelligence = z.infer<typeof repositoryIntelligenceSchema>;

export const scanEventSchema = z.discriminatedUnion("type", [
  z.object({
    type: z.literal("progress"),
    repositoryId: z.string(),
    entriesSeen: z.number().int().nonnegative(),
    currentPath: z.string(),
  }),
  z.object({
    type: z.literal("completed"),
    outcome: z.object({
      repositoryId: z.string(),
      status: z.enum(["completed", "cached", "failed", "cancelled"]),
      scan: repositoryScanSchema.nullable(),
      error: appErrorSchema.nullable(),
    }),
  }),
]);
export type ScanEvent = z.infer<typeof scanEventSchema>;

// ---------------------------------------------------------------- local discovery

export const repositoryRootSchema = z.object({
  id: z.string(),
  canonicalPath: z.string(),
  displayName: z.string(),
  rootKind: z.enum(["repository", "directory"]),
  maxDepth: z.number().int().min(0).max(12),
  followInternalSymlinks: z.boolean(),
  createdAt: timestamp,
  updatedAt: timestamp,
  lastDiscoveredAt: timestamp.nullable(),
  repositoryCount: z.number().int().nonnegative(),
});
export type RepositoryRoot = z.infer<typeof repositoryRootSchema>;

export const remoteInfoSchema = z.object({
  sanitizedUrl: z.string(),
  host: z.string().nullable(),
  owner: z.string().nullable(),
  repo: z.string().nullable(),
});

export const gitMetadataSchema = z.object({
  currentBranch: z.string().nullable(),
  detached: z.boolean(),
  headCommit: z.string().nullable(),
  defaultBranch: z.string().nullable(),
  origin: remoteInfoSchema.nullable(),
  lastCommitAt: z.string().nullable(),
  workingTree: z
    .object({
      staged: z.number().int().nonnegative(),
      modified: z.number().int().nonnegative(),
      untracked: z.number().int().nonnegative(),
    })
    .nullable(),
  warnings: z.array(z.string()),
});
export type GitMetadata = z.infer<typeof gitMetadataSchema>;

export const discoveryModeSchema = z.enum(["repository", "directory"]);
export type DiscoveryMode = z.infer<typeof discoveryModeSchema>;

export const discoveryCandidateSchema = z.object({
  path: z.string(),
  name: z.string(),
  gitKind: z.enum(["standard", "worktree", "submodule", "git_file"]),
  depth: z.number().int().nonnegative(),
  alreadyImported: z.boolean(),
  metadata: gitMetadataSchema,
});
export type DiscoveryCandidate = z.infer<typeof discoveryCandidateSchema>;

export const walkWarningSchema = z.object({
  path: z.string(),
  code: z.enum(["inaccessible", "symlink_escape", "broken_symlink", "invalid_git_marker", "entry_limit"]),
  message: z.string(),
});
export type WalkWarning = z.infer<typeof walkWarningSchema>;

export const walkStatsSchema = z.object({
  directoriesVisited: z.number().int().nonnegative(),
  entriesSeen: z.number().int().nonnegative(),
  skippedDirectories: z.number().int().nonnegative(),
  depthLimitedDirectories: z.number().int().nonnegative(),
});

export const discoveryResultSchema = z.object({
  discoveryId: z.string(),
  rootPath: z.string(),
  mode: discoveryModeSchema,
  maxDepth: z.number().int(),
  followInternalSymlinks: z.boolean(),
  status: z.enum(["running", "completed", "cancelled", "failed"]),
  candidates: z.array(discoveryCandidateSchema),
  warnings: z.array(walkWarningSchema),
  stats: walkStatsSchema,
  truncated: z.boolean(),
  startedAt: timestamp,
  completedAt: timestamp.nullable(),
  durationMs: z.number().int().nonnegative(),
  error: appErrorSchema.nullable(),
});
export type DiscoveryResult = z.infer<typeof discoveryResultSchema>;

export const discoveryEventSchema = z.discriminatedUnion("type", [
  z.object({
    type: z.literal("progress"),
    discoveryId: z.string(),
    phase: z.enum(["walking", "inspecting"]),
    currentPath: z.string(),
    stats: walkStatsSchema,
    repositoriesFound: z.number().int().nonnegative(),
    inspected: z.number().int().nonnegative(),
  }),
  z.object({ type: z.literal("completed"), result: discoveryResultSchema }),
]);
export type DiscoveryEvent = z.infer<typeof discoveryEventSchema>;

export const importSummarySchema = z.object({
  root: repositoryRootSchema,
  imported: z.number().int().nonnegative(),
  updated: z.number().int().nonnegative(),
  repositoryIds: z.array(z.string()),
});
export type ImportSummary = z.infer<typeof importSummarySchema>;

export const branchInfoSchema = z.object({
  name: z.string(),
  commit: z.string(),
  committedAt: z.string().nullable(),
  upstream: z.string().nullable(),
});
export type BranchInfo = z.infer<typeof branchInfoSchema>;

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
  reviewRequired: z.boolean(),
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
  instructionsHash: z.string().nullable(),
  approvedAt: timestamp.nullable(),
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

// ---------------------------------------------------------------- GitHub

export const githubAccountSchema = z.object({
  id: z.string(),
  host: z.string(),
  apiBaseUrl: z.string(),
  githubUserId: z.number().int(),
  login: z.string(),
  name: z.string().nullable(),
  htmlUrl: z.string().nullable(),
  authMethod: z.enum(["oauth_device", "personal_access_token"]),
  credentialKind: z.enum(["github_oauth_token", "github_personal_access_token"]),
  scopes: z.array(z.string()),
  scopesReported: z.boolean(),
  status: z.enum(["connected", "invalid", "disconnected"]),
  validatedAt: timestamp.nullable(),
  rateLimit: z.record(z.string(), z.unknown()).nullable(),
  createdAt: timestamp,
  updatedAt: timestamp,
});
export type GithubAccount = z.infer<typeof githubAccountSchema>;

export const githubStatusSchema = z.object({
  host: z.string(),
  apiBaseUrl: z.string(),
  oauthConfigured: z.boolean(),
  account: githubAccountSchema.nullable(),
  credentialStored: z.boolean(),
});
export type GithubStatus = z.infer<typeof githubStatusSchema>;

export const deviceFlowStartSchema = z
  .object({
    flowId: z.string(),
    userCode: z.string(),
    verificationUri: z.string(),
    expiresInSeconds: z.number().int().positive(),
    intervalSeconds: z.number().int().positive(),
  })
  .strict();
export type DeviceFlowStart = z.infer<typeof deviceFlowStartSchema>;

export const deviceFlowEventSchema = z.object({
  flowId: z.string(),
  state: z.enum(["pending", "slow_down", "succeeded", "expired", "denied", "cancelled", "failed"]),
  intervalSeconds: z.number().int().nonnegative(),
  account: githubAccountSchema.nullable(),
  error: appErrorSchema.nullable(),
});
export type DeviceFlowEvent = z.infer<typeof deviceFlowEventSchema>;

export const githubOrganizationSchema = z.object({
  id: z.string(),
  githubId: z.number().int(),
  login: z.string(),
  description: z.string().nullable(),
  syncedAt: timestamp,
});
export type GithubOrganization = z.infer<typeof githubOrganizationSchema>;

export const githubRepositorySchema = z.object({
  id: z.string(),
  accountId: z.string(),
  githubId: z.number().int(),
  owner: z.string(),
  name: z.string(),
  fullName: z.string(),
  private: z.boolean(),
  archived: z.boolean(),
  fork: z.boolean(),
  defaultBranch: z.string().nullable(),
  htmlUrl: z.string(),
  cloneUrl: z.string().nullable(),
  sshUrl: z.string().nullable(),
  description: z.string().nullable(),
  primaryLanguage: z.string().nullable(),
  languages: z.record(z.string(), z.number()).nullable(),
  pushedAt: z.string().nullable(),
  remoteUpdatedAt: z.string().nullable(),
  permissions: z.record(z.string(), z.unknown()).nullable(),
  repositoryId: z.string().nullable(),
  syncedAt: timestamp,
});
export type GithubRepository = z.infer<typeof githubRepositorySchema>;

export const githubBrowsePageSchema = z.object({
  items: z.array(githubRepositorySchema),
  page: z.number().int().positive(),
  perPage: z.number().int().positive(),
  hasNextPage: z.boolean(),
  totalCount: z.number().int().nonnegative().nullable(),
});
export type GithubBrowsePage = z.infer<typeof githubBrowsePageSchema>;

export interface GithubBrowseParams {
  owner?: string | undefined;
  query?: string | undefined;
  page?: number | undefined;
  perPage?: number | undefined;
  visibility?: "all" | "public" | "private" | undefined;
  includeArchived?: boolean | undefined;
}

export const githubPullRequestSchema = z.object({
  id: z.string(),
  githubRepositoryId: z.string(),
  githubId: z.number().int(),
  number: z.number().int().positive(),
  title: z.string(),
  state: z.enum(["open", "closed", "merged"]),
  draft: z.boolean(),
  authorLogin: z.string().nullable(),
  htmlUrl: z.string(),
  headRef: z.string(),
  headSha: z.string(),
  headRepoFullName: z.string().nullable(),
  baseRef: z.string(),
  baseSha: z.string().nullable(),
  mergeable: z.boolean().nullable(),
  mergeableState: z.string().nullable(),
  checks: z
    .object({
      total: z.number().int(),
      success: z.number().int(),
      failure: z.number().int(),
      pending: z.number().int(),
      neutral: z.number().int(),
    })
    .nullable(),
  remoteCreatedAt: z.string().nullable(),
  remoteUpdatedAt: z.string().nullable(),
  syncedAt: timestamp,
});
export type GithubPullRequest = z.infer<typeof githubPullRequestSchema>;

export const githubRepositoryDetailsSchema = z.object({
  repository: githubRepositorySchema,
  languages: z.record(z.string(), z.number()),
  branches: z.array(z.object({ name: z.string(), commit: z.object({ sha: z.string() }), protected: z.boolean() })),
  branchesTruncated: z.boolean(),
});

export const githubImportResultSchema = z.object({ repository: repositorySchema, created: z.boolean() });

// ---------------------------------------------------------------- managed workspaces and checkout imports

export const managedWorkspaceSchema = z.object({
  id: z.string(),
  canonicalPath: z.string(),
  approvedAt: timestamp,
  createdAt: timestamp,
});
export type ManagedWorkspace = z.infer<typeof managedWorkspaceSchema>;

export const repositoryCloneSchema = z.object({
  id: z.string(),
  repositoryId: z.string().nullable(),
  githubRepositoryId: z.string().nullable(),
  workspaceId: z.string(),
  fullName: z.string(),
  cloneUrl: z.string(),
  destinationPath: z.string(),
  status: z.enum(["running", "succeeded", "failed", "cancelled", "cleaned_up"]),
  runId: z.string().nullable(),
  errorCode: z.string().nullable(),
  errorMessage: z.string().nullable(),
  partialPathExists: z.boolean(),
  startedAt: timestamp,
  completedAt: timestamp.nullable(),
  cleanedAt: timestamp.nullable(),
});
export type RepositoryClone = z.infer<typeof repositoryCloneSchema>;

export const clonePreviewSchema = z.object({
  githubRepositoryId: z.string(),
  fullName: z.string(),
  private: z.boolean(),
  workspace: managedWorkspaceSchema.nullable(),
  cloneUrl: z.string().nullable(),
  destination: z.string().nullable(),
  repositoryId: z.string().nullable(),
  problems: z.array(z.string()),
  ready: z.boolean(),
});
export type ClonePreview = z.infer<typeof clonePreviewSchema>;

export const linkIdentitySchema = z.object({ host: z.string(), owner: z.string(), repo: z.string() });
export const linkInspectionSchema = z.object({
  githubRepositoryId: z.string(),
  path: z.string(),
  gitKind: z.enum(["standard", "worktree", "submodule", "git_file"]),
  remoteUrl: z.string().nullable(),
  expected: linkIdentitySchema,
  actual: linkIdentitySchema.nullable(),
  actualDisplay: z.string(),
  matches: z.boolean(),
  warnings: z.array(z.string()),
});
export type LinkInspection = z.infer<typeof linkInspectionSchema>;

export const cloneResultSchema = z.object({
  clone: repositoryCloneSchema,
  repository: repositorySchema.nullable(),
  error: appErrorSchema.nullable(),
  scanStarted: z.boolean(),
});
export const cloneEventSchema = z.discriminatedUnion("type", [
  z.object({
    type: z.literal("progress"),
    cloneId: z.string(),
    phase: z.string().nullable(),
    percent: z.number().int().min(0).max(100).nullable(),
    line: z.string(),
  }),
  z.object({ type: z.literal("completed"), result: cloneResultSchema }),
]);
export type CloneEvent = z.infer<typeof cloneEventSchema>;

export const submissionReadinessSchema = z.object({
  status: z.enum(["not_ready", "needs_participant_input", "ready_for_final_verification", "submitted"]),
  sourceRoot: z.string(),
  requiredFiles: z.record(z.string(), z.boolean()),
  remoteUrl: z.string().nullable(),
  headSha: z.string().nullable(),
  originMainSha: z.string().nullable(),
  workingTreeClean: z.boolean(),
  commitsAhead: z.number().int().nonnegative().nullable(),
  pushed: z.boolean(),
  todoCount: z.number().int().nonnegative(),
  secretFindings: z.array(z.object({ path: z.string(), rule: z.string() })),
  trackedFilesScanned: z.number().int().nonnegative(),
  validation: z
    .object({ commitSha: z.string(), recordedAt: timestamp, passed: z.boolean(), summary: z.string() })
    .nullable(),
  finalSha: z.string().nullable(),
  reasons: z.array(z.string()),
});
export type SubmissionReadiness = z.infer<typeof submissionReadinessSchema>;

// ------------------------------------------------------------ task 4: planning & devin

export const repositoryDependencySchema = z.object({
  id: z.string(),
  sourceRepositoryId: z.string(),
  targetRepositoryId: z.string(),
  relationshipType: z.string(),
  confidence: z.number().min(0).max(1),
  evidence: json,
  detectionMethod: z.string(),
  confirmationStatus: z.enum(["inferred", "confirmed", "rejected"]),
  createdAt: timestamp,
  updatedAt: timestamp,
});
export type RepositoryDependency = z.infer<typeof repositoryDependencySchema>;

export const planEvidenceMemberSchema = z.object({
  memberId: z.string(),
  repositoryId: z.string(),
  name: z.string(),
  trustStatus: z.string(),
  ownership: z.string().nullable(),
  hasLocalCheckout: z.boolean(),
  uncommittedChanges: z.number().int().nonnegative().nullable(),
  lastScannedAt: timestamp.nullable(),
  facts: z.array(repositoryFactSchema),
  conflicts: z.array(repositoryConflictSchema),
});
export type PlanEvidenceMember = z.infer<typeof planEvidenceMemberSchema>;

export const campaignPlanEvidenceSchema = z.object({
  campaign: campaignSchema,
  members: z.array(planEvidenceMemberSchema),
  edges: z.array(
    z.object({
      from: z.string(),
      to: z.string(),
      confidence: z.number(),
      confirmationStatus: z.enum(["inferred", "confirmed", "rejected"]),
      detectionMethod: z.string(),
    }),
  ),
});
export type CampaignPlanEvidence = z.infer<typeof campaignPlanEvidenceSchema>;

export const edgeRefreshSchema = z.object({
  repositoriesWithScan: z.number().int().nonnegative(),
  candidates: z.number().int().nonnegative(),
  upserted: z.number().int().nonnegative(),
});
export type EdgeRefresh = z.infer<typeof edgeRefreshSchema>;

export const devinStatusSchema = z.object({
  installed: z.boolean(),
  version: z.string().nullable(),
  authenticated: z.boolean(),
  accountEmail: z.string().nullable(),
  detail: z.string(),
});
export type DevinStatus = z.infer<typeof devinStatusSchema>;

export const agentRunSchema = z.object({
  id: z.string(),
  agentTaskId: z.string(),
  provider: z.string(),
  processId: z.number().int().nullable(),
  externalRunId: z.string().nullable(),
  status: z.enum(["starting", "running", "succeeded", "failed", "cancelled", "timed_out"]),
  exitCode: z.number().int().nullable(),
  startedAt: timestamp,
  completedAt: timestamp.nullable(),
  redactedOutputPath: z.string().nullable(),
  error: json.nullable(),
});
export type AgentRun = z.infer<typeof agentRunSchema>;
