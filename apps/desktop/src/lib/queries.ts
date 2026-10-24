import {
  parseSetting,
  settingDefaults,
  type SettingKey,
  type SettingValue,
} from "@shiftops/core";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { native } from "./native";

export const queryKeys = {
  settings: ["settings"] as const,
  environment: ["environment"] as const,
  appInfo: ["app-info"] as const,
  credential: (provider: "github" | "devin") => ["credential", provider] as const,
};

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
    onSettled: () => client.invalidateQueries({ queryKey: queryKeys.settings }),
  });
}

export function useEnvironment() {
  return useQuery({ queryKey: queryKeys.environment, queryFn: native.environment, staleTime: Infinity });
}

export function useAppInfo() {
  return useQuery({ queryKey: queryKeys.appInfo, queryFn: native.appInfo, staleTime: Infinity });
}

export function useCredentialStatus(provider: "github" | "devin") {
  return useQuery({ queryKey: queryKeys.credential(provider), queryFn: () => native.credentialStatus(provider) });
}
