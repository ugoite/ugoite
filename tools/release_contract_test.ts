import { assertEquals } from "@std/assert/equals";

const root = new URL("../", import.meta.url);
const firstReleaseBoundary = "2cacdd060ac38a60a286f213ea6f94c4bb8dd563";
const expectedVersion = "0.1.0";

async function readText(path: string): Promise<string> {
  return await Deno.readTextFile(new URL(path, root));
}

function sourceVersion(text: string, pattern: RegExp, label: string): string {
  const match = text.match(pattern);
  if (!match) throw new Error(`${label} is missing`);
  return match[1].replace(/\s+#.*$/, "").replace(/^"|"$/g, "");
}

function applyGenericVersionUpdate(text: string, version: string): string {
  return text.split("\n").map((line) =>
    line.includes("x-release-please-version")
      ? line.replace(/\d+\.\d+\.\d+(?:-[\w.]+)?/, version)
      : line
  ).join("\n");
}

function jobPermissionLines(workflow: string, job: string): string[] {
  const start = workflow.indexOf(`  ${job}:\n`);
  if (start < 0) throw new Error(`release job ${job} is missing`);
  const bodyStart = start + `  ${job}:\n`.length;
  const nextJob = workflow.slice(bodyStart).search(
    /\n\x20{2}[a-zA-Z0-9_-]+:\n/,
  );
  const body = workflow.slice(
    bodyStart,
    nextJob < 0 ? workflow.length : bodyStart + nextJob,
  );
  const permissions = body.match(
    /\n\x20{4}permissions:\n((?:\x20{6}[a-z-]+: (?:read|write)\n?)+)/,
  )?.[1] ?? "";
  return [...permissions.matchAll(/^\x20{6}([a-z-]+): (read|write)$/gm)]
    .map((match) => `${match[1]}: ${match[2]}`)
    .sort();
}

Deno.test("first release bootstrap is explicit and single-sourced", async () => {
  const config = JSON.parse(
    await Deno.readTextFile(
      new URL(".github/release-please-config.json", root),
    ),
  ) as {
    [key: string]: unknown;
    packages: Record<string, Record<string, unknown>>;
  };

  assertEquals(config["bootstrap-sha"], firstReleaseBoundary);
  assertEquals(config.packages["."]["release-as"], "0.1.0");
  assertEquals(config.packages["."]["version-file"], "version.txt");
  assertEquals(config.packages["."]["extra-files"], [
    { type: "generic", path: "Cargo.toml" },
    "packages/ugoite/package.json",
    { type: "generic", path: "charts/ugoite/Chart.yaml" },
    { type: "generic", path: "charts/ugoite/values.yaml" },
  ]);
  if (!/^[0-9a-f]{40}$/.test(String(config["bootstrap-sha"]))) {
    throw new Error("bootstrap-sha must be a full commit SHA");
  }

  const manifestPaths: string[] = [];
  for await (const entry of Deno.readDir(new URL(".", root))) {
    if (entry.isDirectory && entry.name === ".github") {
      for await (const nested of Deno.readDir(new URL(".github/", root))) {
        if (nested.name.endsWith("release-please-manifest.json")) {
          manifestPaths.push(`.github/${nested.name}`);
        }
      }
    }
    if (entry.isFile && entry.name === ".release-please-manifest.json") {
      manifestPaths.push(entry.name);
    }
  }
  assertEquals(manifestPaths, [".release-please-manifest.json"]);
});

Deno.test("release sources stay synchronized at the first release", async () => {
  const cargo = await readText("Cargo.toml");
  const packageJson = JSON.parse(
    await readText("packages/ugoite/package.json"),
  ) as {
    version?: string;
  };
  const chart = await readText("charts/ugoite/Chart.yaml");
  const values = await readText("charts/ugoite/values.yaml");
  const versionFile = await readText("version.txt");
  const manifest = JSON.parse(
    await readText(".release-please-manifest.json"),
  ) as {
    "."?: string;
  };

  const versions = [
    sourceVersion(
      cargo,
      /\[workspace\.package\][\s\S]*?\nversion\s*=\s*"([^"]+)"/,
      "workspace version",
    ),
    packageJson.version,
    sourceVersion(chart, /\nversion:\s*([^\n]+)/, "Helm chart version"),
    sourceVersion(chart, /\nappVersion:\s*([^\n]+)/, "Helm appVersion"),
    sourceVersion(values, /\n\x20\x20tag:\s*([^\n]+)/, "Helm image tag"),
    versionFile.trim(),
    manifest["."],
  ];

  assertEquals(versions, Array(versions.length).fill(expectedVersion));
  assertEquals(
    [
      cargo.includes("x-release-please-version"),
      chart.match(/^(?:version|appVersion):.*x-release-please-version$/m) !==
        null,
      values.includes("x-release-please-version"),
    ],
    [true, true, true],
  );

  const promotedCargo = applyGenericVersionUpdate(cargo, "0.1.1");
  const promotedChart = applyGenericVersionUpdate(chart, "0.1.1");
  const promotedValues = applyGenericVersionUpdate(values, "0.1.1");
  const promotedVersionFile = versionFile.replace(
    /^\d+\.\d+\.\d+(?:-[\w.]+)?/,
    "0.1.1",
  );
  assertEquals(
    [
      sourceVersion(
        promotedCargo,
        /\[workspace\.package\][\s\S]*?\nversion\s*=\s*"([^"]+)"/,
        "promoted workspace version",
      ),
      sourceVersion(
        promotedChart,
        /\nversion:\s*([^\n]+)/,
        "promoted Helm chart version",
      ),
      sourceVersion(
        promotedChart,
        /\nappVersion:\s*([^\n]+)/,
        "promoted Helm appVersion",
      ),
      sourceVersion(
        promotedValues,
        /\n\x20\x20tag:\s*([^\n]+)/,
        "promoted Helm image tag",
      ),
      promotedVersionFile.trim(),
    ],
    ["0.1.1", "0.1.1", "0.1.1", "0.1.1", "0.1.1"],
  );
});

