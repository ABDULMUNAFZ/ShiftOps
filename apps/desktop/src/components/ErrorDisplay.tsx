import { toAppError } from "@shiftops/core";
import { Badge, Button } from "@shiftops/ui";
import { Check, Copy } from "lucide-react";
import { useState, type ReactNode } from "react";

/** Copy text to the clipboard, reporting success so the UI can confirm it. */
export function useCopy(): [boolean, (text: string) => void] {
  const [copied, setCopied] = useState(false);
  const copy = (text: string) => {
    void navigator.clipboard
      ?.writeText(text)
      .then(() => {
        setCopied(true);
        setTimeout(() => setCopied(false), 1500);
      })
      .catch(() => setCopied(false));
  };
  return [copied, copy];
}

export function CopyButton({ text, label = "Copy details" }: { text: string; label?: string }) {
  const [copied, copy] = useCopy();
  return (
    <Button size="sm" variant="ghost" onClick={() => copy(text)} aria-label={label}>
      {copied ? <Check /> : <Copy />}
      {copied ? "Copied" : label}
    </Button>
  );
}

/**
 * Renders the structured error contract: message, stable code, suggested
 * action, and expandable, copyable technical details.
 */
export function ErrorDisplay({ title, error, action }: { title: string; error: unknown; action?: ReactNode }) {
  const e = toAppError(error);
  const diagnostic = JSON.stringify(e, null, 2);
  return (
    <div role="alert" className="rounded-md border border-danger/40 bg-danger/8 px-4 py-3 text-sm">
      <div className="flex flex-wrap items-center gap-2">
        <p className="font-medium text-danger">{title}</p>
        <Badge tone="danger" className="font-mono">
          {e.code}
        </Badge>
      </div>
      <p className="selectable mt-1 text-fg">{e.message}</p>
      {e.suggestedAction ? <p className="mt-1 text-muted">{e.suggestedAction}</p> : null}
      <details className="mt-2">
        <summary className="cursor-pointer text-xs text-muted">Technical details</summary>
        <pre className="selectable mt-2 max-h-60 overflow-auto whitespace-pre-wrap break-words rounded bg-surface-2 p-2 font-mono text-xs">
          {diagnostic}
        </pre>
      </details>
      <div className="mt-2 flex flex-wrap items-center gap-2">
        <CopyButton text={diagnostic} />
        {action}
      </div>
    </div>
  );
}
