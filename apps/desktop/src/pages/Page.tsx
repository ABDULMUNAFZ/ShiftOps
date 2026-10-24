import type { ReactNode } from "react";

export function Page({
  title,
  description,
  actions,
  children,
}: {
  title: string;
  description: string;
  actions?: ReactNode;
  children: ReactNode;
}) {
  return (
    <div className="flex min-h-full flex-col">
      <div className="flex items-start justify-between gap-4 border-b border-border px-6 py-4">
        <div>
          <h1 className="text-base font-semibold">{title}</h1>
          <p className="mt-0.5 text-sm text-muted">{description}</p>
        </div>
        {actions}
      </div>
      <div className="flex-1 space-y-4 p-6">{children}</div>
    </div>
  );
}

/** Key/value rows for factual details such as paths and versions. */
export function DefinitionList({ rows }: { rows: { term: string; value: ReactNode }[] }) {
  return (
    <dl className="divide-y divide-border">
      {rows.map(({ term, value }) => (
        <div key={term} className="grid grid-cols-[12rem_1fr] gap-4 px-4 py-2.5">
          <dt className="text-muted">{term}</dt>
          <dd className="selectable min-w-0 break-words font-mono text-xs leading-5">{value}</dd>
        </div>
      ))}
    </dl>
  );
}
