import { satteri } from "@astrojs/markdown-satteri";
import starlight from "@astrojs/starlight";
import { defineConfig } from "astro/config";
import satteriDocLinks from "./src/satteri-doc-links.mjs";

const configuredBase = process.env.DOCSITE_BASE ?? "/";
const withLeadingSlash = configuredBase.startsWith("/")
  ? configuredBase
  : `/${configuredBase}`;
const base = withLeadingSlash.endsWith("/")
  ? withLeadingSlash
  : `${withLeadingSlash}/`;
const site = process.env.DOCSITE_ORIGIN;

export default defineConfig({
  ...(site ? { site } : {}),
  base,
  markdown: {
    processor: satteri({ mdastPlugins: [satteriDocLinks] }),
  },
  integrations: [
    starlight({
      title: "Ugoite",
      description:
        "Documentation for Ugoite, a local-first knowledge-space system built around operator-owned files.",
      social: [
        {
          icon: "github",
          label: "GitHub",
          href: "https://github.com/ugoite/ugoite",
        },
      ],
      editLink: {
        baseUrl: "https://github.com/ugoite/ugoite/edit/main/docs/",
      },
      credits: true,
      markdown: {
        // `docs/` is intentionally outside Starlight's conventional collection
        // directory so the repository and the website share one source tree.
        processedDirs: ["../docs"],
      },
      sidebar: [
        { slug: "index" },
        {
          label: "Guides",
          items: [{ autogenerate: { directory: "docs/guide" } }],
        },
        {
          label: "Architecture",
          items: [{ autogenerate: { directory: "docs/architecture" } }],
        },
        {
          label: "Specification",
          collapsed: true,
          items: [
            {
              autogenerate: {
                directory: "docs/spec",
                collapsed: true,
              },
            },
          ],
        },
      ],
    }),
  ],
});
