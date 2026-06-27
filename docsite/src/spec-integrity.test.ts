import { promises as fs } from "node:fs";
import path from "node:path";
import { parse } from "yaml";
import { describe, expect, test } from "vitest";

const repoRoot = path.resolve(process.cwd(), "..");
const specRoot = path.join(repoRoot, "docs/spec");

describe("executable documentation sources", () => {
  test("REQ-OPS-003: requirement IDs, related documents, and test references resolve", async () => {
    const requirementDir = path.join(specRoot, "requirements");
    const ids = new Set<string>();

    for (const filename of await yamlFiles(requirementDir)) {
      const source = parse(
        await fs.readFile(path.join(requirementDir, filename), "utf8"),
      ) as { requirements?: Requirement[] };
      for (const requirement of source.requirements ?? []) {
        expect(
          ids.has(requirement.id),
          `duplicate requirement ${requirement.id}`,
        ).toBe(
          false,
        );
        ids.add(requirement.id);

        for (const related of requirement.related_spec ?? []) {
          const relativePath = related.split("#", 1)[0];
          await expectOnePath(
            [
              path.resolve(specRoot, relativePath),
              path.resolve(requirementDir, relativePath),
            ],
            requirement.id,
          );
        }
        for (const reference of requirement.tests ?? []) {
          await expectPath(
            path.resolve(repoRoot, reference.file),
            requirement.id,
          );
        }
      }
    }
    expect(ids.size).toBeGreaterThan(40);
  });

  test("REQ-API-004: feature registry files and implementation paths resolve", async () => {
    const featureRoot = path.join(specRoot, "features");
    const registry = parse(
      await fs.readFile(path.join(featureRoot, "features.yaml"), "utf8"),
    ) as { files?: Array<{ file: string }> };

    for (const entry of registry.files ?? []) {
      const featurePath = path.join(featureRoot, entry.file);
      await expectPath(featurePath, entry.file);
      const feature = parse(await fs.readFile(featurePath, "utf8"));
      for (const implementationPath of collectFileValues(feature)) {
        await expectPath(
          path.resolve(repoRoot, implementationPath),
          entry.file,
        );
      }
    }
  });

  test("REQ-API-013: MCP documentation describes the shipped resource-first boundary", async () => {
    const source = await fs.readFile(path.join(specRoot, "api/mcp.md"), "utf8");
    expect(source).toContain("ugoite://{space_id}/entries/list");
    expect(source).toMatch(/no MCP tools or prompts/i);
    expect(source).toMatch(/v0\.2/i);
  });
});

type Requirement = {
  id: string;
  related_spec?: string[];
  tests?: Array<{ file: string }>;
};

async function yamlFiles(directory: string): Promise<string[]> {
  return (await fs.readdir(directory))
    .filter((filename) => filename.endsWith(".yaml"))
    .sort();
}

async function expectPath(filePath: string, owner: string): Promise<void> {
  await expect(fs.stat(filePath), `${owner}: missing ${filePath}`).resolves
    .toBeDefined();
}

async function expectOnePath(
  filePaths: string[],
  owner: string,
): Promise<void> {
  for (const filePath of filePaths) {
    try {
      await fs.stat(filePath);
      return;
    } catch {
      // Try the next documented relative-path convention.
    }
  }
  throw new Error(`${owner}: missing one of ${filePaths.join(", ")}`);
}

function collectFileValues(value: unknown): string[] {
  if (Array.isArray(value)) {
    return value.flatMap(collectFileValues);
  }
  if (!value || typeof value !== "object") {
    return [];
  }

  const files: string[] = [];
  for (const [key, child] of Object.entries(value)) {
    if (key === "file" && typeof child === "string") {
      files.push(child);
    } else {
      files.push(...collectFileValues(child));
    }
  }
  return files;
}
