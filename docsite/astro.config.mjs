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
          label: "Guides",
          items: [
            { slug: "docs/guide/index" },
            {
              label: "Start here",
              items: [
                {
                  autogenerate: {
                    directory: docsSidebarDirectory("guide/start"),
                  },
                },
              ],
            },
            {
              label: "Deploy",
              collapsed: true,
              items: [
                {
                  autogenerate: {
                    directory: docsSidebarDirectory("guide/deploy"),
                  },
                },
              ],
            },
            {
              label: "Operate",
              collapsed: true,
              items: [
                {
                  slug: "docs/guide/operate/index",
                },
                {
                  label: "Server operations",
                  collapsed: true,
                  items: [
                    {
                      autogenerate: {
                        directory: docsSidebarDirectory("guide/operate/server"),
                      },
                    },
                  ],
                },
                {
                  label: "Authentication & agents",
                  collapsed: true,
                  items: [
                    {
                      autogenerate: {
                        directory: docsSidebarDirectory("guide/operate/auth"),
                      },
                    },
                  ],
                },
                {
                  label: "Spaces & storage",
                  collapsed: true,
                  items: [
                    {
                      autogenerate: {
                        directory: docsSidebarDirectory(
                          "guide/operate/storage",
                        ),
                      },
                    },
                  ],
                },
              ],
            },
            {
              label: "Automate",
              collapsed: true,
              items: [
                {
                  autogenerate: {
                    directory: docsSidebarDirectory("guide/automate"),
                  },
                },
              ],
            },
            {
              label: "Develop",
              collapsed: true,
              items: [
                {
                  autogenerate: {
                    directory: docsSidebarDirectory("guide/develop"),
                  },
                },
              ],
            },
            {
              label: "Troubleshoot",
              collapsed: true,
              items: [
                {
                  autogenerate: {
                    directory: docsSidebarDirectory("guide/troubleshoot"),
                  },
                },
              ],
            },
          ],
        },
        {
          label: "Architecture",
          collapsed: true,
          items: [
            { slug: "docs/architecture/index" },
            {
              label: "Principles",
              collapsed: true,
              items: [
                {
                  autogenerate: {
                    directory: docsSidebarDirectory("architecture/principles"),
                  },
                },
              ],
            },
            {
              label: "Boundaries",
              collapsed: true,
              items: [
                {
                  autogenerate: {
                    directory: docsSidebarDirectory("architecture/boundaries"),
                  },
                },
              ],
            },
            {
              label: "Security",
              collapsed: true,
              items: [
                {
                  autogenerate: {
                    directory: docsSidebarDirectory("architecture/security"),
                  },
                },
              ],
            },
            {
              label: "Release",
              collapsed: true,
              items: [
                {
                  autogenerate: {
                    directory: docsSidebarDirectory("architecture/release"),
                  },
                },
              ],
            },
          ],
        },
        {
          label: "Specification",
          collapsed: true,
          items: [
            { slug: "docs/spec/index" },
            {
              label: "Core model",
              collapsed: true,
              items: [
                {
                  autogenerate: {
                    directory: docsSidebarDirectory("spec/data-model"),
                    collapsed: true,
                  },
                },
                {
                  autogenerate: {
                    directory: docsSidebarDirectory("spec/features"),
                    collapsed: true,
                  },
                },
              ],
            },
            {
              label: "Interfaces",
              collapsed: true,
              items: [
                {
                  autogenerate: {
                    directory: docsSidebarDirectory("spec/api"),
                    collapsed: true,
                  },
                },
                {
                  autogenerate: {
                    directory: docsSidebarDirectory("spec/ui"),
                    collapsed: true,
                  },
                },
              ],
            },
            {
              label: "Requirements & stories",
              collapsed: true,
              items: [
                {
                  autogenerate: {
                    directory: docsSidebarDirectory("spec/requirements"),
                    collapsed: true,
                  },
                },
                {
                  autogenerate: {
                    directory: docsSidebarDirectory("spec/stories"),
                    collapsed: true,
                  },
                },
              ],
            },
            {
              label: "Architecture contracts",
              collapsed: true,
              items: [
                {
                  autogenerate: {
                    directory: docsSidebarDirectory("spec/architecture"),
                    collapsed: true,
                  },
                },
              ],
            },
            {
              label: "Operations & quality",
              collapsed: true,
              items: [
                {
                  autogenerate: {
                    directory: docsSidebarDirectory("spec/policies"),
                    collapsed: true,
                  },
                },
                {
                  autogenerate: {
                    directory: docsSidebarDirectory("spec/security"),
                    collapsed: true,
                  },
                },
                {
                  autogenerate: {
                    directory: docsSidebarDirectory("spec/testing"),
                    collapsed: true,
                  },
                },
                {
                  autogenerate: {
                    directory: docsSidebarDirectory("spec/quality"),
                    collapsed: true,
                  },
                },
                {
                  autogenerate: {
                    directory: docsSidebarDirectory("spec/product"),
                    collapsed: true,
                  },
                },
                {
                  autogenerate: {
                    directory: docsSidebarDirectory("spec/versions"),
                    collapsed: true,
                  },
                },
              ],
            },
          ],
        },
      ],
    }),
  ],
});
