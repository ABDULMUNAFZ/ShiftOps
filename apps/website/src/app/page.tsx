import { brand } from "@shiftops/ui";
import {
  ArrowRight,
  Bot,
  FolderSearch,
  GitBranch,
  KeyRound,
  Layers,
  ListChecks,
  Lock,
  Radar,
  ShieldCheck,
  Waypoints,
  Wrench,
} from "lucide-react";
import Link from "next/link";
import type { ReactNode } from "react";
import { AppMockup } from "../components/landing/app-mockup";
import { ScrollEffects } from "../components/landing/scroll-effects";
import { Spotlight } from "../components/landing/spotlight";

const WORKFLOW = [
  {
    icon: FolderSearch,
    title: "Discover",
    body: "Open a checkout, search a folder, or import from GitHub: metadata only, a clone into an approved workspace, or a link to a clone you already have.",
  },
  {
    icon: Radar,
    title: "Scan",
    body: "A read-only scanner records runtimes, package managers, frameworks, scripts, Docker and CI, each with the file and line it came from.",
  },
  {
    icon: Waypoints,
    title: "Plan",
    body: "A deterministic planner classifies every repository against the goal, scores risk with cited evidence and orders dependency-first waves.",
  },
  {
    icon: Bot,
    title: "Delegate",
    body: "Each repository gets a generated Devin task. You approve its exact text; dispatch runs in Devin's sandbox, only for trusted, clean checkouts.",
  },
  {
    icon: ListChecks,
    title: "Verify",
    body: "ShiftOps runs the repository's own checks itself and records real exit codes. A finished agent session never counts as verified on its own.",
  },
  {
    icon: Wrench,
    title: "Repair",
    body: "Repairable failures become a bounded repair task, at most two per repository. The same approved checks re-run afterwards, only if unchanged.",
  },
];

const USE_CASES = [
  {
    title: "Runtime upgrades",
    body: "Move every service from Node.js 18 to 22 with engines, .nvmrc, Docker images and CI matrices found for you.",
    tag: "Verified end to end for Node.js",
  },
  {
    title: "Dependency rollouts",
    body: "See which repositories declare a package, at which range, and which internal packages depend on each other.",
    tag: "Planning and delegation",
  },
  {
    title: "CI and container changes",
    body: "Find every workflow and Dockerfile that needs the same edit, and track the change repository by repository.",
    tag: "Planning and delegation",
  },
];

const SAFEGUARDS = [
  {
    icon: Lock,
    title: "Local first",
    body: "State lives in SQLite on your machine. Repositories are read with your own Git.",
  },
  {
    icon: KeyRound,
    title: "Keychain only",
    body: "Tokens stay in the OS keychain and never reach the interface, logs or database.",
  },
  {
    icon: ShieldCheck,
    title: "Exact commands",
    body: "Every process runs from an allow-listed argument vector, never a shell string.",
  },
  {
    icon: GitBranch,
    title: "One mutating Git command",
    body: "An initial HTTPS clone into a workspace you approved. No hooks, no submodules, no tokens in URLs.",
  },
];

function Eyebrow({ children }: { children: ReactNode }) {
  return <p className="font-mono text-xs uppercase tracking-[0.25em] text-accent">{children}</p>;
}

function SectionTitle({ eyebrow, title, lead }: { eyebrow: string; title: ReactNode; lead?: string }) {
  return (
    <div data-reveal className="max-w-3xl">
      <Eyebrow>{eyebrow}</Eyebrow>
      <h2 className="mt-4 text-4xl font-semibold leading-[1.05] tracking-[-0.03em] text-fg sm:text-5xl">{title}</h2>
      {lead ? <p className="mt-5 text-lg leading-8 text-muted">{lead}</p> : null}
    </div>
  );
}

function PrimaryCta({ href, children }: { href: string; children: ReactNode }) {
  return (
    <Link
      href={href}
      className="group inline-flex h-12 items-center gap-2 rounded-full bg-accent px-6 text-sm font-semibold text-accent-fg transition hover:brightness-110 focus-visible:outline-2"
    >
      {children}
      <ArrowRight className="size-4 transition group-hover:translate-x-0.5" aria-hidden />
    </Link>
  );
}

function SecondaryCta({ href, children, external }: { href: string; children: ReactNode; external?: boolean }) {
  const className =
    "inline-flex h-12 items-center gap-2 rounded-full border border-border px-6 text-sm font-medium text-fg transition hover:bg-surface-2";
  return external ? (
    <a href={href} className={className} rel="noreferrer">
      {children}
    </a>
  ) : (
    <Link href={href} className={className}>
      {children}
    </Link>
  );
}

