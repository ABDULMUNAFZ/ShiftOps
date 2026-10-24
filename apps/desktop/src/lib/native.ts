import {
  activeProcessSchema,
  activityEventSchema,
  agentTaskSchema,
  agentRunSchema,
  campaignPlanEvidenceSchema,
  devinStatusSchema,
  edgeRefreshSchema,
  repositoryDependencySchema,
  appErrorSchema,
  appInfoSchema,
  branchInfoSchema,
  cloneEventSchema,
  clonePreviewSchema,
  campaignRepositorySchema,
  campaignSchema,
  credentialMetadataSchema,
  databaseHealthSchema,
  deviceFlowEventSchema,
  deviceFlowStartSchema,
  discoveryEventSchema,
  discoveryResultSchema,
  environmentReportSchema,
  executionLimitsSchema,
  githubAccountSchema,
  githubBrowsePageSchema,
  githubImportResultSchema,
  githubOrganizationSchema,
  githubPullRequestSchema,
  githubRepositoryDetailsSchema,
  githubRepositorySchema,
  githubStatusSchema,
  importSummarySchema,
  keychainStatusSchema,
  linkInspectionSchema,
  managedWorkspaceSchema,
  migrationPatternSchema,
  pageSchema,
  processEventSchema,
  repositoryIntelligenceSchema,
  repositoryCloneSchema,
  repositoryRootSchema,
  repositoryScanSchema,
  scanEventSchema,
  submissionReadinessSchema,
  repositorySchema,
  ShiftOpsError,
  verificationRunSchema,
  type ActivityQuery,
  type CloneEvent,
  type CredentialKind,
  type CredentialProvider,
  type DiagnosticKind,
  type DiscoveryEvent,
  type DiscoveryMode,
  type DeviceFlowEvent,
  type GithubBrowseParams,
  type ListParams,
  type ProcessEvent,
  type ScanEvent,
} from "@shiftops/core";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { z } from "zod";

/**
 * Typed boundary to the Rust core. Every response is validated with Zod and
 * every rejection is converted to a `ShiftOpsError` carrying the structured
 * error contract. Nothing here runs SQL or touches the filesystem.
 */

export class NativeUnavailableError extends ShiftOpsError {
  constructor() {
    super({
      code: "INTERNAL_ERROR",
      message: "The Tauri native runtime is not available. This page is running in a plain browser.",
      technicalDetails: null,
      suggestedAction: "Start the desktop app with `pnpm dev:desktop`.",
      recoverable: false,
    });
    this.name = "NativeUnavailableError";
  }
}

function toShiftOpsError(command: string, error: unknown): ShiftOpsError {
  const structured = appErrorSchema.safeParse(error);
  if (structured.success) return new ShiftOpsError(structured.data, { cause: error });
  // Tauri itself rejects with a plain string when arguments fail to deserialize.
  return new ShiftOpsError(
    {
      code: "INTERNAL_ERROR",
      message: `${command} failed.`,
      technicalDetails: typeof error === "string" ? error : String(error),
      suggestedAction: "Restart ShiftOps. If the problem persists, report it with the copied details.",
      recoverable: false,
    },
    { cause: error },
  );
}

export async function call<T>(command: string, schema: z.ZodType<T>, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri()) throw new NativeUnavailableError();
  let raw: unknown;
  try {
    raw = await invoke(command, args);
  } catch (error) {
    throw toShiftOpsError(command, error);
  }
  const parsed = schema.safeParse(raw);
  if (!parsed.success) {
    throw new ShiftOpsError({
      code: "INTERNAL_ERROR",
      message: `${command} returned data that does not match the expected contract.`,
      technicalDetails: z.prettifyError(parsed.error),
      suggestedAction: "This is a ShiftOps bug. Report it with the copied details.",
      recoverable: false,
    });
  }
  return parsed.data;
}

const nothing = z.null();

export const cancelResponseSchema = z.object({
  runId: z.string(),
  outcome: z.enum(["requested", "not_running"]),
});

