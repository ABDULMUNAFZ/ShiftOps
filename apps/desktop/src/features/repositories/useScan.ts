import type { ScanEvent } from "@shiftops/core";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { invalidateAfter } from "../../lib/invalidation";
import { native, onScanEvent } from "../../lib/native";

type Progress = Extract<ScanEvent, { type: "progress" }>;
type Outcome = Extract<ScanEvent, { type: "completed" }>["outcome"];

/**
 * Starts, cancels and follows the scan of one repository. Progress comes from
 * native events; when a scan finishes the repository queries are refreshed.
 */
export function useRepositoryScan(repositoryId: string) {
  const client = useQueryClient();
  const [progress, setProgress] = useState<Progress | null>(null);
  const [outcome, setOutcome] = useState<Outcome | null>(null);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let disposed = false;
    void onScanEvent((event) => {
      if (event.type === "progress" && event.repositoryId === repositoryId) setProgress(event);
      if (event.type === "completed" && event.outcome.repositoryId === repositoryId) {
        setProgress(null);
        setOutcome(event.outcome);
        void invalidateAfter(client, "scan");
      }
    }).then((fn) => {
      if (disposed) fn();
      else unlisten = fn;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [repositoryId, client]);

  const start = useMutation({
    mutationFn: (force: boolean) => native.startRepositoryScan(repositoryId, force),
    onMutate: () => setOutcome(null),
    onSuccess: () => invalidateAfter(client, "scan"),
  });
  const cancel = useMutation({ mutationFn: () => native.cancelRepositoryScan(repositoryId) });

  return { start, cancel, progress, outcome };
}
