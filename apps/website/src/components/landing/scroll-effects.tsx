"use client";

import gsap from "gsap";
import { ScrollTrigger } from "gsap/ScrollTrigger";
import { useEffect } from "react";

/**
 * A few scroll-linked transitions for the landing page. Content is fully
 * visible without JavaScript and when the visitor prefers reduced motion.
 */
export function ScrollEffects() {
  useEffect(() => {
    if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;
    gsap.registerPlugin(ScrollTrigger);
    const ctx = gsap.context(() => {
      gsap.from("[data-hero-line]", { yPercent: 60, opacity: 0, stagger: 0.09, duration: 0.9, ease: "power4.out" });
      for (const el of gsap.utils.toArray<HTMLElement>("[data-reveal]")) {
        gsap.from(el, {
          y: 36,
          opacity: 0,
          duration: 0.8,
          ease: "power3.out",
          scrollTrigger: { trigger: el, start: "top 88%", once: true },
        });
      }
      gsap.fromTo(
        "[data-preview]",
        { scale: 0.9, rotateX: 14, y: 48 },
        {
          scale: 1,
          rotateX: 0,
          y: 0,
          ease: "none",
          scrollTrigger: { trigger: "[data-preview]", start: "top 98%", end: "top 30%", scrub: true },
        },
      );
      gsap.fromTo(
        "[data-progress]",
        { scaleY: 0 },
        {
          scaleY: 1,
          ease: "none",
          transformOrigin: "top center",
          scrollTrigger: { trigger: "[data-workflow]", start: "top 65%", end: "bottom 65%", scrub: true },
        },
      );
    });
    return () => ctx.revert();
  }, []);
  return null;
}