export const native = {
  appInfo: () => call("get_app_info", appInfoSchema),
  settings: () => call("get_settings", z.record(z.string(), z.string())),
  setSetting: (key: string, value: unknown) => call("set_setting", nothing, { key, value: JSON.stringify(value) }),
  executionLimits: () => call("get_execution_limits", executionLimitsSchema),

  environment: () => call("run_environment_checks", environmentReportSchema),
  databaseHealth: () => call("get_database_health", databaseHealthSchema),
  keychainStatus: () => call("get_keychain_status", keychainStatusSchema),

  listCredentials: () => call("list_credentials", z.array(credentialMetadataSchema)),
  storeCredential: (provider: CredentialProvider, kind: CredentialKind, secret: string) =>
    call("store_credential", credentialMetadataSchema, { provider, kind, secret }),
  deleteCredential: (provider: CredentialProvider, kind: CredentialKind) =>
    call("delete_credential", credentialMetadataSchema, { provider, kind, confirm: true }),

  startDiagnostic: (kind: DiagnosticKind) => call("start_diagnostic", z.object({ runId: z.string() }), { kind }),
  cancelProcess: (runId: string) => call("cancel_process", cancelResponseSchema, { runId }),
  activeProcesses: () => call("list_active_processes", z.array(activeProcessSchema)),

  listActivity: (query: ActivityQuery) => call("list_activity", pageSchema(activityEventSchema), { query }),
  activityEntityTypes: () => call("list_activity_entity_types", z.array(z.string())),

  listRepositories: (params: ListParams) => call("list_repositories", pageSchema(repositorySchema), { params }),
  listCampaigns: (params: ListParams) => call("list_campaigns", pageSchema(campaignSchema), { params }),
  getCampaign: (id: string) => call("get_campaign", campaignSchema, { id }),
  listCampaignRepositories: (campaignId: string, params: ListParams) =>
    call("list_campaign_repositories", pageSchema(campaignRepositorySchema), { campaignId, params }),
  createCampaign: (definition: Record<string, unknown>, repositoryIds: string[]) =>
    call("create_campaign", campaignSchema, { definition, repositoryIds }),
  updateCampaign: (id: string, definition: Record<string, unknown>) =>
    call("update_campaign", campaignSchema, { id, definition }),
  deleteCampaign: (id: string) => call("delete_campaign", campaignSchema, { id, confirm: true }),
  setCampaignStatus: (id: string, status: string) => call("set_campaign_status", campaignSchema, { id, status }),
  addCampaignRepositories: (campaignId: string, repositoryIds: string[]) =>
    call("add_campaign_repositories", campaignSchema, { campaignId, repositoryIds }),
  removeCampaignRepository: (memberId: string) =>
    call("remove_campaign_repository", campaignRepositorySchema, { memberId, confirm: true }),
  excludeCampaignRepository: (memberId: string, reason: string) =>
    call("exclude_campaign_repository", campaignRepositorySchema, { memberId, reason, confirm: true }),
  includeCampaignRepository: (memberId: string) =>
    call("include_campaign_repository", campaignRepositorySchema, { memberId }),
  resolveMemberReview: (memberId: string) =>
    call("resolve_member_review", campaignRepositorySchema, { memberId, confirm: true }),
  campaignPlanEvidence: (campaignId: string) =>
    call("campaign_plan_evidence", campaignPlanEvidenceSchema, { campaignId }),
  applyCampaignPlan: (campaignId: string, members: unknown[], markReady: boolean) =>
    call("apply_campaign_plan", campaignSchema, { campaignId, members, markReady }),
  refreshDependencyEdges: () => call("refresh_dependency_edges", edgeRefreshSchema, {}),
  listRepositoryDependencies: (repositoryId: string, params: ListParams) =>
    call("list_repository_dependencies", pageSchema(repositoryDependencySchema), { repositoryId, params }),
  devinStatus: () => call("devin_status", devinStatusSchema),
  createAgentTask: (memberId: string) => call("create_agent_task", agentTaskSchema, { memberId }),
  approveAgentTask: (id: string) => call("approve_agent_task", agentTaskSchema, { id, confirm: true }),
  dispatchAgentTask: (id: string) => call("dispatch_agent_task", agentTaskSchema, { id, confirm: true }),
  cancelAgentTask: (id: string) => call("cancel_agent_task", agentTaskSchema, { id }),
  listAgentRuns: (taskId: string, params: ListParams) =>
    call("list_agent_runs", pageSchema(agentRunSchema), { taskId, params }),
  listAgentTasks: (query: ListParams) => call("list_agent_tasks", pageSchema(agentTaskSchema), { query }),
  listVerificationRuns: (query: ListParams) =>
    call("list_verification_runs", pageSchema(verificationRunSchema), { query }),
  listMigrationPatterns: (params: ListParams) =>
    call("list_migration_patterns", pageSchema(migrationPatternSchema), { params }),

  openExternalUrl: (url: string) => call("open_external_url", nothing, { url }),

  pickDirectory: (title: string) => call("pick_directory", z.string().nullable(), { title }),
  startDiscovery: (request: { path: string; mode: DiscoveryMode; maxDepth: number; followInternalSymlinks: boolean }) =>
    call("start_discovery", z.object({ discoveryId: z.string() }), { request }),
  cancelDiscovery: (discoveryId: string) =>
    call("cancel_discovery", z.object({ discoveryId: z.string(), outcome: z.enum(["requested", "not_running"]) }), {
      discoveryId,
    }),
  getDiscovery: (discoveryId: string) => call("get_discovery", discoveryResultSchema.nullable(), { discoveryId }),
  importDiscovered: (discoveryId: string, paths: string[]) =>
    call("import_discovered", importSummarySchema, { discoveryId, paths }),
  listRepositoryRoots: () => call("list_repository_roots", z.array(repositoryRootSchema)),
  removeRepositoryRoot: (id: string) => call("remove_repository_root", repositoryRootSchema, { id, confirm: true }),
  listRepositoryBranches: (id: string) => call("list_repository_branches", z.array(branchInfoSchema), { id }),
  getRepository: (id: string) => call("get_repository", repositorySchema, { id }),
  startRepositoryScan: (repositoryId: string, force: boolean) =>
    call("start_repository_scan", z.object({ repositoryId: z.string() }), { repositoryId, force }),
  cancelRepositoryScan: (repositoryId: string) =>
    call(
      "cancel_repository_scan",
      z.object({ discoveryId: z.string(), outcome: z.enum(["requested", "not_running"]) }),
      {
        repositoryId,
      },
    ),
  getRepositoryIntelligence: (repositoryId: string) =>
    call("get_repository_intelligence", repositoryIntelligenceSchema, { repositoryId }),
  listRepositoryScans: (repositoryId: string, params: ListParams) =>
    call("list_repository_scans", pageSchema(repositoryScanSchema), { repositoryId, params }),
  setRepositoryTrust: (id: string, trust: "untrusted" | "trusted" | "blocked") =>
    call("set_repository_trust", repositorySchema, { id, trust, confirm: true }),
  removeRepositoryRecord: (id: string) => call("remove_repository_record", repositorySchema, { id, confirm: true }),

  githubStatus: () => call("github_status", githubStatusSchema),
  githubStartDeviceFlow: () => call("github_start_device_flow", deviceFlowStartSchema),
  githubCancelDeviceFlow: (flowId: string) =>
    call(
      "github_cancel_device_flow",
      z.object({ discoveryId: z.string(), outcome: z.enum(["requested", "not_running"]) }),
      {
        flowId,
      },
    ),
  githubConnectPat: (token: string) => call("github_connect_pat", githubAccountSchema, { token }),
  githubRefresh: () => call("github_refresh", githubAccountSchema),
  githubDisconnect: () => call("github_disconnect", githubAccountSchema.nullable(), { confirm: true }),
  githubListOrganizations: () => call("github_list_organizations", z.array(githubOrganizationSchema)),
  githubBrowse: (params: GithubBrowseParams, requestId?: string) =>
    call("github_browse_repositories", githubBrowsePageSchema, { params, requestId: requestId ?? null }),
  githubRepositoryDetails: (owner: string, name: string) =>
    call("github_repository_details", githubRepositoryDetailsSchema, { owner, name, requestId: null }),
  githubListPullRequests: (owner: string, name: string, requestId?: string) =>
    call("github_list_pull_requests", z.array(githubPullRequestSchema), { owner, name, requestId: requestId ?? null }),
  githubGetPullRequest: (owner: string, name: string, number: number) =>
    call("github_get_pull_request", githubPullRequestSchema, { owner, name, number }),
  githubRateLimit: () => call("github_rate_limit", z.record(z.string(), z.unknown())),
  githubImportRepository: (githubRepositoryId: string) =>
    call("github_import_repository", githubImportResultSchema, { githubRepositoryId }),
  githubCancelRequest: (requestId: string) =>
    call(
      "github_cancel_request",
      z.object({ discoveryId: z.string(), outcome: z.enum(["requested", "not_running"]) }),
      {
        requestId,
      },
    ),
  githubRepositoryFor: (repositoryId: string) =>
    call("github_repository_for", githubRepositorySchema.nullable(), { repositoryId }),

  getManagedWorkspace: () => call("get_managed_workspace", managedWorkspaceSchema.nullable()),
  approveManagedWorkspace: (path: string) => call("approve_managed_workspace", managedWorkspaceSchema, { path }),
  previewManagedClone: (githubRepositoryId: string) =>
    call("preview_managed_clone", clonePreviewSchema, { githubRepositoryId }),
  startManagedClone: (githubRepositoryId: string, destination: string) =>
    call("start_managed_clone", z.object({ clone: repositoryCloneSchema }), { githubRepositoryId, destination }),
  cancelManagedClone: (cloneId: string) =>
    call("cancel_managed_clone", z.object({ cloneId: z.string(), outcome: z.enum(["requested", "not_running"]) }), {
      cloneId,
    }),
  listRepositoryClones: (limit = 25) => call("list_repository_clones", z.array(repositoryCloneSchema), { limit }),
  cleanupManagedClone: (cloneId: string, confirmedPath: string) =>
    call(
      "cleanup_managed_clone",
      z.object({ outcome: z.enum(["removed", "already_gone"]), clone: repositoryCloneSchema }),
      { cloneId, confirmedPath },
    ),
  inspectExistingClone: (githubRepositoryId: string, path: string) =>
    call("inspect_existing_clone", linkInspectionSchema, { githubRepositoryId, path }),
  linkExistingClone: (githubRepositoryId: string, path: string, mismatchConfirmation?: string) =>
    call(
      "link_existing_clone",
      z.object({ repository: repositorySchema, inspection: linkInspectionSchema, scanStarted: z.boolean() }),
      { githubRepositoryId, path, mismatchConfirmation: mismatchConfirmation ?? null },
    ),
  getSubmissionReadiness: () => call("get_submission_readiness", submissionReadinessSchema),
  openSubmissionFile: (name: string) => call("open_submission_file", nothing, { name }),
};

