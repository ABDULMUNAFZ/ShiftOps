import { z } from "zod";

/**
 * The structured error contract shared with the Rust core (`src-tauri/src/error.rs`).
 * Every rejected Tauri command resolves to this shape.
 */
export const errorCodes = [
  "DATABASE_UNAVAILABLE",
  "DATABASE_MIGRATION_FAILED",
  "RECORD_NOT_FOUND",
  "VALIDATION_FAILED",
  "CREDENTIAL_STORE_UNAVAILABLE",
  "CREDENTIAL_WRITE_FAILED",
  "CREDENTIAL_READ_FAILED",
  "EXECUTABLE_NOT_FOUND",
  "COMMAND_NOT_ALLOWED",
  "COMMAND_TIMED_OUT",
  "COMMAND_CANCELLED",
  "COMMAND_FAILED",
  "OUTPUT_LIMIT_EXCEEDED",
  "PATH_NOT_ALLOWED",
  "EXTERNAL_URL_REJECTED",
  "NETWORK_FAILED",
  "GITHUB_NOT_CONFIGURED",
  "GITHUB_AUTHENTICATION_FAILED",
  "GITHUB_PERMISSION_DENIED",
  "GITHUB_RATE_LIMITED",
  "GITHUB_AUTHORIZATION_EXPIRED",
  "GITHUB_AUTHORIZATION_DENIED",
  "GIT_AUTHENTICATION_REQUIRED",
  "DEVIN_NOT_CONFIGURED",
  "INTERNAL_ERROR",
] as const;

export const errorCodeSchema = z.enum(errorCodes);
export type ErrorCode = z.infer<typeof errorCodeSchema>;

export const appErrorSchema = z.object({
  code: errorCodeSchema,
  message: z.string(),
  technicalDetails: z.string().nullable(),
  suggestedAction: z.string().nullable(),
  recoverable: z.boolean(),
  context: z.record(z.string(), z.unknown()).optional(),
});
export type AppErrorData = z.infer<typeof appErrorSchema>;

/** Error thrown by the desktop bridge. Carries the full structured contract. */
export class ShiftOpsError extends Error {
  readonly code: ErrorCode;
  readonly technicalDetails: string | null;
  readonly suggestedAction: string | null;
  readonly recoverable: boolean;
  readonly context: Record<string, unknown> | undefined;

  constructor(data: AppErrorData, options?: { cause?: unknown }) {
    super(data.message, options);
    this.name = "ShiftOpsError";
    this.code = data.code;
    this.technicalDetails = data.technicalDetails;
    this.suggestedAction = data.suggestedAction;
    this.recoverable = data.recoverable;
    this.context = data.context;
  }

  toJSON(): AppErrorData {
    return {
      code: this.code,
      message: this.message,
      technicalDetails: this.technicalDetails,
      suggestedAction: this.suggestedAction,
      recoverable: this.recoverable,
      ...(this.context ? { context: this.context } : {}),
    };
  }
}

/** Normalise anything thrown into the structured contract for display. */
export function toAppError(error: unknown): AppErrorData {
  if (error instanceof ShiftOpsError) return error.toJSON();
  const parsed = appErrorSchema.safeParse(error);
  if (parsed.success) return parsed.data;
  const message = error instanceof Error ? error.message : String(error);
  return {
    code: "INTERNAL_ERROR",
    message,
    technicalDetails: error instanceof Error && error.stack ? error.stack : null,
    suggestedAction: "Restart ShiftOps. If the problem persists, report it with the copied details.",
    recoverable: false,
  };
}
