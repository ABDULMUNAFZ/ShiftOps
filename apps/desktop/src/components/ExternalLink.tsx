import { useMutation } from "@tanstack/react-query";
import { ExternalLink as ExternalIcon } from "lucide-react";
import type { ReactNode } from "react";
import { native } from "../lib/native";
import { ErrorDisplay } from "./ErrorDisplay";

/**
 * Opens a URL in the system browser through the native URL validator. The
 * web view never navigates away and never opens URLs itself.
 */
export function ExternalLink({ href, children }: { href: string; children: ReactNode }) {
  const open = useMutation({ mutationFn: () => native.openExternalUrl(href) });
  return (
    <>
      <button
        type="button"
        onClick={() => open.mutate()}
        className="inline-flex items-center gap-1 text-accent underline underline-offset-2 hover:text-accent/80"
        title={href}
      >
        {children}
        <ExternalIcon className="size-3" aria-hidden />
      </button>
      {open.isError ? <ErrorDisplay title="Link not opened" error={open.error} /> : null}
    </>
  );
}
