import { environmentReportSchema, type EnvironmentReport } from "@shiftops/core";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { z } from "zod";

/**
 * Typed boundary to the Rust side. Every response is validated with Zod so a
 * mismatch between Rust and TypeScript surfaces as an explicit error.
 */

export class NativeUnavailableError extends Error {
  constructor() {
    super(
      "The Tauri native runtime is not available. This page is running in a plain browser; start the desktop app with `pnpm dev:desktop`.",
    );
    this.name = "NativeUnavailableError";
  }
}

async function call<T>(command: string, schema: z.ZodType<T>, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri()) throw new NativeUnavailableError();
  let raw: unknown;
  try {
    raw = await invoke(command, args);
  } catch (error) {
    // Rust errors are serialized as plain strings.
    throw new Error(typeof error === "string" ? error : `${command} failed: ${String(error)}`, { cause: error });
  }
  const parsed = schema.safeParse(raw);
  if (!parsed.success) {
    throw new Error(`${command} returned an unexpected shape: ${z.prettifyError(parsed.error)}`);
  }
  return parsed.data;
}

export const appInfoSchema = z.object({
  version: z.string(),
  dataDir: z.string(),
  databasePath: z.string(),
  schemaVersion: z.number().int(),
});
export type AppInfo = z.infer<typeof appInfoSchema>;

export const credentialStatusSchema = z.object({
  provider: z.enum(["github", "devin"]),
  present: z.boolean(),
  error: z.string().nullable(),
});
export type CredentialStatus = z.infer<typeof credentialStatusSchema>;

export const native = {
  appInfo: () => call("get_app_info", appInfoSchema),
  settings: () => call("get_settings", z.record(z.string(), z.string())),
  setSetting: (key: string, value: unknown) =>
    call("set_setting", z.null(), { key, value: JSON.stringify(value) }),
  environment: (): Promise<EnvironmentReport> => call("run_environment_checks", environmentReportSchema),
  credentialStatus: (provider: "github" | "devin") =>
    call("credential_status", credentialStatusSchema, { provider }),
};
