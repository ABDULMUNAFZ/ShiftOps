import { describe, expect, it } from "vitest";
import { ShiftOpsError, appErrorSchema, toAppError } from "./errors";
import { credentialMetadataSchema, processEventSchema } from "./domain";

describe("error contract", () => {
  it("round-trips a native error into ShiftOpsError", () => {
    const data = appErrorSchema.parse({
      code: "COMMAND_TIMED_OUT",
      message: "git --version timed out after 10 s.",
      technicalDetails: null,
      suggestedAction: "Increase the execution timeout.",
      recoverable: true,
      context: { timeoutSeconds: 10 },
    });
    const err = new ShiftOpsError(data);
    expect(err).toBeInstanceOf(Error);
    expect(toAppError(err)).toEqual(data);
  });

  it("rejects unknown codes", () => {
    expect(
      appErrorSchema.safeParse({
        code: "NOPE",
        message: "x",
        technicalDetails: null,
        suggestedAction: null,
        recoverable: true,
      }).success,
    ).toBe(false);
  });

  it("wraps unexpected throwables as INTERNAL_ERROR", () => {
    expect(toAppError(new TypeError("boom")).code).toBe("INTERNAL_ERROR");
    expect(toAppError("plain string").message).toBe("plain string");
  });
});

describe("domain contracts", () => {
  it("refuses credential metadata that carries a secret field", () => {
    const base = {
      provider: "github",
      kind: "github_personal_access_token",
      identifier: "github:github_personal_access_token",
      exists: true,
      manageable: true,
      createdAt: null,
      updatedAt: null,
      lastValidatedAt: null,
      lastValidationStatus: "not_validated",
    };
    expect(credentialMetadataSchema.safeParse(base).success).toBe(true);
    expect(credentialMetadataSchema.safeParse({ ...base, secret: "ghp_x" }).success).toBe(false);
  });

  it("parses each process event variant", () => {
    expect(
      processEventSchema.parse({ type: "output", runId: "r", seq: 1, lines: [{ stream: "stderr", text: "x" }] }).type,
    ).toBe("output");
    expect(
      processEventSchema.safeParse({ type: "output", runId: "r", seq: 1, lines: [{ stream: "stdin", text: "x" }] })
        .success,
    ).toBe(false);
  });
});