export const CLONE_EVENT = "shiftops://clone";

export async function onCloneEvent(handler: (event: CloneEvent) => void): Promise<() => void> {
  if (!isTauri()) return () => undefined;
  return listen<unknown>(CLONE_EVENT, ({ payload }) => {
    const parsed = cloneEventSchema.safeParse(payload);
    if (parsed.success) handler(parsed.data);
    else console.error("Dropped malformed clone event", z.prettifyError(parsed.error));
  });
}

export const GITHUB_AUTH_EVENT = "shiftops://github-auth";

/** Subscribe to validated GitHub device-flow events. Returns an unsubscribe function. */
export async function onGithubAuthEvent(handler: (event: DeviceFlowEvent) => void): Promise<() => void> {
  if (!isTauri()) return () => undefined;
  return listen<unknown>(GITHUB_AUTH_EVENT, ({ payload }) => {
    const parsed = deviceFlowEventSchema.safeParse(payload);
    if (parsed.success) handler(parsed.data);
    else console.error("Dropped malformed GitHub auth event", z.prettifyError(parsed.error));
  });
}

export const SCAN_EVENT = "shiftops://scan";

/** Subscribe to validated repository scan events. Returns an unsubscribe function. */
export async function onScanEvent(handler: (event: ScanEvent) => void): Promise<() => void> {
  if (!isTauri()) return () => undefined;
  return listen<unknown>(SCAN_EVENT, ({ payload }) => {
    const parsed = scanEventSchema.safeParse(payload);
    if (parsed.success) handler(parsed.data);
    else console.error("Dropped malformed scan event", z.prettifyError(parsed.error));
  });
}

export const DISCOVERY_EVENT = "shiftops://discovery";

/** Subscribe to validated discovery events. Returns an unsubscribe function. */
export async function onDiscoveryEvent(handler: (event: DiscoveryEvent) => void): Promise<() => void> {
  if (!isTauri()) return () => undefined;
  return listen<unknown>(DISCOVERY_EVENT, ({ payload }) => {
    const parsed = discoveryEventSchema.safeParse(payload);
    if (parsed.success) handler(parsed.data);
    else console.error("Dropped malformed discovery event", z.prettifyError(parsed.error));
  });
}

export const PROCESS_EVENT = "shiftops://process";

/** Subscribe to validated process events. Returns an unsubscribe function. */
export async function onProcessEvent(handler: (event: ProcessEvent) => void): Promise<() => void> {
  if (!isTauri()) return () => undefined;
  return listen<unknown>(PROCESS_EVENT, ({ payload }) => {
    const parsed = processEventSchema.safeParse(payload);
    if (parsed.success) handler(parsed.data);
    else console.error("Dropped malformed process event", z.prettifyError(parsed.error));
  });
}
