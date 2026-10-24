import type { OutputLine, ProcessEvent, RunResult } from "@shiftops/core";
import { useSyncExternalStore } from "react";

/**
 * In-memory view of process runs started from this window, fed by native
 * process events. Output is capped per run so a chatty command cannot grow
 * memory without bound; the Rust side already enforces the byte limit and
 * batches events every 50 ms.
 */
export const MAX_LINES_PER_RUN = 2000;

export interface RunView {
  runId: string;
  label: string;
  program: string;
  args: string[];
  pid: number | null;
  startedAt: string;
  lines: OutputLine[];
  droppedLines: number;
  truncated: boolean;
  result: RunResult | null;
}

type Listener = () => void;

export function createProcessStore() {
  let runs = new Map<string, RunView>();
  let snapshot: RunView[] = [];
  const listeners = new Set<Listener>();

  const commit = () => {
    snapshot = [...runs.values()].sort((a, b) => b.startedAt.localeCompare(a.startedAt));
    for (const l of listeners) l();
  };

  const update = (runId: string, fn: (run: RunView) => RunView) => {
    const current = runs.get(runId);
    if (!current) return false;
    runs = new Map(runs).set(runId, fn(current));
    return true;
  };

  return {
    apply(event: ProcessEvent) {
      switch (event.type) {
        case "started":
          runs = new Map(runs).set(event.runId, {
            runId: event.runId,
            label: event.label,
            program: event.program,
            args: event.args,
            pid: event.pid,
            startedAt: event.startedAt,
            lines: [],
            droppedLines: 0,
            truncated: false,
            result: null,
          });
          break;
        case "output":
          if (
            !update(event.runId, (run) => {
              const lines = run.lines.concat(event.lines);
              const overflow = Math.max(0, lines.length - MAX_LINES_PER_RUN);
              return {
                ...run,
                lines: overflow ? lines.slice(overflow) : lines,
                droppedLines: run.droppedLines + overflow,
              };
            })
          )
            return;
          break;
        case "truncated":
          if (!update(event.runId, (run) => ({ ...run, truncated: true }))) return;
          break;
        case "completed":
          if (!update(event.result.runId, (run) => ({ ...run, result: event.result }))) return;
          break;
      }
      commit();
    },
    get(runId: string): RunView | undefined {
      return runs.get(runId);
    },
    subscribe(listener: Listener) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    getSnapshot: () => snapshot,
  };
}

export type ProcessStore = ReturnType<typeof createProcessStore>;

export const processStore = createProcessStore();

export function useProcessRuns(store: ProcessStore = processStore): RunView[] {
  return useSyncExternalStore(store.subscribe, store.getSnapshot);
}
