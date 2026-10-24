import { Slot } from "@radix-ui/react-slot";
import { cva, type VariantProps } from "class-variance-authority";
import type { ComponentProps, ReactNode } from "react";
import { cn } from "./cn";

export const buttonVariants = cva(
  "inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-md text-sm font-medium transition-colors disabled:pointer-events-none disabled:opacity-50 [&_svg]:size-4 [&_svg]:shrink-0",
  {
    variants: {
      variant: {
        primary: "bg-accent text-accent-fg hover:bg-accent/90",
        secondary: "border border-border bg-surface text-fg hover:bg-surface-2",
        ghost: "text-muted hover:bg-surface-2 hover:text-fg",
        danger: "bg-danger text-white hover:bg-danger/90",
      },
      size: {
        sm: "h-7 px-2.5 text-xs",
        md: "h-8 px-3",
        lg: "h-10 px-4",
        icon: "size-8",
      },
    },
    defaultVariants: { variant: "secondary", size: "md" },
  },
);

export interface ButtonProps extends ComponentProps<"button">, VariantProps<typeof buttonVariants> {
  asChild?: boolean;
}

export function Button({ className, variant, size, asChild = false, type, ...props }: ButtonProps) {
  const Comp = asChild ? Slot : "button";
  return (
    <Comp
      className={cn(buttonVariants({ variant, size }), className)}
      {...(asChild ? {} : { type: type ?? "button" })}
      {...props}
    />
  );
}

export function Card({ className, ...props }: ComponentProps<"section">) {
  return <section className={cn("rounded-lg border border-border bg-surface", className)} {...props} />;
}

export function CardHeader({
  title,
  description,
  actions,
  className,
}: {
  title: ReactNode;
  description?: ReactNode;
  actions?: ReactNode;
  className?: string;
}) {
  return (
    <header className={cn("flex items-start justify-between gap-4 border-b border-border px-4 py-3", className)}>
      <div className="min-w-0">
        <h2 className="text-sm font-semibold">{title}</h2>
        {description ? <p className="mt-0.5 text-xs text-muted">{description}</p> : null}
      </div>
      {actions ? <div className="flex shrink-0 items-center gap-2">{actions}</div> : null}
    </header>
  );
}

const badgeVariants = cva("inline-flex items-center gap-1.5 rounded px-1.5 py-0.5 text-xs font-medium", {
  variants: {
    tone: {
      neutral: "bg-surface-2 text-muted",
      ok: "bg-ok/12 text-ok",
      warn: "bg-warn/15 text-warn",
      danger: "bg-danger/12 text-danger",
      accent: "bg-accent-soft text-accent",
    },
  },
  defaultVariants: { tone: "neutral" },
});

export type BadgeTone = NonNullable<VariantProps<typeof badgeVariants>["tone"]>;

export function Badge({ tone, className, ...props }: ComponentProps<"span"> & { tone?: BadgeTone }) {
  return <span className={cn(badgeVariants({ tone }), className)} {...props} />;
}

export function StatusDot({ tone, label }: { tone: BadgeTone; label: string }) {
  const color = {
    neutral: "bg-muted",
    ok: "bg-ok",
    warn: "bg-warn",
    danger: "bg-danger",
    accent: "bg-accent",
  }[tone];
  return <span role="img" aria-label={label} className={cn("inline-block size-2 shrink-0 rounded-full", color)} />;
}

export function Kbd({ className, ...props }: ComponentProps<"kbd">) {
  return (
    <kbd
      className={cn(
        "inline-flex h-5 min-w-5 items-center justify-center rounded border border-border bg-surface-2 px-1 font-mono text-[10px] text-muted",
        className,
      )}
      {...props}
    />
  );
}

export function Spinner({ className, label = "Loading" }: { className?: string; label?: string }) {
  return (
    <span
      role="status"
      aria-label={label}
      className={cn("inline-block size-4 animate-spin rounded-full border-2 border-border border-t-accent", className)}
    />
  );
}

export function EmptyState({
  icon,
  title,
  children,
  action,
  className,
}: {
  icon?: ReactNode;
  title: string;
  children?: ReactNode;
  action?: ReactNode;
  className?: string;
}) {
  return (
    <div className={cn("flex flex-col items-center justify-center px-6 py-14 text-center", className)}>
      {icon ? <div className="mb-3 text-muted [&_svg]:size-6">{icon}</div> : null}
      <h3 className="text-sm font-semibold">{title}</h3>
      {children ? <div className="mt-1 max-w-md text-sm text-muted">{children}</div> : null}
      {action ? <div className="mt-4">{action}</div> : null}
    </div>
  );
}

export function ErrorState({ title, error, action }: { title: string; error: unknown; action?: ReactNode }) {
  const message = error instanceof Error ? error.message : String(error);
  return (
    <div role="alert" className="rounded-md border border-danger/40 bg-danger/8 px-4 py-3 text-sm">
      <p className="font-medium text-danger">{title}</p>
      <pre className="mt-1 whitespace-pre-wrap break-words font-mono text-xs text-fg/80">{message}</pre>
      {action ? <div className="mt-3">{action}</div> : null}
    </div>
  );
}

export function Logo({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true" className={cn("size-5", className)} fill="none">
      <rect x="2" y="2" width="20" height="20" rx="5" className="fill-accent" />
      <path
        d="M7 9.5h7.5M11.5 6.5l3 3-3 3M17 14.5H9.5M12.5 11.5l-3 3 3 3"
        className="stroke-accent-fg"
        strokeWidth="1.8"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}
