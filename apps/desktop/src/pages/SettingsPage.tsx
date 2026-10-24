import {
  executionFormSchema,
  preferencesFormSchema,
  type ExecutionForm,
  type PreferencesForm,
  type Theme,
} from "@shiftops/core";
import { zodResolver } from "@hookform/resolvers/zod";
import { Button, Card, CardHeader, Spinner } from "@shiftops/ui";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";
import { useForm } from "react-hook-form";
import { ErrorDisplay } from "../components/ErrorDisplay";
import { CredentialSecurity } from "../features/credentials/CredentialSecurity";
import { NativeDiagnostics } from "../features/diagnostics/NativeDiagnostics";
import { EnvironmentPanel } from "../features/environment/EnvironmentPanel";
import { DatabaseHealthCard } from "../features/health/DatabaseHealthCard";
import { invalidateAfter } from "../lib/invalidation";
import { native } from "../lib/native";
import { useAppInfo, useSettings, type Settings } from "../lib/queries";
import { DefinitionList, Page } from "./Page";

const THEME_OPTIONS: { value: Theme; label: string }[] = [
  { value: "system", label: "System" },
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
];

function PreferencesCard({ settings }: { settings: Settings }) {
  const client = useQueryClient();
  const form = useForm<PreferencesForm>({
    resolver: zodResolver(preferencesFormSchema),
    defaultValues: { theme: settings.theme, sidebarCollapsed: settings.sidebarCollapsed },
  });

  // Theme and sidebar can also change from the command palette and shortcuts.
  useEffect(() => {
    form.reset({ theme: settings.theme, sidebarCollapsed: settings.sidebarCollapsed });
  }, [form, settings.theme, settings.sidebarCollapsed]);

  const save = useMutation({
    mutationFn: async (values: PreferencesForm) => {
      await native.setSetting("theme", values.theme);
      await native.setSetting("sidebarCollapsed", values.sidebarCollapsed);
    },
    onSettled: () => invalidateAfter(client, "settings"),
  });

  return (
    <Card>
      <CardHeader title="Preferences" description="Stored in the local SQLite database." />
      <form className="space-y-4 p-4" onSubmit={form.handleSubmit((values) => save.mutate(values))}>
        <fieldset>
          <legend className="mb-2 text-sm font-medium">Theme</legend>
          <div className="flex gap-2">
            {THEME_OPTIONS.map((opt) => (
              <label
                key={opt.value}
                className="flex h-8 cursor-pointer items-center gap-2 rounded-md border border-border px-3 has-[:checked]:border-accent has-[:checked]:bg-accent-soft has-[:focus-visible]:outline-2 has-[:focus-visible]:outline-accent"
              >
                <input type="radio" value={opt.value} className="sr-only" {...form.register("theme")} />
                {opt.label}
              </label>
            ))}
          </div>
        </fieldset>
        <label className="flex items-center gap-2 text-sm">
          <input type="checkbox" className="size-4 accent-[var(--so-accent)]" {...form.register("sidebarCollapsed")} />
          Collapse sidebar
        </label>
        <div className="flex items-center gap-3">
          <Button type="submit" variant="primary" disabled={!form.formState.isDirty || save.isPending}>
            {save.isPending ? "Saving…" : "Save preferences"}
          </Button>
          {save.isSuccess && !form.formState.isDirty ? <span className="text-sm text-muted">Saved.</span> : null}
        </div>
        {save.isError ? <ErrorDisplay title="Preferences not saved" error={save.error} /> : null}
      </form>
    </Card>
  );
}

