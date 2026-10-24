import type { DiscoveryEvent, DiscoveryMode, DiscoveryResult } from "@shiftops/core";
import { useQueryClient } from "@tanstack/react-query";
import { useCallback, useEffect, useRef, useState } from "react";
import { invalidateAfter } from "../../lib/invalidation";
import { native, onDiscoveryEvent } from "../../lib/native";

export type DiscoveryProgress = Extract<DiscoveryEvent, { type: "progress" }>;

export type DiscoveryState =
  | { phase: "idle" }
  | { phase: "starting"; path: string; mode: DiscoveryMode }
  | { phase: "running"; discoveryId: string; path: string; mode: DiscoveryMode; progress: DiscoveryProgress | null }
  | { phase: "done"; result: DiscoveryResult }
  | { phase: "error"; error: unknown };

const POLL_MS = 1500;

/**
 * Drives one discovery at a time. Progress arrives as native events; the
 * final result is read from events or, if an event was missed (a very fast
 * discovery can finish before `start_discovery` returns), by polling
 * `get_discovery`.
 */
export function useDiscovery() {
  const client = useQueryClient();
  const [state, setState] = useState<DiscoveryState>({ phase: "idle" });
  const currentId = useRef<string | null>(null);
  const finished = useRef(new Map<string, DiscoveryResult>());

  const complete = useCallback(
    (result: DiscoveryResult) => {
      finished.current.set(result.discoveryId, result);
      if (currentId.current === result.discoveryId) {
        currentId.current = null;
        setState({ phase: "done", result });
        void invalidateAfter(client, "discovery");
      }
    },
    [client],
  );

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let disposed = false;
    void onDiscoveryEvent((event) => {
      if (event.type === "completed") {
        complete(event.result);
      } else if (event.discoveryId === currentId.current) {
        setState((s) => (s.phase === "running" ? { ...s, progress: event } : s));
      }
    }).then((fn) => {
      if (disposed) fn();
      else unlisten = fn;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [complete]);

  const runningId = state.phase === "running" ? state.discoveryId : null;
  useEffect(() => {
    if (!runningId) return;
    const timer = setInterval(() => {
      void native
        .getDiscovery(runningId)
        .then((result) => {
          if (result) complete(result);
        })
        .catch(() => undefined);
    }, POLL_MS);
    return () => clearInterval(timer);
  }, [runningId, complete]);

  const start = useCallback(
    async (request: { path: string; mode: DiscoveryMode; maxDepth: number; followInternalSymlinks: boolean }) => {
      setState({ phase: "starting", path: request.path, mode: request.mode });
      try {
        const { discoveryId } = await native.startDiscovery(request);
        currentId.current = discoveryId;
        const early = finished.current.get(discoveryId) ?? (await native.getDiscovery(discoveryId));
        if (early) {
          complete(early);
        } else if (currentId.current === discoveryId) {
          setState({ phase: "running", discoveryId, path: request.path, mode: request.mode, progress: null });
        }
      } catch (error) {
        currentId.current = null;
        setState({ phase: "error", error });
      }
    },
    [complete],
  );

  const cancel = useCallback(async () => {
    if (state.phase === "running") await native.cancelDiscovery(state.discoveryId);
  }, [state]);

  const reset = useCallback(() => {
    currentId.current = null;
    setState({ phase: "idle" });
  }, []);

  return { state, start, cancel, reset };
}
