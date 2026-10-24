import { z } from "zod";

/**
 * Devin provider. Step 1 ships the contract only. Delegation is implemented
 * in a later step; until then the desktop app reports Devin as not connected.
 */

export const devinSessionStatusSchema = z.enum(["queued", "running", "blocked", "finished", "failed", "stopped"]);
export type DevinSessionStatus = z.infer<typeof devinSessionStatusSchema>;

export const devinSessionSchema = z.object({
  sessionId: z.string().min(1),
  url: z.url(),
  status: devinSessionStatusSchema,
  pullRequestUrl: z.url().nullable(),
  updatedAt: z.string(),
});
export type DevinSession = z.infer<typeof devinSessionSchema>;

export interface DelegationRequest {
  repository: string;
  baseBranch: string;
  /** Full change specification, written once at campaign level. */
  instructions: string;
  /** Commands the session must run successfully before opening a PR. */
  verificationCommands: string[];
}

export interface DevinProvider {
  readonly id: "devin";
  createSession(request: DelegationRequest): Promise<DevinSession>;
  getSession(sessionId: string): Promise<DevinSession>;
  stopSession(sessionId: string): Promise<void>;
}

const TERMINAL: ReadonlySet<DevinSessionStatus> = new Set(["finished", "failed", "stopped"]);

export function isTerminalStatus(status: DevinSessionStatus): boolean {
  return TERMINAL.has(status);
}
