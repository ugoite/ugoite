import { assert } from "@std/assert/assert";
import { assertEquals } from "@std/assert/equals";
import { parse } from "yaml";
import path from "node:path";

const repoRoot = path.resolve(Deno.cwd());
Deno.test("REQ-OPS-003: legacy requirement directory contains no YAML records", async () => {
  const requirementDir = path.join(repoRoot, "docs/spec/requirements");
  const yamlRecords = await yamlFilesRecursively(requirementDir);
  assertEquals(
    yamlRecords,
    [],
    "requirement records are maintained in Mitase, not the legacy directory",
  );
});

Deno.test("REQ-OPS-004: version statuses agree with their tasks and canonical sources", async () => {
  const versionRoot = path.join(repoRoot, "docs/version");

  for (const filename of await yamlFilesRecursively(versionRoot)) {
    const filePath = path.join(versionRoot, filename);
    const document = parse(
      await Deno.readTextFile(filePath),
    ) as VersionDocument;

    // Changelog entries are historical records, not status-bearing plans.
    if (document.status === undefined) continue;

    assertVersionStatus(document.status, filename);
    const phaseStatuses = [] as VersionStatus[];
    for (const phase of document.phases ?? []) {
      assertVersionStatus(phase.status, `${filename}:${phase.id}`);
      phaseStatuses.push(phase.status);
      assert(
        Array.isArray(phase.tasks),
        `${filename}:${phase.id}: status must be supported by tasks`,
      );
      const tasks = phase.tasks ?? [];
      for (const [index, task] of tasks.entries()) {
        assertEquals(
          typeof task.done,
          "boolean",
          `${filename}:${phase.id}: task ${index} must declare done`,
        );
      }
      assertEquals(
        phase.status,
        statusFromTasks(tasks),
        `${filename}:${phase.id}: stale status`,
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
    assert(
      childStatuses.length > 0,
      `${filename}: status has no children`,
    );
    assertEquals(
      document.status,
      statusFromStatuses(childStatuses),
      `${filename}: stale top-level status`,
    );
  }
});

Deno.test("REQ-API-013: MCP documentation describes the shipped semantic facade", async () => {
  const source = await Deno.readTextFile(
    path.join(repoRoot, "docs/architecture/api/mcp.md"),
  );
  assert(source.includes("POST /mcp"), "MCP docs must describe POST /mcp");
  assert(
    source.includes("ugoite.search"),
    "MCP docs must describe ugoite.search",
  );
  assert(
    source.includes("ugoite://entry/{id}"),
    "MCP docs must describe the entry resource URI",
  );
  assert(
    source.includes("/.well-known/oauth-protected-resource"),
    "MCP docs must describe protected-resource metadata",
  );
  assert(/DPoP/i.test(source), "MCP docs must describe DPoP");
});

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

const versionStatuses = new Set<VersionStatus>([
  "planned",
  "in_progress",
  "completed",
]);

function assertVersionStatus(
  status: string,
  owner: string,
): asserts status is VersionStatus {
  assert(
    versionStatuses.has(status as VersionStatus),
    `${owner}: invalid version status`,
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
  assert(
    milestone.source.length > 0,
    `${owner}:${milestone.id}: missing source`,
  );
  for (const source of milestone.source) {
    await expectPath(
      path.resolve(repoRoot, source),
      `${owner}:${milestone.id}`,
    );
  }

  const yamlSource = milestone.source.find((source) =>
    source.endsWith(".yaml")
  );
  assert(
    yamlSource !== undefined,
    `${owner}:${milestone.id}: missing canonical YAML source`,
  );
  const canonical = parse(
    await Deno.readTextFile(path.resolve(repoRoot, yamlSource as string)),
  ) as VersionDocument;
  assertEquals(
    milestone.status,
    canonical.status,
    `${owner}:${milestone.id}: stale milestone status`,
  );
  assertEquals(
    milestone.phases.map(({ id, status }) => ({ id, status })),
    canonical.phases?.map(({ id, status }) => ({ id, status })),
    `${owner}:${milestone.id}: stale phase status summary`,
  );
}

async function yamlFilesRecursively(
  directory: string,
  prefix = "",
): Promise<string[]> {
  const files: string[] = [];
  for await (const entry of Deno.readDir(directory)) {
    const relativePath = path.join(prefix, entry.name);
    const filePath = path.join(directory, entry.name);
    if (entry.isDirectory) {
      files.push(...await yamlFilesRecursively(filePath, relativePath));
    } else if (entry.isFile && entry.name.endsWith(".yaml")) {
      files.push(relativePath);
    }
  }
  return files.sort();
}

async function expectPath(filePath: string, owner: string): Promise<void> {
  try {
    await Deno.stat(filePath);
  } catch (error) {
    throw new Error(`${owner}: missing ${filePath}`, { cause: error });
  }
}
