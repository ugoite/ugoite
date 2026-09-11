import { satteri } from "@astrojs/markdown-satteri";
import starlight from "@astrojs/starlight";
import { defineConfig } from "astro/config";
import { docsSidebarDirectory, docsSourceDirectory } from "./src/docs-ssot.mjs";
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
        "Documentation for Ugoite, a private, portable Knowledge Space built around operator-owned Spaces.",
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
        processedDirs: [docsSourceDirectory],
      },
      sidebar: [
        { slug: "index" },
        {
          label: "Get started",
          items: [
            {
              autogenerate: { directory: docsSidebarDirectory("get-started") },
            },
          ],
        },
        {
          label: "Use Ugoite",
          items: [
            { autogenerate: { directory: docsSidebarDirectory("use") } },
          ],
        },
        {
          label: "Operate Ugoite",
          items: [
            { autogenerate: { directory: docsSidebarDirectory("operate") } },
          ],
        },
        {
          label: "Vision & Concepts",
          items: [
            { autogenerate: { directory: docsSidebarDirectory("vision") } },
          ],
        },
        {
          label: "Develop Ugoite",
          items: [
            { autogenerate: { directory: docsSidebarDirectory("develop") } },
          ],
        },
        {
          label: "Reference",
          items: [
            { autogenerate: { directory: docsSidebarDirectory("reference") } },
          ],
        },
        {
          label: "Specification",
          items: [
            {
              autogenerate: { directory: docsSidebarDirectory("spec") },
            },
          ],
        },
      ],
    }),
  ],
});
