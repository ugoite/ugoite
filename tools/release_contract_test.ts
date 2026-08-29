import { assertEquals } from "@std/assert/equals";

const root = new URL("../", import.meta.url);
const expectedVersion = "0.1.0";

async function readText(path: string): Promise<string> {
  return await Deno.readTextFile(new URL(path, root));
}

function sourceVersion(text: string, pattern: RegExp, label: string): string {
  const match = text.match(pattern);
  if (!match) throw new Error(`${label} is missing`);
  return match[1].replace(/\s+#.*$/, "").replace(/^"|"$/g, "").trim();
}

Deno.test("REQ-OPS-009: first release metadata has one explicit version", async () => {
  const cargo = await readText("Cargo.toml");
  const packageJson = JSON.parse(
    await readText("packages/ugoite/package.json"),
  ) as { version?: string };
  const chart = await readText("charts/ugoite/Chart.yaml");
  const values = await readText("charts/ugoite/values.yaml");
  const versionFile = (await readText("version.txt")).trim();
  const manifest = JSON.parse(
    await readText(".release-please-manifest.json"),
  ) as { "."?: string };

  assertEquals(
    [
      sourceVersion(
        cargo,
        /\[workspace\.package\][\s\S]*?\nversion\s*=\s*"([^"]+)"/,
        "workspace version",
      ),
      packageJson.version,
      sourceVersion(chart, /\nversion:\s*([^\n]+)/, "Helm chart version"),
      sourceVersion(chart, /\nappVersion:\s*([^\n]+)/, "Helm appVersion"),
      sourceVersion(values, /\n\x20\x20tag:\s*([^\n]+)/, "Helm image tag"),
      versionFile,
      manifest["."],
    ],
    Array(7).fill(expectedVersion),
  );
  assertEquals(
    /^(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)$/.test(versionFile),
    true,
  );
});

Deno.test("REQ-OPS-009: the root manifest is the only release manifest", async () => {
  const manifestPaths: string[] = [];
  for await (const entry of Deno.readDir(root)) {
    if (entry.isFile && entry.name === ".release-please-manifest.json") {
      manifestPaths.push(entry.name);
    }
    if (entry.isDirectory && entry.name === ".github") {
      for await (const nested of Deno.readDir(new URL(".github/", root))) {
        if (nested.name.endsWith("release-please-manifest.json")) {
          manifestPaths.push(`.github/${nested.name}`);
        }
      }
    }
  }
  assertEquals(manifestPaths, [".release-please-manifest.json"]);
});

Deno.test("REQ-OPS-009: release promotion is an exact-tag manual workflow", async () => {
  const workflow = await readText(".github/workflows/release-publish.yml");
  const trigger = workflow.slice(0, workflow.indexOf("permissions: {}"));

  assertEquals(trigger.includes("workflow_dispatch:"), true);
  assertEquals(trigger.includes("push:"), false);
  assertEquals(workflow.includes("release_tag:"), true);
  assertEquals(workflow.includes("ref: ${{ inputs.release_tag }}"), true);
  assertEquals(
    workflow.includes(
      '["gh", "release", "view", candidate_tag, "--json", "tagName,targetCommitish,isPrerelease"]',
    ),
    true,
  );
  assertEquals(
    workflow.includes('["git", "rev-list", "-n", "1", candidate_tag]'),
    true,
  );
  assertEquals(
    workflow.includes('["mise", "run", "validate:release"]'),
    true,
  );
  assertEquals(workflow.includes("release_sha={tag_sha}"), true);
  assertEquals(
    workflow.includes("ref: ${{ needs.prepare.outputs.release_sha }}"),
    true,
  );
  assertEquals(
    workflow.includes("bash scripts/verify-release-container-quickstart.sh"),
    true,
  );
  assertEquals(
    workflow.includes("bash scripts/verify-release-cli-quickstart.sh"),
    true,
  );
});

Deno.test("REQ-OPS-009: release publication does not depend on PR creation policy", async () => {
  const workflow = await readText(".github/workflows/release-publish.yml");

  assertEquals(workflow.includes("release-please-action"), false);
  assertEquals(workflow.includes("pull-requests: write"), false);
  assertEquals(workflow.includes("issues: write"), false);
  assertEquals(workflow.includes("permissions: {}"), true);
});
