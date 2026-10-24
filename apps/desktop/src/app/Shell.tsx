import { brand, Button, cn, Kbd, Logo } from "@shiftops/ui";
import { History, PanelLeftClose, PanelLeftOpen, Search } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { NavLink, Outlet, useNavigate } from "react-router";
import { ActivityCentre } from "../features/activity/ActivityCentre";
import { useDatabaseHealth, useSetSetting, useSettings } from "../lib/queries";
import { CommandPalette } from "./CommandPalette";
import { modKey, NAV_ITEMS } from "./navigation";

export function Shell() {
  const settings = useSettings();
  const setSetting = useSetSetting();
  const navigate = useNavigate();
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [activityOpen, setActivityOpen] = useState(false);
  const health = useDatabaseHealth();
  const closeActivity = useCallback(() => setActivityOpen(false), []);
  const collapsed = settings.data?.sidebarCollapsed ?? false;

  const toggleSidebar = useCallback(
    () => setSetting.mutate({ key: "sidebarCollapsed", value: !collapsed }),
    [setSetting, collapsed],
  );

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (!(event.metaKey || event.ctrlKey) || event.altKey) return;
      const key = event.key.toLowerCase();
      if (key === "k") {
        event.preventDefault();
        setPaletteOpen((open) => !open);
      } else if (key === "j") {
        event.preventDefault();
        setActivityOpen((open) => !open);
      } else if (key === "b") {
        event.preventDefault();
        toggleSidebar();
      } else {
        const item = NAV_ITEMS.find((n) => n.shortcut === key);
        if (item) {
          event.preventDefault();
          void navigate(item.path);
        }
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [navigate, toggleSidebar]);

  return (
    <div className="flex h-full">
      <aside
        className={cn(
          "flex shrink-0 flex-col border-r border-border bg-surface transition-[width] duration-150",
          collapsed ? "w-14" : "w-56",
        )}
        aria-label="Primary"
      >
        <div
          className={cn("flex h-12 items-center gap-2 border-b border-border", collapsed ? "justify-center" : "px-4")}
        >
          <Logo />
          {collapsed ? null : <span className="text-sm font-semibold tracking-tight">{brand.name}</span>}
        </div>

        <nav className="flex-1 space-y-0.5 p-2">
          {NAV_ITEMS.map(({ path, label, icon: Icon, shortcut }) => (
            <NavLink
              key={path}
              to={path}
              end={path === "/"}
              title={collapsed ? `${label} (${modKey}${shortcut})` : undefined}
              aria-label={collapsed ? label : undefined}
              className={({ isActive }) =>
                cn(
                  "group flex h-8 items-center gap-2.5 rounded-md text-sm transition-colors",
                  collapsed ? "justify-center" : "px-2.5",
                  isActive ? "bg-accent-soft font-medium text-fg" : "text-muted hover:bg-surface-2 hover:text-fg",
                )
              }
            >
              <Icon className="size-4 shrink-0" aria-hidden />
              {collapsed ? null : (
                <>
                  <span className="flex-1 truncate">{label}</span>
                  <Kbd className="opacity-0 group-hover:opacity-100">
                    {modKey}
                    {shortcut}
                  </Kbd>
                </>
              )}
            </NavLink>
          ))}
        </nav>

        <div className="border-t border-border p-2">
          <Button
            variant="ghost"
            size={collapsed ? "icon" : "sm"}
            className={cn(collapsed ? "w-full" : "w-full justify-start")}
            onClick={toggleSidebar}
            aria-label={collapsed ? "Expand sidebar" : "Collapse sidebar"}
            title={`${collapsed ? "Expand" : "Collapse"} sidebar (${modKey}B)`}
          >
            {collapsed ? <PanelLeftOpen /> : <PanelLeftClose />}
            {collapsed ? null : "Collapse"}
          </Button>
        </div>
      </aside>

      <div className="flex min-w-0 flex-1 flex-col">
        <header className="flex h-12 shrink-0 items-center justify-end gap-2 border-b border-border px-4">
          <Button
            variant="ghost"
            size="sm"
            onClick={() => setActivityOpen(true)}
            aria-label="Open Activity Centre"
            title={`Activity Centre (${modKey}J)`}
          >
            <History /> Activity
            {health.data ? <Kbd className="tabular-nums">{health.data.activityEventCount}</Kbd> : null}
          </Button>
          <Button
            variant="secondary"
            size="sm"
            className="w-64 justify-between text-muted"
            onClick={() => setPaletteOpen(true)}
            aria-label="Open command palette"
          >
            <span className="flex items-center gap-2">
              <Search /> Search commands
            </span>
            <Kbd>{modKey}K</Kbd>
          </Button>
        </header>
        <main className="min-h-0 flex-1 overflow-y-auto">
          <Outlet />
        </main>
      </div>

      <CommandPalette
        open={paletteOpen}
        onOpenChange={setPaletteOpen}
        onToggleSidebar={toggleSidebar}
        onOpenActivity={() => setActivityOpen(true)}
      />
      <ActivityCentre open={activityOpen} onClose={closeActivity} />
    </div>
  );
}
