import type { CredentialKind, CredentialMetadata, CredentialProvider } from "@shiftops/core";
import { zodResolver } from "@hookform/resolvers/zod";
import { Badge, Button, Card, CardHeader, Spinner } from "@shiftops/ui";
import { KeyRound } from "lucide-react";
import { useState } from "react";
import { useForm } from "react-hook-form";
import { z } from "zod";
import { ErrorDisplay } from "../../components/ErrorDisplay";
import { ExternalLink } from "../../components/ExternalLink";
import { useCredentials, useDeleteCredential, useKeychainStatus, useStoreCredential } from "../../lib/queries";

const LABELS: Record<CredentialKind, string> = {
  github_personal_access_token: "GitHub personal access token",
  github_oauth_token: "GitHub OAuth token",
  devin_api_token: "Devin API token",
  provider_token: "Provider token",
};

const HELP: Partial<Record<CredentialKind, { href: string; text: string }>> = {
  github_personal_access_token: {
    href: "https://github.com/settings/personal-access-tokens",
    text: "Create a fine-grained token on GitHub",
  },
};

const secretSchema = z.object({
  secret: z
    .string()
    .min(1, "Enter the token")
    .max(8192, "The token is too long")
    .refine((v) => v === v.trim() && !/\s/.test(v), "Paste only the token, without spaces or line breaks"),
});

function formatTime(iso: string | null) {
  if (!iso) return "unknown";
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? iso : d.toLocaleString();
}

function SecretForm({
  provider,
  kind,
  replacing,
  onDone,
}: {
  provider: CredentialProvider;
  kind: CredentialKind;
  replacing: boolean;
  onDone: () => void;
}) {
  const store = useStoreCredential();
  const form = useForm<z.infer<typeof secretSchema>>({
    resolver: zodResolver(secretSchema),
    defaultValues: { secret: "" },
  });
  const id = `secret-${kind}`;

  const submit = form.handleSubmit(({ secret }) => {
    store.mutate(
      { provider, kind, secret },
      {
        // Clear the field whatever the outcome; the value is never shown again.
        onSettled: () => form.reset({ secret: "" }),
        onSuccess: onDone,
      },
    );
  });

  return (
    <form onSubmit={submit} className="mt-3 space-y-2" autoComplete="off">
      <label htmlFor={id} className="block text-xs font-medium">
        {replacing ? `New ${LABELS[kind]}` : LABELS[kind]}
      </label>
      <div className="flex gap-2">
        <input
          id={id}
          type="password"
          autoComplete="off"
          spellCheck={false}
          className="h-8 min-w-0 flex-1 rounded-md border border-border bg-surface px-2 font-mono text-sm"
          aria-invalid={form.formState.errors.secret ? true : undefined}
          aria-describedby={`${id}-help`}
          {...form.register("secret")}
        />
        <Button type="submit" variant="primary" disabled={store.isPending}>
          {store.isPending ? "Saving…" : replacing ? "Replace" : "Save to keychain"}
        </Button>
        {replacing ? (
          <Button onClick={onDone} disabled={store.isPending}>
            Cancel
          </Button>
        ) : null}
      </div>
      <p id={`${id}-help`} className="text-xs text-muted">
        {form.formState.errors.secret?.message ??
          "Stored in the OS keychain. ShiftOps never displays a saved token again."}
      </p>
      {store.isError ? <ErrorDisplay title="Credential not saved" error={store.error} /> : null}
    </form>
  );
}

