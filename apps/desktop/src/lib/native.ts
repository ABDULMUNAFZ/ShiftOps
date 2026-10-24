import {
  activeProcessSchema,
  activityEventSchema,
  agentTaskSchema,
  appErrorSchema,
  appInfoSchema,
  branchInfoSchema,
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
  migrationPatternSchema,
  pageSchema,
  processEventSchema,
  repositoryIntelligenceSchema,
  repositoryRootSchema,
  repositoryScanSchema,
  scanEventSchema,
  repositorySchema,
  ShiftOpsError,
  verificationRunSchema,
  type ActivityQuery,
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
};

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
