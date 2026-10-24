import { Logo } from "@shiftops/ui";

type Tone = "lime" | "amber" | "sky" | "muted" | "rose";

const TONE: Record<Tone, string> = {
  lime: "bg-lime-300/15 text-lime-300 ring-lime-300/30",
  amber: "bg-amber-300/15 text-amber-200 ring-amber-300/30",
  sky: "bg-sky-300/15 text-sky-200 ring-sky-300/30",
  muted: "bg-white/5 text-white/60 ring-white/10",
  rose: "bg-rose-400/15 text-rose-200 ring-rose-400/30",
};

function Pill({ tone, children }: { tone: Tone; children: string }) {
  return (
    <span className={`inline-flex items-center rounded-full px-2 py-0.5 text-[10px] font-medium ring-1 ${TONE[tone]}`}>
      {children}
    </span>
  );
}

const WAVES: { wave: string; repos: { name: string; status: string; tone: Tone }[] }[] = [
  {
    wave: "Wave 1",
    repos: [
      { name: "shared-config", status: "verified", tone: "lime" },
      { name: "auth-sdk", status: "verified", tone: "lime" },
    ],
  },
  {
    wave: "Wave 2",
    repos: [
      { name: "orders-api", status: "verifying", tone: "sky" },
      { name: "billing-api", status: "repair 1 of 2", tone: "amber" },
    ],
  },
  {
    wave: "Wave 3",
    repos: [
      { name: "web-storefront", status: "awaiting approval", tone: "muted" },
      { name: "legacy-admin", status: "needs review", tone: "rose" },
    ],
  },
];

const NAV = ["Overview", "Repositories", "Campaigns", "Agent Runs", "Readiness"];

/**
 * An illustrative mockup of the desktop app's campaign view. It shows how the
 * product is organised; the repositories and statuses are examples, not data.
 */
export function AppMockup() {
  return (
    <figure className="relative">
      <div className="overflow-hidden rounded-[28px] border border-white/10 bg-[#0f120f] shadow-[0_40px_120px_-40px_rgba(198,255,61,0.35)]">
        <div className="flex items-center gap-2 border-b border-white/10 px-4 py-3">
          <span className="size-2.5 rounded-full bg-white/15" />
          <span className="size-2.5 rounded-full bg-white/15" />
          <span className="size-2.5 rounded-full bg-white/15" />
          <span className="ml-3 font-mono text-[11px] text-white/40">ShiftOps · Campaigns · Node 22 rollout</span>
        </div>
        <div className="grid grid-cols-[150px_1fr] max-sm:grid-cols-1">
          <aside className="border-r border-white/10 p-3 max-sm:hidden">
            <div className="mb-4 flex items-center gap-2 px-2 text-sm font-semibold text-white">
              <Logo className="size-4" /> ShiftOps
            </div>
            <ul className="space-y-1 text-xs">
              {NAV.map((item) => (
                <li
                  key={item}
                  className={`rounded-lg px-2 py-1.5 ${item === "Campaigns" ? "bg-lime-300 font-medium text-black" : "text-white/55"}`}
                >
                  {item}
                </li>
              ))}
            </ul>
          </aside>
          <div className="space-y-4 p-4 sm:p-5">
            <div className="flex flex-wrap items-end justify-between gap-3">
              <div>
                <p className="font-mono text-[10px] uppercase tracking-[0.2em] text-lime-300/80">Campaign</p>
                <p className="text-lg font-semibold text-white">Upgrade Node.js 18 to Node.js 22</p>
              </div>
              <div className="flex gap-2">
                <Pill tone="lime">2 verified</Pill>
                <Pill tone="amber">1 repairing</Pill>
                <Pill tone="rose">1 review</Pill>
              </div>
            </div>
            <div className="grid gap-3 sm:grid-cols-3">
              {WAVES.map((w) => (
                <div key={w.wave} className="rounded-2xl border border-white/10 bg-white/[0.03] p-3">
                  <p className="mb-2 text-[11px] font-medium text-white/50">{w.wave}</p>
                  <ul className="space-y-2">
                    {w.repos.map((r) => (
                      <li key={r.name} className="rounded-xl bg-black/30 p-2.5">
                        <p className="font-mono text-xs text-white/90">{r.name}</p>
                        <div className="mt-1.5">
                          <Pill tone={r.tone}>{r.status}</Pill>
                        </div>
                      </li>
                    ))}
                  </ul>
                </div>
              ))}
            </div>
            <div className="rounded-2xl border border-white/10 bg-white/[0.03] p-3">
              <p className="mb-2 text-[11px] font-medium text-white/50">billing-api · latest verification</p>
              <ul className="grid gap-1.5 font-mono text-[11px] sm:grid-cols-3">
                <li className="flex items-center justify-between rounded-lg bg-black/30 px-2 py-1.5 text-white/80">
                  pnpm install <Pill tone="lime">passed</Pill>
                </li>
                <li className="flex items-center justify-between rounded-lg bg-black/30 px-2 py-1.5 text-white/80">
                  pnpm run lint <Pill tone="lime">passed</Pill>
                </li>
                <li className="flex items-center justify-between rounded-lg bg-black/30 px-2 py-1.5 text-white/80">
                  pnpm run test <Pill tone="rose">exit 1</Pill>
                </li>
              </ul>
            </div>
          </div>
        </div>
      </div>
      <figcaption className="mt-3 text-center font-mono text-[11px] text-white/40">
        Illustrative mockup of the desktop app. Repository names and statuses are examples.
      </figcaption>
    </figure>
  );
}
