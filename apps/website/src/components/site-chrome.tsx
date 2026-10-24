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
    <header className="sticky top-0 z-10 border-b border-border bg-bg/90 backdrop-blur">
      <div className="mx-auto flex h-14 max-w-5xl items-center justify-between gap-4 px-4 sm:px-6">
        <Link href="/" className="flex items-center gap-2 font-semibold tracking-tight">
          <Logo /> {brand.name}
        </Link>
        <nav className="flex items-center gap-1 text-sm">
          {LINKS.map((l) => (
            <Link key={l.href} href={l.href} className="rounded-md px-2.5 py-1.5 text-muted hover:bg-surface-2 hover:text-fg">
              {l.label}
            </Link>
          ))}
          <ThemeToggle />
        </nav>
      </div>
    </header>
  );
}

export function SiteFooter() {
  return (
    <footer className="mt-24 border-t border-border">
      <div className="mx-auto flex max-w-5xl flex-wrap items-center justify-between gap-4 px-4 py-8 text-sm text-muted sm:px-6">
        <span>
          {brand.name}: {brand.tagline}
        </span>
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
