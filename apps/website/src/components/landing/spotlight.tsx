"use client";

import { useRef, type ReactNode } from "react";

/**
 * A soft lime glow that follows a fine pointer across the hero. Purely
 * decorative: no effect for touch, keyboard or reduced-motion visitors.
 */
export function Spotlight({ children, className }: { children: ReactNode; className?: string }) {
  const ref = useRef<HTMLDivElement>(null);
  return (
    <div
      ref={ref}
      className={className}
      onPointerMove={(e) => {
        if (e.pointerType !== "mouse" || !ref.current) return;
        if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;
        const box = ref.current.getBoundingClientRect();
        ref.current.style.setProperty("--spot-x", `${e.clientX - box.left}px`);
        ref.current.style.setProperty("--spot-y", `${e.clientY - box.top}px`);
      }}
    >
      {children}
    </div>
  );
}
