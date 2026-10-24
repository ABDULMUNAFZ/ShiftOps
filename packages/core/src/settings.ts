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
} as const;

export type SettingKey = keyof typeof settingSchemas;
export type SettingValue<K extends SettingKey> = z.infer<(typeof settingSchemas)[K]>;

export const settingDefaults: { [K in SettingKey]: SettingValue<K> } = {
  theme: "system",
  sidebarCollapsed: false,
  onboardingCompletedAt: null,
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