Deno.test("release workflow keeps planner permissions and manual recovery", async () => {
  const workflow = await Deno.readTextFile(
    new URL(".github/workflows/release-publish.yml", root),
  );

  if (!workflow.includes("permissions: {}")) {
    throw new Error(
      "release workflow must keep a deny-by-default permission boundary",
    );
  }
  const expectedPermissions: Record<string, string[]> = {
    "release-please": [
      "contents: write",
      "issues: write",
      "pull-requests: write",
    ],
    prepare: ["contents: read"],
    "build-cli": ["attestations: write", "contents: read", "id-token: write"],
    "build-npm-helm": [
      "attestations: write",
      "contents: read",
      "id-token: write",
    ],
    "publish-image": ["contents: read", "id-token: write", "packages: write"],
    "publish-release-assets": ["contents: write", "packages: write"],
    "verify-published-quickstarts": ["contents: read", "packages: read"],
  };
  const jobsSection = workflow.slice(workflow.indexOf("jobs:\n"));
  const jobNames = [...jobsSection.matchAll(/^\x20{2}([a-zA-Z0-9_-]+):$/gm)]
    .map((match) => match[1])
    .sort();
  assertEquals(jobNames, Object.keys(expectedPermissions).sort());
  for (const [job, permissions] of Object.entries(expectedPermissions)) {
    assertEquals(jobPermissionLines(workflow, job), permissions.sort());
  }
  if (!workflow.includes("release_tag:")) {
    throw new Error(
      "manual existing-tag republish input must remain available",
    );
  }
  if (!workflow.includes("github.event_name == 'workflow_dispatch'")) {
    throw new Error("manual dispatch must remain an explicit prepare path");
  }
  for (
    const subject of [
      "target/artifacts/cli/*.tar.gz",
      "target/artifacts/npm/*.tgz",
      "target/artifacts/helm/*.tgz",
    ]
  ) {
    if (!workflow.includes(`subject-path: ${subject}`)) {
      throw new Error(`artifact provenance is missing for ${subject}`);
    }
  }
  if (!workflow.includes("provenance: true")) {
    throw new Error("published image provenance must remain enabled");
  }
});
