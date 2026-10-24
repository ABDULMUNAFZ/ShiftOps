import type { SubmissionReadiness } from "@shiftops/core";
import { Badge, Button, Card, CardHeader, EmptyState, Spinner, type BadgeTone } from "@shiftops/ui";
import { useQuery } from "@tanstack/react-query";
import { CheckCircle2, Clipboard, ExternalLink, FileText, RefreshCw, ShieldAlert } from "lucide-react";
import { useState } from "react";
import { ErrorDisplay } from "../components/ErrorDisplay";
import { native } from "../lib/native";
import { queryKeys } from "../lib/queryKeys";
import { DefinitionList, Page } from "./Page";

const STATUS: Record<SubmissionReadiness["status"], { label: string; tone: BadgeTone }> = {
  not_ready: { label: "Not Ready", tone: "danger" },
  needs_participant_input: { label: "Needs Participant Input", tone: "warn" },
  ready_for_final_verification: { label: "Ready for Final Verification", tone: "ok" },
  submitted: { label: "Submitted", tone: "accent" },
};

function githubUrl(remote: string | null) {
  if (!remote) return null;
  const https = remote.match(/^https:\/\/github\.com\/([^/]+\/[^/]+?)(?:\.git)?$/i);
  if (https) return `https://github.com/${https[1]}`;
  const ssh = remote.match(/^git@github\.com:([^/]+\/[^/]+?)(?:\.git)?$/i);
  return ssh ? `https://github.com/${ssh[1]}` : null;
}

function checklist(data: SubmissionReadiness) {
  const mark = (ok: boolean) => (ok ? "[x]" : "[ ]");
  return [
    `ShiftOps submission readiness — ${STATUS[data.status].label}`,
    ...Object.entries(data.requiredFiles).map(([name, present]) => `${mark(present)} ${name}`),
    `${mark(data.workingTreeClean)} Working tree clean`,
    `${mark(data.pushed)} HEAD pushed to origin/main`,
    `${mark(data.todoCount === 0)} Participant TODOs resolved (${data.todoCount} remaining)`,
    `${mark(data.secretFindings.length === 0)} Tracked-secret review (${data.secretFindings.length} findings)`,
    `${mark(Boolean(data.validation?.passed))} Full validation recorded`,
    `[ ] Final submission SHA recorded in Task 6`,
    `[ ] Submitted manually`,
  ].join("\n");
}

