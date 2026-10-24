import { preferencesFormSchema, type PreferencesForm, type Theme } from "@shiftops/core";
import { zodResolver } from "@hookform/resolvers/zod";
import { Button, Card, CardHeader, ErrorState, Spinner } from "@shiftops/ui";
import { useEffect } from "react";
import { useForm } from "react-hook-form";
import { EnvironmentPanel } from "../features/environment/EnvironmentPanel";
import { native } from "../lib/native";
import { queryKeys, type Settings, useAppInfo, useSettings } from "../lib/queries";
import { DefinitionList, Page } from "./Page";
import { useQueryClient, useMutation } from "@tanstack/react-query";

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
    onSettled: () => client.invalidateQueries({ queryKey: queryKeys.settings }),
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
          {save.isError ? <span className="text-sm text-danger">{save.error.message}</span> : null}
          {save.isSuccess && !form.formState.isDirty ? <span className="text-sm text-muted">Saved.</span> : null}
        </div>
      </form>
    </Card>
  );
}

export function SettingsPage() {
  const settings = useSettings();
  const info = useAppInfo();

  return (
    <Page title="Settings" description="Appearance, local storage and workstation environment.">
      {settings.data ? (
        <PreferencesCard settings={settings.data} />
      ) : settings.isError ? (
        <ErrorState title="Settings could not be loaded" error={settings.error} />
      ) : (
        <Spinner />
      )}

      <Card>
        <CardHeader title="Storage" description="All application state lives on this machine." />
        {info.data ? (
          <DefinitionList
            rows={[
              { term: "Application data", value: info.data.dataDir },
              { term: "Database", value: info.data.databasePath },
              { term: "Schema version", value: info.data.schemaVersion },
              { term: "App version", value: info.data.version },
            ]}
          />
        ) : info.isError ? (
          <div className="p-4">
            <ErrorState title="Application info unavailable" error={info.error} />
          </div>
        ) : (
          <div className="p-4">
            <Spinner />
          </div>
        )}
      </Card>

      <section id="environment">
        <EnvironmentPanel />
      </section>
    </Page>
  );
}
