import { Bot, FolderGit2, LayoutDashboard, type LucideIcon, Plug, Settings, ShieldCheck, Workflow } from "lucide-react";

export interface NavItem {
  path: string;
  label: string;
  icon: LucideIcon;
  /** Digit used with Cmd/Ctrl for direct navigation. */
  shortcut: string;
}

export const NAV_ITEMS: readonly NavItem[] = [
  { path: "/", label: "Overview", icon: LayoutDashboard, shortcut: "1" },
  { path: "/repositories", label: "Repositories", icon: FolderGit2, shortcut: "2" },
  { path: "/campaigns", label: "Campaigns", icon: Workflow, shortcut: "3" },
  { path: "/agent-runs", label: "Agent Runs", icon: Bot, shortcut: "4" },
  { path: "/verification", label: "Verification", icon: ShieldCheck, shortcut: "5" },
  { path: "/integrations", label: "Integrations", icon: Plug, shortcut: "6" },
  { path: "/settings", label: "Settings", icon: Settings, shortcut: "7" },
];

export const isMac = typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform);
export const modKey = isMac ? "⌘" : "Ctrl";
