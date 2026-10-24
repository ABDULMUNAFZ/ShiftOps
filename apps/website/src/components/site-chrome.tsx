import { brand, Logo } from "@shiftops/ui";
import Link from "next/link";
import { ThemeToggle } from "./theme-toggle";

const LINKS = [
  { href: "/docs/", label: "Docs" },
  { href: "/security/", label: "Security" },
  { href: "/privacy/", label: "Privacy" },
  { href: "/download/", label: "Download" },
];

export function SiteHeader() {
  return (
    <header className="sticky top-0 z-20 border-b border-border/70 bg-bg/75 backdrop-blur-xl">
      <div className="mx-auto flex h-16 max-w-6xl items-center justify-between gap-4 px-4 sm:px-6">
        <Link href="/" className="flex items-center gap-2 text-base font-semibold tracking-tight">
          <Logo className="size-6" /> {brand.name}
        </Link>
        <nav aria-label="Main" className="flex items-center gap-1 text-sm">
          {LINKS.map((l) =>
            l.href === "/download/" ? (
              <Link
                key={l.href}
                href={l.href}
                className="ml-1 rounded-full bg-accent px-4 py-2 font-semibold text-accent-fg transition hover:brightness-110"
              >
                {l.label}
              </Link>
            ) : (
              <Link
                key={l.href}
                href={l.href}
                className="hidden rounded-full px-3 py-2 text-muted transition hover:bg-surface-2 hover:text-fg sm:inline-block"
              >
                {l.label}
              </Link>
            ),
          )}
          <ThemeToggle />
        </nav>
      </div>
    </header>
  );
}

export function SiteFooter() {
  return (
    <footer className="border-t border-border">
      <div className="mx-auto flex max-w-6xl flex-wrap items-center justify-between gap-4 px-4 py-10 text-sm text-muted sm:px-6">
        <div className="flex flex-col gap-1">
          <span>
            {brand.name}: {brand.tagline}
          </span>
          <span className="font-mono text-xs">
            {brand.event} · Independent project, not affiliated with Cognition ·{" "}
            <a href={brand.repositoryUrl} className="underline underline-offset-2 hover:text-fg">
              Source on GitHub
            </a>
          </span>
        </div>
        <span className="flex gap-4">
          {LINKS.map((l) => (
            <Link key={l.href} href={l.href} className="hover:text-fg">
              {l.label}
            </Link>
          ))}
        </span>
      </div>
    </footer>
  );
}
