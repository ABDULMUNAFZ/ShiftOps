import type { Theme } from "@shiftops/core";
import { useQueryClient } from "@tanstack/react-query";
import { Command } from "cmdk";
import { History, Info, Monitor, Moon, PanelLeft, RefreshCw, Sun } from "lucide-react";
import type { ReactNode } from "react";
import { useNavigate } from "react-router";
import { queryKeys, useSetSetting } from "../lib/queries";
import { NAV_ITEMS } from "./navigation";

interface Props {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onToggleSidebar: () => void;
  onOpenActivity: () => void;
}

const THEMES: { value: Theme; label: string; icon: ReactNode }[] = [
  { value: "light", label: "Light", icon: <Sun /> },
  { value: "dark", label: "Dark", icon: <Moon /> },
  { value: "system", label: "System", icon: <Monitor /> },
];

const itemClass =
  "flex h-9 cursor-default items-center gap-2.5 rounded-md px-2.5 text-sm text-fg data-[selected=true]:bg-accent-soft [&_svg]:size-4 [&_svg]:text-muted";
const groupClass =
  "[&_[cmdk-group-heading]]:px-2.5 [&_[cmdk-group-heading]]:pb-1 [&_[cmdk-group-heading]]:pt-2 [&_[cmdk-group-heading]]:text-[11px] [&_[cmdk-group-heading]]:font-medium [&_[cmdk-group-heading]]:uppercase [&_[cmdk-group-heading]]:tracking-wide [&_[cmdk-group-heading]]:text-muted";

export function CommandPalette({ open, onOpenChange, onToggleSidebar, onOpenActivity }: Props) {
  const navigate = useNavigate();
  const client = useQueryClient();
  const setSetting = useSetSetting();

  const run = (action: () => void) => () => {
    onOpenChange(false);
    action();
  };

  return (
    <Command.Dialog
      open={open}
      onOpenChange={onOpenChange}
      label="Command palette"
      overlayClassName="fixed inset-0 z-40 bg-black/40"
      contentClassName="fixed left-1/2 top-[18%] z-50 w-[560px] max-w-[90vw] -translate-x-1/2 overflow-hidden rounded-lg border border-border bg-surface shadow-2xl"
    >
      <Command.Input
        placeholder="Type a command or page…"
        className="h-11 w-full border-b border-border bg-transparent px-4 text-sm outline-none placeholder:text-muted"
      />
      <Command.List className="max-h-80 overflow-y-auto p-2">
        <Command.Empty className="px-3 py-6 text-center text-sm text-muted">No matching command.</Command.Empty>
        <Command.Group heading="Go to" className={groupClass}>
          {NAV_ITEMS.map(({ path, label, icon: Icon }) => (
            <Command.Item
              key={path}
              value={`go ${label}`}
              onSelect={run(() => void navigate(path))}
              className={itemClass}
            >
              <Icon /> {label}
            </Command.Item>
          ))}
        </Command.Group>
        <Command.Group heading="Appearance" className={groupClass}>
          {THEMES.map((t) => (
            <Command.Item
              key={t.value}
              value={`theme ${t.label}`}
              onSelect={run(() => setSetting.mutate({ key: "theme", value: t.value }))}
              className={itemClass}
            >
              {t.icon} Theme: {t.label}
            </Command.Item>
          ))}
          <Command.Item value="toggle sidebar" onSelect={run(onToggleSidebar)} className={itemClass}>
            <PanelLeft /> Toggle sidebar
          </Command.Item>
        </Command.Group>
        <Command.Group heading="Help" className={groupClass}>
          <Command.Item value="about shiftops" onSelect={run(() => void navigate("/about"))} className={itemClass}>
            <Info /> About ShiftOps
          </Command.Item>
        </Command.Group>
        <Command.Group heading="Activity" className={groupClass}>
          <Command.Item value="open activity centre" onSelect={run(onOpenActivity)} className={itemClass}>
            <History /> Open Activity Centre
          </Command.Item>
        </Command.Group>
        <Command.Group heading="Environment" className={groupClass}>
          <Command.Item
            value="re-run environment checks"
            onSelect={run(() => void client.refetchQueries({ queryKey: queryKeys.environment }))}
            className={itemClass}
          >
            <RefreshCw /> Re-run environment checks
          </Command.Item>
        </Command.Group>
      </Command.List>
    </Command.Dialog>
  );
}
