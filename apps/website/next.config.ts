import type { NextConfig } from "next";

const config: NextConfig = {
  // Fully static: the site has no server-side behaviour and can be hosted anywhere.
  output: "export",
  trailingSlash: true,
  transpilePackages: ["@shiftops/ui"],
  images: { unoptimized: true },
};

export default config;
