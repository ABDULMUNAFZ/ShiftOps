import type { ActivityEvent, ActivityQuery, Severity } from "@shiftops/core";
import { Badge, Button, EmptyState, Spinner, type BadgeTone } from "@shiftops/ui";
import { History, RefreshCw, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { CopyButton, ErrorDisplay } from "../../components/ErrorDisplay";
import { useActivity, useActivityEntityTypes } from "../../lib/queries";

export const ACTIVITY_PAGE_SIZE = 25;

const SEVERITY_TONE: Record<Severity, BadgeTone> = {
  info: "neutral",
  notice: "accent",
  warning: "warn",
  error: "danger",
};

function formatTime(iso: string) {
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? iso : d.toLocaleString();
}

function EventRow({ event }: { event: ActivityEvent }) {
  const details = JSON.stringify(event, null, 2);
  return (
    <li className="px-4 py-2.5">
      <details>
        <summary className="flex cursor-pointer list-none items-start gap-3 [&::-webkit-details-marker]:hidden">
          <Badge tone={SEVERITY_TONE[event.severity]} className="mt-0.5 w-16 justify-center">
            {event.severity}
          </Badge>
          <div className="min-w-0 flex-1">
            <p className="selectable text-sm">{event.summary}</p>
            <p className="mt-0.5 flex flex-wrap gap-x-3 font-mono text-[11px] text-muted">
              <time dateTime={event.createdAt}>{formatTime(event.createdAt)}</time>
              <span>{event.eventType}</span>
              {event.entityId ? (
                <span className="truncate" title={event.entityId}>
                  {event.entityType}:{event.entityId}
                </span>
              ) : (
                <span>{event.entityType}</span>
              )}
            </p>
          </div>
        </summary>
        <div className="mt-2 pl-[4.75rem]">
          <pre className="selectable max-h-64 overflow-auto rounded bg-surface-2 p-2 font-mono text-xs">
            {JSON.stringify(event.metadata, null, 2)}
          </pre>
          <CopyButton text={details} label="Copy diagnostic details" />
        </div>
      </details>
    </li>
  );
}

/** Persisted activity: filters, pagination, refresh and copyable details. */
export function ActivityList({ pageSize = ACTIVITY_PAGE_SIZE }: { pageSize?: number }) {
  const [severity, setSeverity] = useState<Severity | "">("");
  const [entityType, setEntityType] = useState("");
  const [search, setSearch] = useState("");
  const [page, setPage] = useState(0);

  const query: ActivityQuery = {
    limit: pageSize,
    offset: page * pageSize,
    ...(severity ? { statuses: [severity] } : {}),
    ...(entityType ? { entityType } : {}),
    ...(search.trim() ? { search: search.trim() } : {}),
  };
  const activity = useActivity(query);
  const entityTypes = useActivityEntityTypes();
  const total = activity.data?.total ?? 0;
  const first = total === 0 ? 0 : page * pageSize + 1;
  const last = Math.min(total, (page + 1) * pageSize);
  const resetPage =
    <T,>(set: (v: T) => void) =>
    (v: T) => {
      set(v);
      setPage(0);
    };

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex flex-wrap items-center gap-2 border-b border-border px-4 py-2">
        <label className="sr-only" htmlFor="activity-search">
          Search activity
        </label>
        <input
          id="activity-search"
          value={search}
          onChange={(e) => resetPage(setSearch)(e.target.value)}
          placeholder="Search summaries"
          className="h-7 min-w-0 flex-1 rounded-md border border-border bg-surface px-2 text-sm"
        />
        <label className="sr-only" htmlFor="activity-severity">
          Severity
        </label>
        <select
          id="activity-severity"
          value={severity}
          onChange={(e) => resetPage(setSeverity)(e.target.value as Severity | "")}
          className="h-7 rounded-md border border-border bg-surface px-1 text-sm"
        >
          <option value="">All severities</option>
          <option value="info">Info</option>
          <option value="notice">Notice</option>
          <option value="warning">Warning</option>
          <option value="error">Error</option>
        </select>
        <label className="sr-only" htmlFor="activity-entity">
          Entity type
        </label>
        <select
          id="activity-entity"
          value={entityType}
          onChange={(e) => resetPage(setEntityType)(e.target.value)}
          className="h-7 rounded-md border border-border bg-surface px-1 text-sm"
        >
          <option value="">All entities</option>
          {(entityTypes.data ?? []).map((t) => (
            <option key={t} value={t}>
              {t}
            </option>
          ))}
        </select>
        <Button
          size="sm"
          onClick={() => void activity.refetch()}
          disabled={activity.isFetching}
          aria-label="Refresh activity"
        >
          {activity.isFetching ? <Spinner className="size-3" /> : <RefreshCw />}
          Refresh
        </Button>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto">
        {activity.isPending ? (
          <div className="flex items-center gap-2 p-4 text-muted">
            <Spinner /> Loading activity…
          </div>
        ) : activity.isError ? (
          <div className="p-4">
            <ErrorDisplay title="Activity could not be loaded" error={activity.error} />
          </div>
        ) : activity.data.items.length === 0 ? (
          <EmptyState
            icon={<History />}
            title={total === 0 && !search && !severity && !entityType ? "No activity yet" : "No matching activity"}
          >
            Events appear here when ShiftOps opens its database, runs checks, changes credentials or runs commands.
          </EmptyState>
        ) : (
          <ul className="divide-y divide-border" aria-label="Activity events">
            {activity.data.items.map((event) => (
              <EventRow key={event.id} event={event} />
            ))}
          </ul>
        )}
      </div>

      <div className="flex items-center justify-between border-t border-border px-4 py-2 text-xs text-muted">
        <span aria-live="polite">
          {first}–{last} of {total}
        </span>
        <div className="flex gap-2">
          <Button size="sm" onClick={() => setPage((p) => Math.max(0, p - 1))} disabled={page === 0}>
            Previous
          </Button>
          <Button size="sm" onClick={() => setPage((p) => p + 1)} disabled={last >= total}>
            Next
          </Button>
        </div>
      </div>
    </div>
  );
}

/** Slide-over panel opened from the shell header or ⌘J. */
export function ActivityCentre({ open, onClose }: { open: boolean; onClose: () => void }) {
  const closeRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (!open) return;
    closeRef.current?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, onClose]);

  if (!open) return null;
  return (
    <>
      <div className="fixed inset-0 z-30 bg-black/30" onClick={onClose} aria-hidden />
      <aside
        role="dialog"
        aria-modal="true"
        aria-label="Activity Centre"
        className="fixed inset-y-0 right-0 z-40 flex w-[520px] max-w-full flex-col border-l border-border bg-surface shadow-2xl"
      >
        <header className="flex h-12 items-center justify-between border-b border-border px-4">
          <h2 className="text-sm font-semibold">Activity Centre</h2>
          <Button ref={closeRef} variant="ghost" size="icon" onClick={onClose} aria-label="Close Activity Centre">
            <X />
          </Button>
        </header>
        <ActivityList />
      </aside>
    </>
  );
}
