import type { DeviceFlowEvent, DeviceFlowStart, GithubAccount } from "@shiftops/core";
import { zodResolver } from "@hookform/resolvers/zod";
import { Badge, Button, Card, CardHeader, Spinner } from "@shiftops/ui";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Copy, ExternalLink as ExternalIcon, GitBranch, RefreshCw } from "lucide-react";
import { useEffect, useState } from "react";
import { useForm } from "react-hook-form";
import { z } from "zod";
import { ErrorDisplay, useCopy } from "../../components/ErrorDisplay";
import { ExternalLink } from "../../components/ExternalLink";
import { invalidateAfter } from "../../lib/invalidation";
import { native, onGithubAuthEvent } from "../../lib/native";
import { useGithubOrganizations, useGithubStatus } from "../../lib/queries";
import { DefinitionList } from "../../pages/Page";

function formatTime(iso: string | null | undefined) {
  if (!iso) return "never";
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? iso : d.toLocaleString();
}

function rateLimitText(account: GithubAccount) {
  const r = account.rateLimit;
  if (!r || typeof r.remaining !== "number") return "not reported yet";
  const reset = typeof r.reset === "number" ? new Date(r.reset * 1000).toLocaleTimeString() : "unknown";
  return `${r.remaining} of ${String(r.limit ?? "?")} requests left · resets ${reset}`;
}

/** Live countdown in whole seconds. */
function useCountdown(deadline: number | null) {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (deadline === null) return;
    const t = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(t);
  }, [deadline]);
  return deadline === null ? null : Math.max(0, Math.round((deadline - now) / 1000));
}

