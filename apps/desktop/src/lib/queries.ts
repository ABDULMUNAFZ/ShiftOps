import {
  parseSetting,
  settingDefaults,
  type ActivityQuery,
  type CredentialKind,
  type CredentialProvider,
  type ListParams,
  type SettingKey,
  type SettingValue,
} from "@shiftops/core";
import { keepPreviousData, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { invalidateAfter } from "./invalidation";
import { native } from "./native";
import { queryKeys } from "./queryKeys";

export { queryKeys } from "./queryKeys";

export type Settings = { [K in SettingKey]: SettingValue<K> };

function decode(raw: Record<string, string>): Settings {
  const out = { ...settingDefaults };
  for (const key of Object.keys(settingDefaults) as SettingKey[]) {
    (out as Record<SettingKey, unknown>)[key] = parseSetting(key, raw[key] ?? null);
  }
  return out;
}

export function useSettings() {
  return useQuery({ queryKey: queryKeys.settings, queryFn: async () => decode(await native.settings()) });
}

export function useSetSetting() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: ({ key, value }: { key: SettingKey; value: unknown }) => native.setSetting(key, value),
    onMutate: async ({ key, value }) => {
      await client.cancelQueries({ queryKey: queryKeys.settings });
      const previous = client.getQueryData<Settings>(queryKeys.settings);
      if (previous) client.setQueryData(queryKeys.settings, { ...previous, [key]: value });
      return { previous };
    },
    onError: (_error, _vars, context) => {
      if (context?.previous) client.setQueryData(queryKeys.settings, context.previous);
    },
    onSettled: (_data, _error, { key }) =>
      invalidateAfter(client, key === "onboardingCompletedAt" ? "onboarding" : "settings"),
  });
}

export function useEnvironment() {
  const client = useQueryClient();
  return useQuery({
    queryKey: queryKeys.environment,
    queryFn: async () => {
      const report = await native.environment();
      void invalidateAfter(client, "environment");
      return report;
    },
    staleTime: Infinity,
  });
}

export function useAppInfo() {
  return useQuery({ queryKey: queryKeys.appInfo, queryFn: native.appInfo, staleTime: Infinity });
}

export function useDatabaseHealth() {
  return useQuery({ queryKey: queryKeys.health.database, queryFn: native.databaseHealth });
}

export function useKeychainStatus() {
  return useQuery({ queryKey: queryKeys.health.keychain, queryFn: native.keychainStatus, staleTime: 60_000 });
}

export function useCredentials() {
  return useQuery({ queryKey: queryKeys.credentials, queryFn: native.listCredentials });
}

export function useStoreCredential() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: ({ provider, kind, secret }: { provider: CredentialProvider; kind: CredentialKind; secret: string }) =>
      native.storeCredential(provider, kind, secret),
    onSettled: () => invalidateAfter(client, "credentials"),
  });
}

export function useDeleteCredential() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: ({ provider, kind }: { provider: CredentialProvider; kind: CredentialKind }) =>
      native.deleteCredential(provider, kind),
    onSettled: () => invalidateAfter(client, "credentials"),
  });
}

export function useActivity(query: ActivityQuery) {
  return useQuery({
    queryKey: queryKeys.activity.list(query),
    queryFn: () => native.listActivity(query),
    placeholderData: keepPreviousData,
  });
}

export function useActivityEntityTypes() {
  return useQuery({ queryKey: queryKeys.activity.entityTypes, queryFn: native.activityEntityTypes });
}

export function useRepositories(params: ListParams) {
  return useQuery({ queryKey: queryKeys.repositories.list(params), queryFn: () => native.listRepositories(params) });
}

export function useCampaigns(params: ListParams) {
  return useQuery({ queryKey: queryKeys.campaigns.list(params), queryFn: () => native.listCampaigns(params) });
}

export function useAgentTasks(params: ListParams) {
  return useQuery({ queryKey: queryKeys.agentTasks.list(params), queryFn: () => native.listAgentTasks(params) });
}

export function useVerificationRuns(params: ListParams) {
  return useQuery({
    queryKey: queryKeys.verificationRuns.list(params),
    queryFn: () => native.listVerificationRuns(params),
  });
}

export function useRepositoryRoots() {
  return useQuery({ queryKey: queryKeys.roots, queryFn: native.listRepositoryRoots });
}

export function useImportDiscovered() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: ({ discoveryId, paths }: { discoveryId: string; paths: string[] }) =>
      native.importDiscovered(discoveryId, paths),
    onSettled: () => invalidateAfter(client, "repositories"),
  });
}

export function useRemoveRepositoryRoot() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => native.removeRepositoryRoot(id),
    onSettled: () => invalidateAfter(client, "repositories"),
  });
}

export function useRepositoryBranches(id: string) {
  return useQuery({ queryKey: queryKeys.repositories.branches(id), queryFn: () => native.listRepositoryBranches(id) });
}

export function useRepository(id: string) {
  return useQuery({ queryKey: queryKeys.repositories.detail(id), queryFn: () => native.getRepository(id) });
}

export function useRepositoryIntelligence(id: string) {
  return useQuery({
    queryKey: queryKeys.repositories.intelligence(id),
    queryFn: () => native.getRepositoryIntelligence(id),
  });
}

export function useGithubStatus() {
  return useQuery({ queryKey: queryKeys.github.status, queryFn: native.githubStatus });
}

export function useGithubOrganizations(enabled: boolean) {
  return useQuery({ queryKey: queryKeys.github.organizations, queryFn: native.githubListOrganizations, enabled });
}

export function useCampaign(id: string) {
  return useQuery({ queryKey: queryKeys.campaigns.detail(id), queryFn: () => native.getCampaign(id) });
}

export function useCampaignMembers(id: string, params: ListParams) {
  return useQuery({
    queryKey: queryKeys.campaigns.members(id, params),
    queryFn: () => native.listCampaignRepositories(id, params),
  });
}

export function useDevinStatus() {
  return useQuery({ queryKey: queryKeys.devin.status, queryFn: native.devinStatus });
}

export function useAgentRuns(taskId: string | null) {
  return useQuery({
    queryKey: queryKeys.agentRuns.list(taskId ?? ""),
    queryFn: () => native.listAgentRuns(taskId ?? "", {}),
    enabled: Boolean(taskId),
  });
}
