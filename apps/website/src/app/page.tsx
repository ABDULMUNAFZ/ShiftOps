import { brand, buttonVariants, Card } from "@shiftops/ui";
import Link from "next/link";

const STEPS = [
  {
    title: "Define",
    body: "Write the change once: what to replace, the instructions an implementer needs, and the checks that prove it worked.",
  },
  {
    title: "Discover",
    body: "Find every repository the change touches, from local checkouts and GitHub organisations.",
  },
  {
    title: "Plan",
    body: "Order repositories by dependency so producers migrate before consumers. Independent repositories form parallel waves.",
  },
  {
    title: "Delegate",
    body: "Hand each repository to a Devin session with the campaign-level instructions and its verification commands.",
  },
  {
    title: "Verify",
    body: "Re-run each repository's checks against the agent's branch, independently of the agent's own report.",
  },
  {
    title: "Track",
    body: "Follow the campaign from one place: which repositories are planned, in progress, verified or blocked.",
  },
];

export default function Home() {
  return (
    <>
      <section className="py-20 sm:py-28">
        <p className="mb-4 font-mono text-xs uppercase tracking-widest text-accent">
          Codebase-wide change, run as a campaign
        </p>
        <h1 className="max-w-3xl text-4xl font-semibold tracking-tight sm:text-5xl">{brand.tagline}</h1>
        <p className="mt-6 max-w-2xl text-lg text-muted">
          {brand.name} is a desktop app for engineering organisations. Define one change, find every affected
          repository, plan the migration order, delegate repository-level work to Devin and verify each result yourself.
        </p>
        <div className="mt-8 flex flex-wrap gap-3">
          <Link href="/download/" className={buttonVariants({ variant: "primary", size: "lg" })}>
            Get {brand.name}
          </Link>
          <Link href="/docs/" className={buttonVariants({ variant: "secondary", size: "lg" })}>
            Read the docs
          </Link>
        </div>
      </section>

      <section aria-labelledby="how" className="border-t border-border py-16">
        <h2 id="how" className="text-sm font-semibold uppercase tracking-wide text-muted">
          How a campaign runs
        </h2>
        <ol className="mt-6 grid gap-px overflow-hidden rounded-lg border border-border bg-border sm:grid-cols-2 lg:grid-cols-3">
          {STEPS.map((step, i) => (
            <li key={step.title} className="bg-surface p-5">
              <p className="font-mono text-xs text-accent">{String(i + 1).padStart(2, "0")}</p>
              <h3 className="mt-2 font-semibold">{step.title}</h3>
              <p className="mt-1 text-sm leading-6 text-muted">{step.body}</p>
            </li>
          ))}
        </ol>
      </section>

      <section className="grid gap-4 border-t border-border py-16 md:grid-cols-2">
        <Card className="p-6">
          <h2 className="font-semibold">Runs on your machine</h2>
          <p className="mt-2 text-sm leading-6 text-muted">
            Repositories are read with your local Git. Application state is a SQLite file in your user data directory.
            Credentials are kept in the operating system keychain.
          </p>
          <Link href="/security/" className="mt-4 inline-block text-sm text-accent underline underline-offset-2">
            Security model
          </Link>
        </Card>
        <Card className="p-6">
          <h2 className="font-semibold">Current status</h2>
          <p className="mt-2 text-sm leading-6 text-muted">
            {brand.name} is in early development. The desktop shell and workstation checks work today. Repository
            discovery, planning, delegation and verification are being built in that order.
          </p>
          <Link href="/docs/" className="mt-4 inline-block text-sm text-accent underline underline-offset-2">
            What works today
          </Link>
        </Card>
      </section>
    </>
  );
}