export default function Home() {
  return (
    <div className="dark landing bg-bg text-fg">
      <ScrollEffects />

      <Spotlight className="landing-hero relative overflow-hidden">
        <section className="relative mx-auto max-w-6xl px-4 pb-20 pt-20 sm:px-6 sm:pt-28">
          <div className="max-w-4xl">
            <div data-hero-line>
              <Eyebrow>Codebase-wide change, run as a campaign</Eyebrow>
            </div>
            <h1 className="mt-6 text-5xl font-semibold leading-[0.98] tracking-[-0.045em] sm:text-7xl">
              <span data-hero-line className="block">
                Define the change once.
              </span>
              <span data-hero-line className="block text-accent">
                Ship it everywhere.
              </span>
            </h1>
            <p data-hero-line className="mt-7 max-w-2xl text-lg leading-8 text-muted">
              {brand.description} Find every affected repository, plan the order, hand the work to Devin with your
              approval, and accept a change only when its own checks pass.
            </p>
            <div data-hero-line className="mt-9 flex flex-wrap gap-3">
              <PrimaryCta href="/download/">Get the desktop app</PrimaryCta>
              <SecondaryCta href="#workflow">See how it works</SecondaryCta>
              <SecondaryCta href={brand.repositoryUrl} external>
                Source on GitHub
              </SecondaryCta>
            </div>
          </div>
        </section>

        <section id="product" className="relative mx-auto max-w-6xl px-4 pb-24 sm:px-6 [perspective:1400px]">
          <div data-preview>
            <AppMockup />
          </div>
        </section>
      </Spotlight>

      <section id="problem" className="mx-auto max-w-6xl px-4 py-24 sm:px-6">
        <SectionTitle
          eyebrow="The problem"
          title={
            <>
              One upgrade.
              <br />
              Forty repositories.
            </>
          }
          lead="Organisation-wide changes stall in spreadsheets: nobody knows which repositories are affected, which order is safe, or whether an automated change actually works."
        />
        <div className="mt-14 grid gap-4 md:grid-cols-2">
          <div data-reveal className="rounded-[28px] border border-border bg-surface p-8">
            <p className="font-mono text-xs uppercase tracking-[0.2em] text-muted">Without ShiftOps</p>
            <ul className="mt-6 space-y-4 text-base leading-7 text-fg/80">
              <li>Grepping checkouts by hand to guess what is affected.</li>
              <li>Upgrading consumers before the libraries they depend on.</li>
              <li>Agent pull requests merged on the strength of a summary.</li>
              <li>No record of who approved what, or why.</li>
            </ul>
          </div>
          <div data-reveal className="rounded-[28px] bg-accent p-8 text-accent-fg">
            <p className="font-mono text-xs uppercase tracking-[0.2em] opacity-70">With ShiftOps</p>
            <ul className="mt-6 space-y-4 text-base font-medium leading-7">
              <li>Every repository scanned, with evidence down to the file and line.</li>
              <li>Dependency-ordered waves, with risk and blockers called out.</li>
              <li>Devin work only after you approve the exact task.</li>
              <li>A change accepted only when ShiftOps reruns the checks and they pass.</li>
            </ul>
          </div>
        </div>
      </section>

      <section id="workflow" className="border-y border-border bg-surface/40">
        <div className="mx-auto max-w-6xl px-4 py-24 sm:px-6">
          <SectionTitle
            eyebrow="How a campaign runs"
            title="Six steps, one place."
            lead="Each step stores what it saw and what it decided, so the campaign can be audited after the fact."
          />
          <ol data-workflow className="relative mt-14 space-y-4 pl-8 sm:pl-12">
            <span aria-hidden className="absolute left-3 top-2 bottom-2 w-px bg-border sm:left-5" />
            <span
              aria-hidden
              data-progress
              className="absolute left-3 top-2 bottom-2 w-px origin-top bg-accent sm:left-5"
            />
            {WORKFLOW.map((step, i) => (
              <li
                key={step.title}
                data-reveal
                className="relative grid gap-4 rounded-[24px] border border-border bg-bg p-6 sm:grid-cols-[auto_1fr] sm:p-7"
              >
                <span
                  aria-hidden
                  className="absolute -left-[26px] top-8 size-3 rounded-full border-2 border-accent bg-bg sm:-left-[34px]"
                />
                <div className="flex items-center gap-3">
                  <span className="grid size-11 place-items-center rounded-2xl bg-accent-soft text-accent">
                    <step.icon className="size-5" aria-hidden />
                  </span>
                  <span className="font-mono text-xs text-muted">{String(i + 1).padStart(2, "0")}</span>
                </div>
                <div>
                  <h3 className="text-xl font-semibold tracking-tight">{step.title}</h3>
                  <p className="mt-2 max-w-2xl leading-7 text-muted">{step.body}</p>
                </div>
              </li>
            ))}
          </ol>
        </div>
      </section>

      <section id="use-cases" className="mx-auto max-w-6xl px-4 py-24 sm:px-6">
        <SectionTitle eyebrow="Use cases" title="Built for the changes nobody wants to coordinate." />
        <div className="mt-14 grid gap-4 md:grid-cols-3">
          {USE_CASES.map((u) => (
            <article
              key={u.title}
              data-reveal
              className="flex flex-col justify-between rounded-[28px] border border-border bg-surface p-7"
            >
              <div>
                <Layers className="size-6 text-accent" aria-hidden />
                <h3 className="mt-6 text-2xl font-semibold tracking-tight">{u.title}</h3>
                <p className="mt-3 leading-7 text-muted">{u.body}</p>
              </div>
              <p className="mt-8 inline-flex w-fit rounded-full border border-border px-3 py-1 font-mono text-[11px] text-muted">
                {u.tag}
              </p>
            </article>
          ))}
        </div>
      </section>

      <section id="verification" className="mx-auto max-w-6xl px-4 pb-24 sm:px-6">
        <div className="grid gap-4 lg:grid-cols-[1.1fr_1fr]">
          <div data-reveal className="rounded-[32px] border border-border bg-surface p-8 sm:p-10">
            <Eyebrow>Independent verification</Eyebrow>
            <h2 className="mt-4 text-4xl font-semibold leading-[1.05] tracking-[-0.03em] sm:text-5xl">
              Trust the checks,
              <br />
              not the summary.
            </h2>
            <p className="mt-5 text-lg leading-8 text-muted">
              ShiftOps reads the plan from the repository&apos;s own <code className="font-mono">package.json</code> and
              lockfile, shows you every command and script body, and runs them only after you confirm. The plan is
              hashed: if a script changes, nothing runs until you review it again.
            </p>
          </div>
          <div className="grid gap-4 sm:grid-cols-2">
            {[
              ["Trusted repositories only", "Checked in the native core, not just the interface."],
              ["Real exit codes", "Every check stores its exit code, duration and redacted output."],
              ["Bounded repairs", "At most two repair tasks per repository. No infinite loops."],
              ["Nothing auto-verified", "Only a passing run marks a repository verified."],
            ].map(([title, body]) => (
              <div key={title} data-reveal className="rounded-[28px] border border-border bg-bg p-6">
                <ShieldCheck className="size-5 text-accent" aria-hidden />
                <h3 className="mt-5 font-semibold">{title}</h3>
                <p className="mt-2 text-sm leading-6 text-muted">{body}</p>
              </div>
            ))}
          </div>
        </div>
      </section>

      <section id="security" className="border-t border-border">
        <div className="mx-auto max-w-6xl px-4 py-24 sm:px-6">
          <SectionTitle eyebrow="Safety by construction" title="Your code, your machine, your approval." />
          <div className="mt-14 grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
            {SAFEGUARDS.map((s) => (
              <div key={s.title} data-reveal className="rounded-[28px] border border-border bg-surface p-6">
                <s.icon className="size-5 text-accent" aria-hidden />
                <h3 className="mt-5 font-semibold">{s.title}</h3>
                <p className="mt-2 text-sm leading-6 text-muted">{s.body}</p>
              </div>
            ))}
          </div>
          <p data-reveal className="mt-8 text-sm text-muted">
            Read the full{" "}
            <Link href="/security/" className="text-accent underline underline-offset-4">
              security model
            </Link>
            .
          </p>
        </div>
      </section>

      <section id="download" className="mx-auto max-w-6xl px-4 pb-28 sm:px-6">
        <div
          data-reveal
          className="relative overflow-hidden rounded-[36px] bg-accent px-8 py-14 text-accent-fg sm:px-14 sm:py-20"
        >
          <p className="font-mono text-xs uppercase tracking-[0.25em] opacity-70">Desktop app</p>
          <h2 className="mt-4 max-w-2xl text-4xl font-semibold leading-[1.02] tracking-[-0.035em] sm:text-6xl">
            Run your first campaign today.
          </h2>
          <p className="mt-5 max-w-xl text-lg leading-8 opacity-80">
            Unsigned pre-release installers for macOS, Windows and Linux are on GitHub Releases, or build ShiftOps from
            source in a few commands.
          </p>
          <div className="mt-9 flex flex-wrap gap-3">
            <Link
              href="/download/"
              className="inline-flex h-12 items-center gap-2 rounded-full bg-accent-fg px-6 text-sm font-semibold text-accent transition hover:opacity-90"
            >
              Download for Desktop <ArrowRight className="size-4" aria-hidden />
            </Link>
            <Link
              href="/docs/"
              className="inline-flex h-12 items-center rounded-full border border-current/30 px-6 text-sm font-medium transition hover:bg-black/5"
            >
              Read the docs
            </Link>
          </div>
        </div>
      </section>
    </div>
  );
}
