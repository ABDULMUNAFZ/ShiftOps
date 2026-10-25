import type { MetadataRoute } from "next";
import { SITE_URL } from "../release";

export const dynamic = "force-static";

export default function sitemap(): MetadataRoute.Sitemap {
  return ["", "docs/", "download/", "security/", "privacy/"].map((path) => ({ url: `${SITE_URL}/${path}` }));
}
