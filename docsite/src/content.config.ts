import { defineCollection } from "astro:content";
import { glob } from "astro/loaders";
import { docsSchema } from "@astrojs/starlight/schema";
import { docsSourceDirectory } from "./docs-ssot.mjs";

function documentationId(entry: string): string {
  const id = entry.replace(/\.mdx?$/, "");
  return id === "index" || id === "404" ? id : `docs/${id}`;
}

export const collections = {
  docs: defineCollection({
    loader: glob({
      base: docsSourceDirectory,
      pattern: "**/*.{md,mdx}",
      generateId: ({ entry }) => documentationId(entry),
    }),
    schema: docsSchema(),
  }),
};
