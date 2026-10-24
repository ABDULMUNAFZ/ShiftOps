import { brand } from "@shiftops/ui";
import type { Metadata } from "next";
import type { ReactNode } from "react";
import { SiteFooter, SiteHeader } from "../components/site-chrome";
import "./globals.css";

export const metadata: Metadata = {
  title: { default: `${brand.name}: ${brand.tagline}`, template: `%s · ${brand.name}` },
  description: brand.description,
};

// Applies the saved or OS theme before first paint to avoid a flash.
const themeScript = `(function(){try{var t=localStorage.getItem('shiftops.site.theme');var d=t?t==='dark':matchMedia('(prefers-color-scheme: dark)').matches;document.documentElement.classList.toggle('dark',d)}catch(e){}})()`;

export default function RootLayout({ children }: { children: ReactNode }) {
  return (
    <html lang="en" suppressHydrationWarning>
      <head>
        <script dangerouslySetInnerHTML={{ __html: themeScript }} />
      </head>
      <body className="min-h-screen bg-bg text-fg">
        <SiteHeader />
        <main className="mx-auto w-full max-w-5xl px-4 sm:px-6">{children}</main>
        <SiteFooter />
      </body>
    </html>
  );
}
