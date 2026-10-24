import type { Repository } from "@shiftops/core";
import { Badge, Button, Card, CardHeader } from "@shiftops/ui";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useId, useState } from "react";
import { ErrorDisplay } from "../../components/ErrorDisplay";
import { invalidateAfter } from "../../lib/invalidation";
import { native } from "../../lib/native";

type Trust = Repository["trustStatus"];

const TRUST_EFFECT: Record<Trust, string> = {
  trusted:
    "Trusted repositories may run their own package scripts during verification and may be delegated to Devin after approval.",
  untrusted: "Untrusted repositories are only read. Live verification runs and Devin sessions here are cancelled.",
  blocked: "Blocked repositories are only read and can never be verified or delegated. Live work here is cancelled.",
};

/** Confirmed trust changes. The backend enforces the same rules. */
export function TrustControl({ repo }: { repo: Repository }) {
  const client = useQueryClient();
  const [pending, setPending] = useState<Trust | null>(null);
  const trust = useMutation({
    mutationFn: (value: Trust) => native.setRepositoryTrust(repo.id, value),
    onSuccess: async () => {
      setPending(null);
      await invalidateAfter(client, "repositories");
    },
  });
  return (
    <div className="flex flex-col items-end gap-1">
      <label className="flex items-center gap-2 text-xs text-muted">
        Trust
        <select
          aria-label="Repository trust"
          value={pending ?? repo.trustStatus}
          disabled={trust.isPending}
          onChange={(event) => {
            const value = event.target.value as Trust;
            setPending(value === repo.trustStatus ? null : value);
          }}
          className="h-7 rounded-md border border-border bg-surface px-2 text-fg"
        >
          <option value="untrusted">Untrusted</option>
          <option value="trusted">Trusted</option>
          <option value="blocked">Blocked</option>
        </select>
      </label>
      {pending ? (
        <div
          role="alertdialog"
          aria-label="Confirm trust change"
          className="flex max-w-md flex-wrap items-center gap-2 text-xs"
        >
          <span>{TRUST_EFFECT[pending]}</span>
          <Button size="sm" variant="primary" disabled={trust.isPending} onClick={() => trust.mutate(pending)}>
            Set {pending}
          </Button>
          <Button size="sm" onClick={() => setPending(null)}>
            Keep {repo.trustStatus}
          </Button>
        </div>
      ) : null}
      {trust.isError ? <ErrorDisplay title="Trust was not changed" error={trust.error} /> : null}
    </div>
  );
}

/**
 * Metadata and the three separate ways to let go of a repository: unlink the
 * checkout, delete a ShiftOps-managed clone's files, or forget the record.
 */
