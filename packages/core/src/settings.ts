import { z } from "zod";

export const themeSchema = z.enum(["light", "dark", "system"]);
export type Theme = z.infer<typeof themeSchema>;

/**
 * Keys persisted in the local SQLite `settings` table. Values are stored as
 * JSON text; each key has a schema so corrupt or stale values fall back to
 * defaults instead of crashing the UI.
 */
export const settingSchemas = {
  theme: themeSchema,
  sidebarCollapsed: z.boolean(),
  onboardingCompletedAt: z.string().nullable(),
  "execution.defaultTimeoutSeconds": z.number().int().min(1).max(3600),
  "execution.maxOutputBytes": z
    .number()
    .int()
    .min(4 * 1024)
    .max(16 * 1024 * 1024),
} as const;

export type SettingKey = keyof typeof settingSchemas;
export type SettingValue<K extends SettingKey> = z.infer<(typeof settingSchemas)[K]>;

export const settingDefaults: { [K in SettingKey]: SettingValue<K> } = {
  theme: "system",
  sidebarCollapsed: false,
  onboardingCompletedAt: null,
  "execution.defaultTimeoutSeconds": 120,
  "execution.maxOutputBytes": 1024 * 1024,
};

export function parseSetting<K extends SettingKey>(key: K, raw: string | null): SettingValue<K> {
  if (raw === null) return settingDefaults[key];
  try {
    const result = settingSchemas[key].safeParse(JSON.parse(raw));
    return result.success ? (result.data as SettingValue<K>) : settingDefaults[key];
  } catch {
    return settingDefaults[key];
  }
}

export const preferencesFormSchema = z.object({
  theme: themeSchema,
  sidebarCollapsed: z.boolean(),
});
export type PreferencesForm = z.infer<typeof preferencesFormSchema>;

/** Execution limits as edited in Settings (inputs registered with `valueAsNumber`). Output size is in KiB. */
export const executionFormSchema = z.object({
  timeoutSeconds: z
    .number({ error: "Enter a whole number of seconds" })
    .int()
    .min(1, "At least 1 second")
    .max(3600, "At most 3600 seconds (1 hour)"),
  maxOutputKib: z
    .number({ error: "Enter a whole number of KiB" })
    .int()
    .min(4, "At least 4 KiB")
    .max(16 * 1024, "At most 16384 KiB (16 MiB)"),
});
export type ExecutionForm = z.infer<typeof executionFormSchema>;
