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
        assertRequirementStatusIntegrity(requirement);
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

  test("REQ-OPS-004: version statuses agree with their tasks and canonical sources", async () => {
    const versionRoot = path.join(repoRoot, "docs/version");

    for (const filename of await yamlFilesRecursively(versionRoot)) {
      const filePath = path.join(versionRoot, filename);
      const document = parse(
        await fs.readFile(filePath, "utf8"),
      ) as VersionDocument;

      // Changelog entries are historical records, not status-bearing plans.
      if (document.status === undefined) continue;

      assertVersionStatus(document.status, filename);
      const phaseStatuses = [] as VersionStatus[];
      for (const phase of document.phases ?? []) {
        assertVersionStatus(phase.status, `${filename}:${phase.id}`);
        phaseStatuses.push(phase.status);
        expect(
          Array.isArray(phase.tasks),
          `${filename}:${phase.id}: status must be supported by tasks`,
        ).toBe(true);
        const tasks = phase.tasks ?? [];
        for (const [index, task] of tasks.entries()) {
          expect(
            typeof task.done,
            `${filename}:${phase.id}: task ${index} must declare done`,
          ).toBe("boolean");
        }
        expect(phase.status, `${filename}:${phase.id}: stale status`).toBe(
          statusFromTasks(tasks),
        );
      }

      const milestoneStatuses = [] as VersionStatus[];
      for (const milestone of document.milestones ?? []) {
        assertVersionStatus(milestone.status, `${filename}:${milestone.id}`);
        milestoneStatuses.push(milestone.status);
        await assertMilestoneSourceIntegrity(milestone, filename);
      }

      const childStatuses = document.milestones
        ? milestoneStatuses
        : phaseStatuses;
      expect(childStatuses, `${filename}: status has no children`).not
        .toHaveLength(0);
      expect(document.status, `${filename}: stale top-level status`).toBe(
        statusFromStatuses(childStatuses),
      );
    }
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

  test("REQ-API-013: MCP documentation describes the shipped semantic facade", async () => {
    const source = await fs.readFile(path.join(specRoot, "api/mcp.md"), "utf8");
    expect(source).toContain("POST /mcp");
    expect(source).toContain("ugoite.search");
    expect(source).toContain("ugoite://entry/{id}");
    expect(source).toContain("/.well-known/oauth-protected-resource");
    expect(source).toMatch(/DPoP/i);
  });
});

type Requirement = {
  id: string;
  status: string;
  verification: string;
  related_spec?: string[];
  tests?: Array<{ file: string; cases?: string[] }>;
};

type VersionStatus = "planned" | "in_progress" | "completed";

type VersionTask = { done?: boolean };

type VersionPhase = {
  id: string;
  status: VersionStatus;
  tasks?: VersionTask[];
};

type VersionMilestone = {
  id: string;
  status: VersionStatus;
  source: string[];
  phases: Array<{ id: string; status: VersionStatus }>;
};

type VersionDocument = {
  status?: string;
  phases?: VersionPhase[];
  milestones?: VersionMilestone[];
};

const requirementStatuses = new Set(["implemented", "planned", "superseded"]);
const verificationStatuses = new Set(["traced", "untraced"]);
const versionStatuses = new Set<VersionStatus>([
  "planned",
  "in_progress",
  "completed",
]);

function assertRequirementStatusIntegrity(requirement: Requirement): void {
  expect(
    requirementStatuses.has(requirement.status),
    `${requirement.id}: invalid requirement status`,
  ).toBe(true);
  expect(
    verificationStatuses.has(requirement.verification),
    `${requirement.id}: invalid verification status`,
  ).toBe(true);

  const tests = requirement.tests ?? [];
  if (requirement.verification === "traced") {
    expect(
      tests,
      `${requirement.id}: traced requirements need test references`,
    ).not.toHaveLength(0);
  } else {
    expect(
      tests,
      `${requirement.id}: untraced requirements cannot claim test references`,
    ).toHaveLength(0);
  }
}

function assertVersionStatus(
  status: string,
  owner: string,
): asserts status is VersionStatus {
  expect(
    versionStatuses.has(status as VersionStatus),
    `${owner}: invalid version status`,
  ).toBe(
    true,
  );
}

function statusFromTasks(tasks: VersionTask[]): VersionStatus {
  return statusFromStatuses(
    tasks.map((task) => (task.done === true ? "completed" : "planned")),
  );
}

function statusFromStatuses(statuses: VersionStatus[]): VersionStatus {
  if (statuses.every((status) => status === "completed")) return "completed";
  if (statuses.every((status) => status === "planned")) return "planned";
  return "in_progress";
}

async function assertMilestoneSourceIntegrity(
  milestone: VersionMilestone,
  owner: string,
): Promise<void> {
  expect(milestone.source, `${owner}:${milestone.id}: missing source`).not
    .toHaveLength(0);
  for (const source of milestone.source) {
    await expectPath(
      path.resolve(repoRoot, source),
      `${owner}:${milestone.id}`,
    );
  }

  const yamlSource = milestone.source.find((source) =>
    source.endsWith(".yaml")
  );
  expect(yamlSource, `${owner}:${milestone.id}: missing canonical YAML source`)
    .toBeDefined();
  const canonical = parse(
    await fs.readFile(path.resolve(repoRoot, yamlSource as string), "utf8"),
  ) as VersionDocument;
  expect(milestone.status, `${owner}:${milestone.id}: stale milestone status`)
    .toBe(
      canonical.status,
    );
  expect(
    milestone.phases.map(({ id, status }) => ({ id, status })),
    `${owner}:${milestone.id}: stale phase status summary`,
  ).toEqual(canonical.phases?.map(({ id, status }) => ({ id, status })));
}

async function yamlFiles(directory: string): Promise<string[]> {
  return (await fs.readdir(directory))
    .filter((filename) => filename.endsWith(".yaml"))
    .sort();
}

async function yamlFilesRecursively(
  directory: string,
  prefix = "",
): Promise<string[]> {
  const entries = await fs.readdir(directory, { withFileTypes: true });
  const files: string[] = [];
  for (const entry of entries) {
    const relativePath = path.join(prefix, entry.name);
    if (entry.isDirectory()) {
      files.push(
        ...await yamlFilesRecursively(
          path.join(directory, entry.name),
          relativePath,
        ),
      );
    } else if (entry.name.endsWith(".yaml")) {
      files.push(relativePath);
    }
  }
  return files.sort();
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