export function RepositoryManagement({ repo, onForgotten }: { repo: Repository; onForgotten: () => void }) {
  const client = useQueryClient();
  const nameId = useId();
  const notesId = useId();
  const pathId = useId();
  const [displayName, setDisplayName] = useState(repo.displayName ?? "");
  const [notes, setNotes] = useState(repo.notes ?? "");
  const [confirm, setConfirm] = useState<"unlink" | "delete" | "forget" | null>(null);
  const [typedPath, setTypedPath] = useState("");
  const after = () => invalidateAfter(client, "repositories");
  const save = useMutation({
    mutationFn: () => native.updateRepositoryMetadata(repo.id, displayName || null, notes || null),
    onSuccess: after,
  });
  const unlink = useMutation({
    mutationFn: () => native.unlinkRepositoryCheckout(repo.id),
    onSuccess: async () => {
      setConfirm(null);
      await after();
    },
  });
  const remove = useMutation({
    mutationFn: () => native.deleteManagedCheckout(repo.id, typedPath),
    onSuccess: async () => {
      setConfirm(null);
      setTypedPath("");
      await after();
    },
  });
  const forget = useMutation({
    mutationFn: () => native.removeRepositoryRecord(repo.id),
    onSuccess: async () => {
      await after();
      onForgotten();
    },
  });
  const busy = save.isPending || unlink.isPending || remove.isPending || forget.isPending;
  const dirty = displayName !== (repo.displayName ?? "") || notes !== (repo.notes ?? "");
  const managed = repo.ownership === "shiftops_managed";
  const error = save.error ?? unlink.error ?? remove.error ?? forget.error;

  return (
    <Card aria-label="Repository management">
      <CardHeader
        title="Details and removal"
        description={
          repo.localPath
            ? managed
              ? "ShiftOps cloned this checkout into its managed workspace."
              : "External checkout: ShiftOps reads it and never deletes it."
            : "Metadata only: no local checkout."
        }
      />
      <div className="grid gap-4 p-4 text-sm md:grid-cols-2">
        <form
          className="space-y-2"
          onSubmit={(e) => {
            e.preventDefault();
            save.mutate();
          }}
        >
          <label htmlFor={nameId} className="block text-xs font-medium">
            Display name
          </label>
          <input
            id={nameId}
            value={displayName}
            maxLength={255}
            placeholder={repo.name}
            onChange={(e) => setDisplayName(e.target.value)}
            className="h-8 w-full rounded-md border border-border bg-surface px-2"
          />
          <label htmlFor={notesId} className="block text-xs font-medium">
            Notes
          </label>
          <textarea
            id={notesId}
            value={notes}
            maxLength={4000}
            rows={3}
            onChange={(e) => setNotes(e.target.value)}
            className="w-full rounded-md border border-border bg-surface px-2 py-1"
          />
          <div className="flex items-center gap-2">
            <Button size="sm" type="submit" disabled={!dirty || busy}>
              Save details
            </Button>
            {save.isSuccess && !dirty ? (
              <span role="status" className="text-xs text-muted">
                Saved.
              </span>
            ) : null}
          </div>
        </form>
        <div className="space-y-2 text-xs">
          <p className="flex flex-wrap items-center gap-2">
            Checkout
            {repo.localPath === null ? (
              <Badge>none</Badge>
            ) : repo.checkoutAvailable === false ? (
              <Badge tone="danger">missing on disk</Badge>
            ) : (
              <Badge tone="ok">available</Badge>
            )}
            {repo.ownership ? <Badge>{managed ? "managed clone" : "external"}</Badge> : null}
          </p>
          {repo.checkoutAvailable === false ? (
            <p className="text-muted">
              The folder at {repo.localPath} no longer exists. Run discovery if it moved, or unlink it.
            </p>
          ) : null}

          {confirm === null ? (
            <div className="flex flex-wrap gap-2">
              {repo.localPath && repo.githubOwner ? (
                <Button size="sm" variant="ghost" disabled={busy} onClick={() => setConfirm("unlink")}>
                  Unlink checkout…
                </Button>
              ) : null}
              {managed && repo.localPath ? (
                <Button
                  size="sm"
                  variant="ghost"
                  className="text-danger"
                  disabled={busy}
                  onClick={() => setConfirm("delete")}
                >
                  Delete managed files…
                </Button>
              ) : null}
              <Button
                size="sm"
                variant="ghost"
                className="text-danger"
                disabled={busy}
                onClick={() => setConfirm("forget")}
              >
                Forget record…
              </Button>
            </div>
          ) : null}

          {confirm === "unlink" ? (
            <div role="alertdialog" aria-label="Unlink checkout" className="space-y-2">
              <p>
                The record keeps its GitHub identity and becomes metadata-only. The folder {repo.localPath} is not
                touched.
              </p>
              <div className="flex gap-2">
                <Button size="sm" variant="primary" disabled={busy} onClick={() => unlink.mutate()}>
                  Unlink
                </Button>
                <Button size="sm" onClick={() => setConfirm(null)}>
                  Keep
                </Button>
              </div>
            </div>
          ) : null}

          {confirm === "delete" ? (
            <div role="alertdialog" aria-label="Delete managed files" className="space-y-2">
              <p>
                This permanently deletes the folder ShiftOps cloned. It is refused if the working tree has uncommitted
                changes. Commits that were never pushed are lost. Type the exact path to confirm:
              </p>
              <p className="selectable font-mono">{repo.localPath}</p>
              <label htmlFor={pathId} className="sr-only">
                Path to delete
              </label>
              <input
                id={pathId}
                value={typedPath}
                onChange={(e) => setTypedPath(e.target.value)}
                className="h-8 w-full rounded-md border border-border bg-surface px-2 font-mono"
              />
              <div className="flex gap-2">
                <Button
                  size="sm"
                  variant="danger"
                  disabled={busy || typedPath !== repo.localPath}
                  onClick={() => remove.mutate()}
                >
                  Delete files
                </Button>
                <Button
                  size="sm"
                  onClick={() => {
                    setConfirm(null);
                    setTypedPath("");
                  }}
                >
                  Keep
                </Button>
              </div>
            </div>
          ) : null}

          {confirm === "forget" ? (
            <div role="alertdialog" aria-label="Forget repository" className="space-y-2">
              <p>This removes only the ShiftOps record. Files stay untouched.</p>
              <div className="flex gap-2">
                <Button size="sm" variant="danger" disabled={busy} onClick={() => forget.mutate()}>
                  Forget record
                </Button>
                <Button size="sm" onClick={() => setConfirm(null)}>
                  Keep
                </Button>
              </div>
            </div>
          ) : null}
        </div>
      </div>
      {error ? (
        <div className="px-4 pb-4">
          <ErrorDisplay title="Repository update failed" error={error} />
        </div>
      ) : null}
    </Card>
  );
}