function CredentialRow({ credential }: { credential: CredentialMetadata }) {
  const [replacing, setReplacing] = useState(false);
  const [confirming, setConfirming] = useState(false);
  const remove = useDeleteCredential();
  const help = HELP[credential.kind];

  return (
    <li className="px-4 py-3" data-credential={credential.identifier}>
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div>
          <p className="font-medium">{LABELS[credential.kind]}</p>
          <p className="font-mono text-[11px] text-muted">{credential.identifier}</p>
        </div>
        {credential.exists ? <Badge tone="ok">Stored</Badge> : <Badge>Not stored</Badge>}
      </div>

      {credential.exists ? (
        <dl className="mt-2 grid grid-cols-[8rem_1fr] gap-y-1 text-xs">
          <dt className="text-muted">First saved</dt>
          <dd>{formatTime(credential.createdAt)}</dd>
          <dt className="text-muted">Last updated</dt>
          <dd>{formatTime(credential.updatedAt)}</dd>
          <dt className="text-muted">Validation</dt>
          <dd>
            {credential.lastValidationStatus === "not_validated"
              ? "Not validated yet"
              : credential.lastValidationStatus}
          </dd>
        </dl>
      ) : null}

      {!credential.manageable ? (
        <p className="mt-2 text-xs text-muted">Created by the sign-in flow, which is not available in this build.</p>
      ) : credential.exists ? (
        replacing ? (
          <SecretForm
            provider={credential.provider}
            kind={credential.kind}
            replacing
            onDone={() => setReplacing(false)}
          />
        ) : confirming ? (
          <div
            role="alertdialog"
            aria-label={`Delete ${LABELS[credential.kind]}`}
            className="mt-3 rounded-md border border-danger/40 p-3"
          >
            <p className="text-sm">
              Delete the {LABELS[credential.kind]} from the keychain? Features that need it will stop working until you
              add a new one.
            </p>
            <div className="mt-2 flex gap-2">
              <Button
                variant="danger"
                size="sm"
                disabled={remove.isPending}
                onClick={() =>
                  remove.mutate(
                    { provider: credential.provider, kind: credential.kind },
                    { onSuccess: () => setConfirming(false) },
                  )
                }
              >
                {remove.isPending ? "Deleting…" : "Delete credential"}
              </Button>
              <Button size="sm" onClick={() => setConfirming(false)} disabled={remove.isPending}>
                Keep it
              </Button>
            </div>
            {remove.isError ? <ErrorDisplay title="Credential not deleted" error={remove.error} /> : null}
          </div>
        ) : (
          <div className="mt-3 flex gap-2">
            <Button size="sm" onClick={() => setReplacing(true)}>
              Replace
            </Button>
            <Button size="sm" variant="ghost" className="text-danger" onClick={() => setConfirming(true)}>
              Delete…
            </Button>
          </div>
        )
      ) : (
        <>
          <SecretForm
            provider={credential.provider}
            kind={credential.kind}
            replacing={false}
            onDone={() => undefined}
          />
          {help ? (
            <p className="mt-1 text-xs">
              <ExternalLink href={help.href}>{help.text}</ExternalLink>
            </p>
          ) : null}
        </>
      )}
    </li>
  );
}

export function KeychainStatusLine() {
  const keychain = useKeychainStatus();
  if (keychain.isPending) return <Spinner className="size-3" label="Checking keychain" />;
  if (keychain.isError) return <Badge tone="danger">Keychain check failed</Badge>;
  return keychain.data.available ? (
    <Badge tone="ok">{keychain.data.backend} available</Badge>
  ) : (
    <Badge tone="danger" title={keychain.data.error?.message}>
      {keychain.data.backend} unavailable
    </Badge>
  );
}

/** Credential management. Secrets go in; only metadata comes back. */
export function CredentialSecurity({ providers }: { providers?: CredentialProvider[] }) {
  const credentials = useCredentials();
  const keychain = useKeychainStatus();
  const shown = credentials.data?.filter((c) => !providers || providers.includes(c.provider)) ?? [];

  return (
    <Card>
      <CardHeader
        title={
          <span className="flex items-center gap-2">
            <KeyRound className="size-4" aria-hidden /> Credential security
          </span>
        }
        description="Tokens are written to the operating system keychain. ShiftOps keeps only non-secret metadata."
        actions={<KeychainStatusLine />}
      />
      {keychain.data && !keychain.data.available && keychain.data.error ? (
        <div className="p-4">
          <ErrorDisplay title="The keychain is not usable" error={keychain.data.error} />
        </div>
      ) : null}
      {credentials.isPending ? (
        <div className="flex items-center gap-2 p-4 text-muted">
          <Spinner /> Reading keychain…
        </div>
      ) : credentials.isError ? (
        <div className="p-4">
          <ErrorDisplay title="Credentials could not be read" error={credentials.error} />
        </div>
      ) : (
        <ul className="divide-y divide-border" aria-label="Credentials">
          {shown.map((c) => (
            <CredentialRow key={c.identifier} credential={c} />
          ))}
        </ul>
      )}
    </Card>
  );
}
