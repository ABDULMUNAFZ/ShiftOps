import type { ActivityQuery, ListParams } from "@shiftops/core";

/**
 * Every TanStack Query key in the app comes from this factory. Keys are
 * hierarchical so a prefix invalidates every variant beneath it.
 */
export const queryKeys = {
  settings: ["settings"] as const,
  executionLimits: ["settings", "execution-limits"] as const,
  appInfo: ["app-info"] as const,
  environment: ["environment"] as const,
  health: {
    all: ["health"] as const,
    database: ["health", "database"] as const,
    keychain: ["health", "keychain"] as const,
  },
  credentials: ["credentials"] as const,
  activity: {
    all: ["activity"] as const,
    list: (query: ActivityQuery) => ["activity", "list", query] as const,
    entityTypes: ["activity", "entity-types"] as const,
  },
  processes: { active: ["processes", "active"] as const },
  repositories: {
    all: ["repositories"] as const,
    list: (params: ListParams) => ["repositories", "list", params] as const,
    branches: (id: string) => ["repositories", "branches", id] as const,
    detail: (id: string) => ["repositories", "detail", id] as const,
    intelligence: (id: string) => ["repositories", "intelligence", id] as const,
  },
  roots: ["repository-roots"] as const,
  workspace: {
    all: ["managed-workspace"] as const,
    active: ["managed-workspace", "active"] as const,
    clones: ["managed-workspace", "clones"] as const,
  },
  github: {
    all: ["github"] as const,
    status: ["github", "status"] as const,
    organizations: ["github", "organizations"] as const,
    browse: (params: object) => ["github", "browse", params] as const,
    pulls: (owner: string, name: string) => ["github", "pulls", owner, name] as const,
    repositoryFor: (id: string) => ["github", "repository-for", id] as const,
  },
  agentTasks: {
    all: ["agent-tasks"] as const,
    list: (params: ListParams) => ["agent-tasks", "list", params] as const,
  },
  verificationRuns: { list: (params: ListParams) => ["verification-runs", "list", params] as const },
  migrationPatterns: { list: (params: ListParams) => ["migration-patterns", "list", params] as const },
  readiness: ["submission-readiness"] as const,
  campaigns: {
    all: ["campaigns"] as const,
    list: (params: ListParams) => ["campaigns", "list", params] as const,
    detail: (id: string) => ["campaigns", "detail", id] as const,
    members: (id: string, params: ListParams) => ["campaigns", id, "members", params] as const,
    evidence: (id: string) => ["campaigns", id, "evidence"] as const,
  },
  agentRuns: { list: (taskId: string) => ["agent-runs", taskId] as const },
  devin: { status: ["devin", "status"] as const },
  dependencies: {
    list: (repositoryId: string, params: ListParams) => ["dependencies", repositoryId, params] as const,
  },
};