export function SubmissionReadinessPage() {
  // Readiness describes the checkout on disk, which changes outside the app:
  // refresh when the window regains focus as well as on demand.
  const readiness = useQuery({
    queryKey: queryKeys.readiness,
    queryFn: native.getSubmissionReadiness,
    retry: false,
    refetchOnWindowFocus: true,
  });
  const [copied, setCopied] = useState(false);
  const data = readiness.data;
  const remote = githubUrl(data?.remoteUrl ?? null);

  return (
    <Page
      title="Submission Readiness"
      description="Evidence from this checkout. ShiftOps never marks the project Submitted automatically."
      actions={
        <Button size="sm" onClick={() => void readiness.refetch()} disabled={readiness.isFetching}>
          <RefreshCw /> Refresh
        </Button>
      }
    >
      {readiness.isPending ? (
        <p className="flex items-center gap-2 text-muted">
          <Spinner /> Inspecting Git evidence…
        </p>
      ) : null}
      {readiness.isError ? (
        <ErrorDisplay
          title="Submission evidence could not be collected"
          error={readiness.error}
          action={
            <Button size="sm" onClick={() => void readiness.refetch()}>
              Retry
            </Button>
          }
        />
      ) : null}
      {data ? (
        <>
          <Card>
            <CardHeader
              title={
                <span className="flex items-center gap-2">
                  <CheckCircle2 className="size-4" /> Current status
                </span>
              }
              actions={<Badge tone={STATUS[data.status].tone}>{STATUS[data.status].label}</Badge>}
            />
            <div className="grid gap-4 p-4 lg:grid-cols-2">
              <DefinitionList
                rows={[
                  { term: "Source root", value: <code>{data.sourceRoot}</code> },
                  { term: "HEAD", value: data.headSha?.slice(0, 12) ?? "Unavailable" },
                  { term: "origin/main", value: data.originMainSha?.slice(0, 12) ?? "Unavailable" },
                  {
                    term: "Push state",
                    value: data.pushed ? "Up to date" : `${data.commitsAhead ?? "?"} commits ahead`,
                  },
                  { term: "Working tree", value: data.workingTreeClean ? "Clean" : "Has changes" },
                  { term: "Participant TODOs", value: String(data.todoCount) },
                ]}
              />
              <DefinitionList
                rows={[
                  { term: "Tracked files scanned", value: String(data.trackedFilesScanned) },
                  { term: "Potential secrets", value: String(data.secretFindings.length) },
                  {
                    term: "Validation",
                    value: data.validation
                      ? `${data.validation.passed ? "Passed" : "Failed"} at ${data.validation.commitSha.slice(0, 12)}`
                      : "Not recorded",
                  },
                  { term: "Final SHA", value: data.finalSha ?? "Unset until Task 6" },
                  { term: "Automatic submission", value: "Disabled" },
                ]}
              />
            </div>
          </Card>

          <div className="grid gap-4 lg:grid-cols-2">
            <Card>
              <CardHeader title="Required files" />
              <ul className="divide-y divide-border text-sm">
                {Object.entries(data.requiredFiles).map(([name, present]) => (
                  <li key={name} className="flex items-center justify-between px-4 py-2">
                    <span className="font-mono">{name}</span>
                    <Badge tone={present ? "ok" : "danger"}>{present ? "present" : "missing"}</Badge>
                  </li>
                ))}
              </ul>
            </Card>
            <Card>
              <CardHeader title="Actions" />
              <div className="flex flex-wrap gap-2 p-4">
                <Button
                  size="sm"
                  onClick={() => void navigator.clipboard.writeText(checklist(data)).then(() => setCopied(true))}
                >
                  <Clipboard /> {copied ? "Copied" : "Copy checklist"}
                </Button>
                <Button
                  size="sm"
                  onClick={() => void Promise.all(Object.keys(data.requiredFiles).map(native.openSubmissionFile))}
                >
                  <FileText /> Open files
                </Button>
                <Button size="sm" disabled={!remote} onClick={() => remote && void native.openExternalUrl(remote)}>
                  <ExternalLink /> Open GitHub
                </Button>
              </div>
              <p className="px-4 pb-4 text-xs text-muted">
                Submitted remains a human-controlled state and the final SHA is intentionally unset until Task 6.
              </p>
            </Card>
          </div>

          {data.reasons.length ? (
            <Card>
              <CardHeader
                title={
                  <span className="flex items-center gap-2">
                    <ShieldAlert className="size-4" /> What remains
                  </span>
                }
              />
              <ul className="list-disc space-y-1 p-4 pl-8 text-sm">
                {data.reasons.map((reason) => (
                  <li key={reason}>{reason}</li>
                ))}
              </ul>
            </Card>
          ) : null}

          {data.warnings.length ? (
            <Card>
              <CardHeader
                title="Notes"
                description="These do not block readiness but limit how complete the evidence is."
              />
              <ul className="list-disc space-y-1 p-4 pl-8 text-sm text-muted">
                {data.warnings.map((warning) => (
                  <li key={warning}>{warning}</li>
                ))}
              </ul>
            </Card>
          ) : null}

          {data.secretFindings.length ? (
            <Card>
              <CardHeader
                title="Potential tracked secrets"
                description="Only rule names and paths are shown; matched values never cross the native boundary."
              />
              <ul className="divide-y divide-border text-xs">
                {data.secretFindings.map((finding) => (
                  <li key={`${finding.path}:${finding.rule}`} className="flex justify-between px-4 py-2">
                    <code>{finding.path}</code>
                    <Badge tone="warn">{finding.rule}</Badge>
                  </li>
                ))}
              </ul>
            </Card>
          ) : (
            <EmptyState title="No potential tracked secrets detected">
              The bounded scan reports paths and rules only.
            </EmptyState>
          )}
        </>
      ) : null}
    </Page>
  );
}
