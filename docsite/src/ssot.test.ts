import { describe, expect, test } from "vitest";
import { promises as fs } from "node:fs";
import path from "node:path";
import { rewriteDocLink } from "./satteri-doc-links.mjs";

const repoRoot = path.resolve(process.cwd(), "..");
const docsRoot = path.join(repoRoot, "docs");

describe("documentation single source of truth", () => {
  test("Starlight loads the repository docs directory directly", async () => {
    const config = await fs.readFile(
      path.join(process.cwd(), "src/content.config.ts"),
      "utf8",
    );
    expect(config).toContain('base: "../docs"');
    expect(config).toContain("docsSchema()");
  });

  test("external docs use Starlight's native Sätteri pipeline", async () => {
    const config = await fs.readFile(
      path.join(process.cwd(), "astro.config.mjs"),
      "utf8",
    );
    expect(config).toContain(
      'import { satteri } from "@astrojs/markdown-satteri"',
    );
    expect(config).toContain(
      "processor: satteri({ mdastPlugins: [satteriDocLinks] })",
    );
    expect(config).toContain('processedDirs: ["../docs"]');
    expect(config).not.toContain("@astrojs/markdown-remark");
    expect(config).not.toContain("GITHUB_ACTIONS");
    expect(config).not.toContain("http://localhost");
  });

  test("docsite contains no hand-authored route tree", async () => {
    await expect(fs.stat(path.join(process.cwd(), "src/pages"))).rejects
      .toThrow();
  });

  test("GitHub Markdown links become Starlight routes at build time", () => {
    expect(
      rewriteDocLink(
        "../guide/cli.md#core-mode",
        "/repo/docs/spec/index.md",
      ),
    ).toBe("../guide/cli/#core-mode");
    expect(
      rewriteDocLink("features/index.md", "/repo/docs/spec/index.md"),
    ).toBe("features/");
    expect(
      rewriteDocLink("https://example.com/file.md", "/repo/docs/index.md"),
    ).toBe("https://example.com/file.md");
    expect(rewriteDocLink("guide/cli.md", "/repo/docs/index.md")).toBe(
      "docs/guide/cli/",
    );
    expect(
      rewriteDocLink(
        "sibling.md",
        "/repo/docs/spec/quality/error-handling.md",
      ),
    ).toBe("../sibling/");
    expect(
      rewriteDocLink(
        "../architecture/overview.md",
        "/repo/docs/spec/quality/error-handling.md",
      ),
    ).toBe("../../architecture/overview/");
  });

  test("all rendered Markdown pages declare Starlight metadata", async () => {
    const files = await collectMarkdown(docsRoot);
    expect(files.length).toBeGreaterThan(40);
    for (const file of files) {
      const source = await fs.readFile(file, "utf8");
      expect(source.startsWith("---\n"), file).toBe(true);
      const closing = source.indexOf("\n---\n", 4);
      expect(closing, file).toBeGreaterThan(3);
      const frontmatter = source.slice(4, closing);
      expect(/^title:\s*.+$/m.test(frontmatter), file).toBe(true);
    }
  });

  test("authored pages follow Starlight heading structure", async () => {
    const files = await collectMarkdown(docsRoot);
    for (const file of files) {
      const source = await fs.readFile(file, "utf8");
      const body = markdownBody(source);
      const authoredLines = linesOutsideCodeFences(body);
      const headings = authoredLines
        .filter((line) => /^(?:#{1,6})\s+\S/.test(line))
        .map((line) => line.match(/^#+/)?.[0].length ?? 0);

      expect(headings, `${file} must not contain a body-level H1`).not
        .toContain(1);
      for (let index = 1; index < headings.length; index += 1) {
        expect(
          headings[index] - headings[index - 1],
          `${file} must not skip heading levels`,
        ).toBeLessThanOrEqual(1);
      }

      const firstContent = authoredLines
        .map((line) => line.trim())
        .find((line) => line && !line.startsWith("<!--"));
      if (headings.length > 0) {
        expect(
          firstContent?.startsWith("#"),
          `${file} should introduce the page before its first section`,
        ).toBe(false);
      }
    }
  });
});

function linesOutsideCodeFences(source: string): string[] {
  const lines: string[] = [];
  let fence: "```" | "~~~" | undefined;
  for (const line of source.split("\n")) {
    const marker = line.trimStart().match(/^(```|~~~)/)?.[1] as
      | "```"
      | "~~~"
      | undefined;
    if (marker) {
      fence = fence === marker ? undefined : fence ?? marker;
      continue;
    }
    if (!fence) {
      lines.push(line);
    }
  }
  return lines;
}

function markdownBody(source: string): string {
  if (!source.startsWith("---\n")) {
    return source;
  }
  const closing = source.indexOf("\n---\n", 4);
  return closing === -1 ? source : source.slice(closing + 5);
}

async function collectMarkdown(root: string): Promise<string[]> {
  const files: string[] = [];
  for (const entry of await fs.readdir(root, { withFileTypes: true })) {
    const fullPath = path.join(root, entry.name);
    if (entry.isDirectory()) {
      files.push(...(await collectMarkdown(fullPath)));
    } else if (/\.mdx?$/.test(entry.name)) {
      files.push(fullPath);
    }
  }
  return files;
}
