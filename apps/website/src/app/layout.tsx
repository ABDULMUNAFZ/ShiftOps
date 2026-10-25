import { brand } from "@shiftops/ui";
import type { Metadata } from "next";
import type { ReactNode } from "react";
import { SiteFooter, SiteHeader } from "../components/site-chrome";
import { SITE_URL } from "../release";
import "./globals.css";

export const metadata: Metadata = {
  metadataBase: new URL(SITE_URL),
  title: { default: `${brand.name}: ${brand.tagline}`, template: `%s · ${brand.name}` },
  description: brand.description,
  alternates: { canonical: "/" },
  openGraph: {
    type: "website",
    url: SITE_URL,
    siteName: brand.name,
    title: `${brand.name}: ${brand.tagline}`,
    description: brand.description,
  },
};

// Applies the saved theme before first paint to avoid a flash; dark by default.
const themeScript = `(function(){try{var t=localStorage.getItem('shiftops.site.theme');var d=t?t==='dark':true;document.documentElement.classList.toggle('dark',d)}catch(e){}})()`;

export default function RootLayout({ children }: { children: ReactNode }) {
  return (
    <html lang="en" suppressHydrationWarning>
      <head>
        <script dangerouslySetInnerHTML={{ __html: themeScript }} />
      </head>
      <body className="min-h-screen bg-bg text-fg">
        <SiteHeader />
        <main className="w-full">{children}</main>
        <SiteFooter />
      </body>
    </html>
  );
}