function DeviceFlowPanel({ start, onDone }: { start: DeviceFlowStart; onDone: () => void }) {
  const client = useQueryClient();
  const [deadline] = useState(() => Date.now() + start.expiresInSeconds * 1000);
  const remaining = useCountdown(deadline);
  const [event, setEvent] = useState<DeviceFlowEvent | null>(null);
  const [copied, copy] = useCopy();
  const open = useMutation({ mutationFn: () => native.openExternalUrl(start.verificationUri) });
  const cancel = useMutation({ mutationFn: () => native.githubCancelDeviceFlow(start.flowId) });

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let disposed = false;
    void onGithubAuthEvent((e) => {
      if (e.flowId !== start.flowId) return;
      setEvent(e);
      if (["succeeded", "expired", "denied", "cancelled", "failed"].includes(e.state)) {
        void invalidateAfter(client, "github");
      }
    }).then((fn) => {
      if (disposed) fn();
      else unlisten = fn;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [start.flowId, client]);

  const finished = event && ["succeeded", "expired", "denied", "cancelled", "failed"].includes(event.state);
  const minutes = remaining === null ? "" : `${Math.floor(remaining / 60)}:${String(remaining % 60).padStart(2, "0")}`;

  return (
    <div className="space-y-3 border-t border-border p-4" aria-live="polite">
      {!finished ? (
        <>
          <p className="text-sm">
            Enter this code on GitHub to authorize ShiftOps. The code expires in{" "}
            <span className="font-mono tabular-nums">{minutes}</span>.
          </p>
          <div className="flex flex-wrap items-center gap-3">
            <code className="selectable rounded-md border border-border bg-surface-2 px-3 py-2 font-mono text-xl tracking-[0.3em]">
              {start.userCode}
            </code>
            <Button size="sm" onClick={() => copy(start.userCode)} aria-label="Copy code">
              <Copy /> {copied ? "Copied" : "Copy Code"}
            </Button>
            <Button size="sm" variant="primary" onClick={() => open.mutate()}>
              <ExternalIcon /> Open GitHub
            </Button>
            <Button size="sm" variant="ghost" onClick={() => cancel.mutate()} disabled={cancel.isPending}>
              Cancel
            </Button>
          </div>
          <p className="flex items-center gap-2 text-xs text-muted">
            <Spinner className="size-3" />
            {event?.state === "slow_down"
              ? `GitHub asked ShiftOps to poll more slowly (every ${event.intervalSeconds} s).`
              : `Waiting for approval · checking every ${event?.intervalSeconds ?? start.intervalSeconds} s`}
          </p>
          {open.isError ? <ErrorDisplay title="GitHub could not be opened" error={open.error} /> : null}
        </>
      ) : event.state === "succeeded" ? (
        <p role="status" className="text-sm">
          Connected as <span className="font-medium">{event.account?.login}</span>.
        </p>
      ) : event.state === "cancelled" ? (
        <p className="text-sm text-muted">Sign-in cancelled.</p>
      ) : (
        <ErrorDisplay title="GitHub sign-in did not complete" error={event.error} />
      )}
      {finished ? (
        <Button size="sm" onClick={onDone}>
          Close
        </Button>
      ) : null}
    </div>
  );
}

const patSchema = z.object({
  token: z
    .string()
    .min(1, "Paste the token")
    .max(8192, "The token is too long")
    .refine((v) => v === v.trim() && !/\s/.test(v), "Paste only the token, without spaces or line breaks"),
});

function PatForm({ replacing, onDone }: { replacing: boolean; onDone: () => void }) {
  const client = useQueryClient();
  const form = useForm<z.infer<typeof patSchema>>({ resolver: zodResolver(patSchema), defaultValues: { token: "" } });
  const connect = useMutation({
    mutationFn: (token: string) => native.githubConnectPat(token),
    onSettled: () => {
      form.reset({ token: "" });
      void invalidateAfter(client, "github");
    },
    onSuccess: onDone,
  });
  return (
    <form
      className="space-y-2 border-t border-border p-4 text-sm"
      autoComplete="off"
      onSubmit={form.handleSubmit(({ token }) => connect.mutate(token))}
    >
      <p className="font-medium">{replacing ? "Replace token" : "Use a fine-grained personal access token"}</p>
      <p className="text-xs text-muted">
        Minimum permissions: repository access to the repositories you want to migrate, with <b>Metadata</b>,{" "}
        <b>Contents</b>, <b>Pull requests</b> and <b>Checks</b> set to read-only. Add organisation <b>Members</b> (read)
        to list your organisations. ShiftOps validates the token, stores it in the OS keychain and never shows it again.{" "}
        <ExternalLink href="https://github.com/settings/personal-access-tokens/new">
          Create a token on GitHub
        </ExternalLink>
      </p>
      <label htmlFor="github-pat" className="sr-only">
        Personal access token
      </label>
      <div className="flex gap-2">
        <input
          id="github-pat"
          type="password"
          spellCheck={false}
          autoComplete="off"
          placeholder="github_pat_…"
          className="h-8 min-w-0 flex-1 rounded-md border border-border bg-surface px-2 font-mono"
          aria-invalid={form.formState.errors.token ? true : undefined}
          {...form.register("token")}
        />
        <Button type="submit" variant="primary" disabled={connect.isPending}>
          {connect.isPending ? "Validating…" : replacing ? "Validate and replace" : "Validate and connect"}
        </Button>
        {replacing ? <Button onClick={onDone}>Cancel</Button> : null}
      </div>
      {form.formState.errors.token ? (
        <p className="text-xs text-danger">{form.formState.errors.token.message}</p>
      ) : null}
      {connect.isError ? <ErrorDisplay title="Token not accepted" error={connect.error} /> : null}
    </form>
  );
}

function Connected({ account }: { account: GithubAccount }) {
  const client = useQueryClient();
  const orgs = useGithubOrganizations(true);
  const [confirming, setConfirming] = useState(false);
  const [replacing, setReplacing] = useState(false);
  const refresh = useMutation({ mutationFn: native.githubRefresh, onSettled: () => invalidateAfter(client, "github") });
  const disconnect = useMutation({
    mutationFn: native.githubDisconnect,
    onSettled: () => invalidateAfter(client, "github"),
  });
  return (
    <>
      {account.status === "invalid" ? (
        <div className="p-4">
          <ErrorDisplay
            title="GitHub rejected the stored credential"
            error={{
              code: "GITHUB_AUTHENTICATION_FAILED",
              message: "The token was revoked, expired or lost its access.",
              technicalDetails: null,
              suggestedAction: "Reconnect GitHub or replace the token.",
              recoverable: true,
            }}
          />
        </div>
      ) : null}
      <DefinitionList
        rows={[
          { term: "Account", value: `${account.login}${account.name ? ` (${account.name})` : ""} on ${account.host}` },
          {
            term: "Signed in with",
            value: account.authMethod === "oauth_device" ? "GitHub device sign-in (OAuth)" : "Personal access token",
          },
          {
            term: "Scopes",
            value: account.scopesReported
              ? account.scopes.join(", ") || "none"
              : "Not reported (fine-grained token permissions are set on GitHub)",
          },
          { term: "Validated", value: formatTime(account.validatedAt) },
          { term: "Rate limit", value: rateLimitText(account) },
          {
            term: "Organisations",
            value: orgs.isPending
              ? "loading…"
              : orgs.isError
                ? "could not be read"
                : orgs.data.length === 0
                  ? "none visible to this token"
                  : orgs.data.map((o) => o.login).join(", "),
          },
        ]}
      />
      <div className="flex flex-wrap gap-2 border-t border-border p-4">
        <Button size="sm" onClick={() => refresh.mutate()} disabled={refresh.isPending}>
          <RefreshCw /> {refresh.isPending ? "Refreshing…" : "Refresh"}
        </Button>
        {account.authMethod === "personal_access_token" ? (
          <Button size="sm" onClick={() => setReplacing(true)}>
            Replace token
          </Button>
        ) : null}
        {confirming ? (
          <span role="alertdialog" aria-label="Disconnect GitHub" className="flex items-center gap-2 text-xs">
            Remove the token from the keychain? Cached repository data stays.
            <Button
              size="sm"
              variant="danger"
              disabled={disconnect.isPending}
              onClick={() => disconnect.mutate(undefined, { onSuccess: () => setConfirming(false) })}
            >
              Disconnect
            </Button>
            <Button size="sm" onClick={() => setConfirming(false)}>
              Keep
            </Button>
          </span>
        ) : (
          <Button size="sm" variant="ghost" className="text-danger" onClick={() => setConfirming(true)}>
            Disconnect…
          </Button>
        )}
      </div>
      {refresh.isError ? (
        <div className="px-4 pb-4">
          <ErrorDisplay title="Refresh failed" error={refresh.error} />
        </div>
      ) : null}
      {disconnect.isError ? (
        <div className="px-4 pb-4">
          <ErrorDisplay title="Disconnect failed" error={disconnect.error} />
        </div>
      ) : null}
      {replacing ? <PatForm replacing onDone={() => setReplacing(false)} /> : null}
    </>
  );
}

export function GithubConnection() {
  const status = useGithubStatus();
  const [flow, setFlow] = useState<DeviceFlowStart | null>(null);
  const start = useMutation({ mutationFn: native.githubStartDeviceFlow, onSuccess: setFlow });
  const account = status.data?.account ?? null;
  const connected = account && account.status !== "disconnected";

  return (
    <Card>
      <CardHeader
        title={
          <span className="flex items-center gap-2">
            <GitBranch className="size-4" aria-hidden /> GitHub
          </span>
        }
        description={
          status.data
            ? `${status.data.host} · API ${status.data.apiBaseUrl}`
            : "Repository browsing, pull requests and checks."
        }
        actions={
          status.isPending ? (
            <Spinner className="size-3" />
          ) : connected ? (
            <Badge tone={account.status === "invalid" ? "danger" : "ok"}>
              {account.status === "invalid" ? "Credential rejected" : `Connected as ${account.login}`}
            </Badge>
          ) : (
            <Badge>Not configured</Badge>
          )
        }
      />
      {status.isError ? (
        <div className="p-4">
          <ErrorDisplay title="GitHub configuration problem" error={status.error} />
        </div>
      ) : status.data && connected ? (
        <Connected account={account} />
      ) : status.data ? (
        <>
          <div className="space-y-2 p-4 text-sm">
            <div className="flex flex-wrap items-center gap-3">
              <Button
                variant="primary"
                onClick={() => start.mutate()}
                disabled={!status.data.oauthConfigured || start.isPending || flow !== null}
              >
                {start.isPending ? "Requesting code…" : "Sign in with GitHub"}
              </Button>
              {!status.data.oauthConfigured ? (
                <span className="text-xs text-muted">
                  Device sign-in needs <code>VITE_GITHUB_OAUTH_CLIENT_ID</code> in <code>apps/desktop/.env</code>. Use a
                  token below instead.
                </span>
              ) : (
                <span className="text-xs text-muted">
                  Uses GitHub&apos;s device code flow; no password or secret is entered here.
                </span>
              )}
            </div>
            {start.isError ? <ErrorDisplay title="Device sign-in could not start" error={start.error} /> : null}
          </div>
          {flow ? (
            <DeviceFlowPanel start={flow} onDone={() => setFlow(null)} />
          ) : (
            <PatForm replacing={false} onDone={() => undefined} />
          )}
        </>
      ) : null}
    </Card>
  );
}
