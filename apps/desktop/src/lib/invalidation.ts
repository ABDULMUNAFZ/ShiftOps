import type { QueryClient, QueryKey } from "@tanstack/react-query";
import { queryKeys } from "./queryKeys";

/**
 * Central map from a kind of change to the queries it makes stale. Mutations
 * and native events call `invalidateAfter` instead of listing keys locally,
 * so a new side effect (for example a new activity event) is wired in one place.
 */
export type ChangeKind =
  "settings" | "onboarding" | "credentials" | "environment" | "process" | "discovery" | "repositories" | "scan";

const affected: Record<ChangeKind, readonly QueryKey[]> = {
  settings: [queryKeys.settings],
  onboarding: [queryKeys.settings, queryKeys.activity.all],
  // Credential changes write activity events and change keychain-derived state.
  credentials: [queryKeys.credentials, queryKeys.activity.all, queryKeys.health.database],
  // Each environment run records an activity event.
  environment: [queryKeys.activity.all, queryKeys.health.database],
  // Process start/finish record activity and change the active set.
  process: [queryKeys.processes.active, queryKeys.activity.all, queryKeys.health.database],
  // A finished discovery records an activity event.
  discovery: [queryKeys.activity.all, queryKeys.health.database],
  // Import and root removal change repositories, roots and activity.
  repositories: [queryKeys.repositories.all, queryKeys.roots, queryKeys.activity.all, queryKeys.health.database],
  // A finished scan writes facts, the scan row and an activity event.
  scan: [queryKeys.repositories.all, queryKeys.activity.all, queryKeys.health.database],
};

export function invalidationTargets(kind: ChangeKind): readonly QueryKey[] {
  return affected[kind];
}

export async function invalidateAfter(client: QueryClient, kind: ChangeKind): Promise<void> {
  await Promise.all(affected[kind].map((queryKey) => client.invalidateQueries({ queryKey })));
}
