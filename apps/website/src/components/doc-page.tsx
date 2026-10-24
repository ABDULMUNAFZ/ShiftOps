import type { ReactNode } from "react";

export function DocPage({ title, lead, children }: { title: string; lead?: string; children: ReactNode }) {
  return (
    <article className="prose-doc mx-auto max-w-3xl px-4 py-14 sm:px-6">
      <h1 className="text-3xl font-semibold tracking-tight">{title}</h1>
      {lead ? <p className="mt-3 text-lg text-muted">{lead}</p> : null}
      {children}
    </article>
  );
}