function ExecutionCard({ settings }: { settings: Settings }) {
  const client = useQueryClient();
  const current = {
    timeoutSeconds: settings["execution.defaultTimeoutSeconds"],
    maxOutputKib: Math.round(settings["execution.maxOutputBytes"] / 1024),
  };
  const form = useForm<ExecutionForm>({ resolver: zodResolver(executionFormSchema), defaultValues: current });

  useEffect(() => {
    form.reset({ timeoutSeconds: current.timeoutSeconds, maxOutputKib: current.maxOutputKib });
  }, [form, current.timeoutSeconds, current.maxOutputKib]);

  const save = useMutation({
    mutationFn: async (values: ExecutionForm) => {
      await native.setSetting("execution.defaultTimeoutSeconds", values.timeoutSeconds);
      await native.setSetting("execution.maxOutputBytes", values.maxOutputKib * 1024);
    },
    onSettled: () => invalidateAfter(client, "settings"),
  });
  const errors = form.formState.errors;

  return (
    <Card>
      <CardHeader
        title="Command execution"
        description="Applied to every command ShiftOps runs. Commands are stopped at the timeout; output beyond the limit is discarded with a marker."
      />
      <form className="grid gap-4 p-4 sm:grid-cols-2" onSubmit={form.handleSubmit((v) => save.mutate(v))} noValidate>
        <label className="space-y-1 text-sm">
          <span className="font-medium">Default timeout (seconds)</span>
          <input
            type="number"
            min={1}
            max={3600}
            className="h-8 w-full rounded-md border border-border bg-surface px-2"
            aria-invalid={errors.timeoutSeconds ? true : undefined}
            {...form.register("timeoutSeconds", { valueAsNumber: true })}
          />
          {errors.timeoutSeconds ? <span className="text-xs text-danger">{errors.timeoutSeconds.message}</span> : null}
        </label>
        <label className="space-y-1 text-sm">
          <span className="font-medium">Output limit (KiB)</span>
          <input
            type="number"
            min={4}
            max={16384}
            className="h-8 w-full rounded-md border border-border bg-surface px-2"
            aria-invalid={errors.maxOutputKib ? true : undefined}
            {...form.register("maxOutputKib", { valueAsNumber: true })}
          />
          {errors.maxOutputKib ? <span className="text-xs text-danger">{errors.maxOutputKib.message}</span> : null}
        </label>
        <div className="flex items-center gap-3 sm:col-span-2">
          <Button type="submit" variant="primary" disabled={!form.formState.isDirty || save.isPending}>
            {save.isPending ? "Saving…" : "Save limits"}
          </Button>
        </div>
        {save.isError ? (
          <div className="sm:col-span-2">
            <ErrorDisplay title="Limits not saved" error={save.error} />
          </div>
        ) : null}
      </form>
    </Card>
  );
}

function StorageCard() {
  const info = useAppInfo();
  return (
    <Card>
      <CardHeader title="Storage" description="All application state lives on this machine." />
      {info.data ? (
        <DefinitionList
          rows={[
            { term: "Application data", value: info.data.dataDir },
            { term: "Database", value: info.data.databasePath },
            { term: "Log directory", value: info.data.logDir },
            { term: "Schema version", value: `${info.data.schemaVersion} (latest ${info.data.latestSchemaVersion})` },
            { term: "App version", value: info.data.version },
          ]}
        />
      ) : info.isError ? (
        <div className="p-4">
          <ErrorDisplay title="Application info unavailable" error={info.error} />
        </div>
      ) : (
        <div className="p-4">
          <Spinner />
        </div>
      )}
    </Card>
  );
}

export function SettingsPage() {
  const settings = useSettings();

  return (
    <Page title="Settings" description="Appearance, execution limits, storage, credentials and native diagnostics.">
      {settings.data ? (
        <div className="grid gap-4 xl:grid-cols-2">
          <PreferencesCard settings={settings.data} />
          <ExecutionCard settings={settings.data} />
        </div>
      ) : settings.isError ? (
        <ErrorDisplay title="Settings could not be loaded" error={settings.error} />
      ) : (
        <Spinner />
      )}

      <div className="grid gap-4 xl:grid-cols-2">
        <StorageCard />
        <DatabaseHealthCard detailed />
      </div>

      <CredentialSecurity />
      <NativeDiagnostics />

      <section id="environment">
        <EnvironmentPanel />
      </section>
    </Page>
  );
}
