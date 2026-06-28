import { assertEquals } from "@std/assert/equals";

Deno.test("Phase 1 workspace has one root toolchain and Deno lockfile", async () => {
  const rootMise = await Deno.readTextFile("mise.toml");
  assertEquals(rootMise.includes('deno = "2.8.3"'), true);
  assertEquals(rootMise.includes('rust = "1.93.0"'), true);
  assertEquals(rootMise.includes("python ="), false);
  assertEquals(rootMise.includes("bun ="), false);
  assertEquals(rootMise.includes("node ="), false);
  assertEquals(rootMise.includes("uv ="), false);

  const expectedFiles = [
    "deno.lock",
    "frontend/deno.json",
    "docsite/deno.json",
    "e2e/deno.json",
    "tools/deno.json",
  ];
  for (const file of expectedFiles) {
    assertEquals((await Deno.stat(file)).isFile, true, `${file} must exist`);
  }
});

Deno.test("Phase 1 removes legacy root and subdirectory tool entrypoints", async () => {
  const removedFiles = [
    "package.json",
    "package-lock.json",
    ".pre-commit-config.yaml",
    "backend/mise.toml",
    "frontend/mise.toml",
    "docsite/mise.toml",
    "e2e/mise.toml",
    "ugoite-core/mise.toml",
    "ugoite-cli/mise.toml",
    "ugoite-minimum/mise.toml",
    "frontend/biome.json",
    "docsite/biome.json",
  ];

  for (const file of removedFiles) {
    try {
      await Deno.stat(file);
      throw new Error(`${file} must be removed`);
    } catch (error) {
      if (!(error instanceof Deno.errors.NotFound)) {
        throw error;
      }
    }
  }
});

Deno.test("Phase 4 removes Python from tracked source and contributor tooling", async () => {
  const command = new Deno.Command("git", {
    args: ["ls-files", "*.py", "**/uv.lock"],
    stdout: "piped",
  });
  const output = await command.output();
  assertEquals(new TextDecoder().decode(output.stdout).trim(), "");

  for (
    const path of [
      "mise.toml",
      ".devcontainer/devcontainer.json",
      ".github/workflows/ci.yml",
      ".github/workflows/codeql.yml",
    ]
  ) {
    const contents = await Deno.readTextFile(path);
    assertEquals(
      /\b(?:python|python3|uv|pytest)\b/i.test(contents),
      false,
      path,
    );
  }
});

Deno.test("Phase 5 uses Deno metadata as the workspace source of truth", async () => {
  for (
    const path of [
      "frontend/package.json",
      "docsite/package.json",
      "e2e/package.json",
      "frontend/bun.lock",
      "docsite/bun.lock",
      "e2e/package-lock.json",
    ]
  ) {
    try {
      await Deno.stat(path);
      throw new Error(`${path} must be removed`);
    } catch (error) {
      if (!(error instanceof Deno.errors.NotFound)) {
        throw error;
      }
    }
  }
});

Deno.test("release path does not reference removed runtimes", async () => {
  const forbidden = [
    /backend\//,
    /ugoite-minimum/,
    /uv run/,
    /uvicorn/,
    /bun run/,
    /bun install/,
    /frontend\/Dockerfile/,
    /frontend_node_modules/,
    /BUN_TEST_TIMEOUT_MS/,
  ];
  const paths = [
    "Dockerfile",
    "docker-compose.yaml",
    "docker-compose.e2e.yml",
    "docker-compose.release.yaml",
    "e2e/scripts/run-e2e.sh",
    "e2e/scripts/run-e2e-compose.sh",
    ".devcontainer/devcontainer.json",
    ".vscode/settings.json",
    ".github/dependabot.yml",
  ];

  for (const path of paths) {
    const contents = await Deno.readTextFile(path);
    for (const pattern of forbidden) {
      assertEquals(pattern.test(contents), false, `${path}: ${pattern}`);
    }
  }
});

Deno.test("CI image and E2E tasks preserve the build-once contract", async () => {
  const rootMise = await Deno.readTextFile("mise.toml");
  const workflow = await Deno.readTextFile(".github/workflows/ci.yml");

  assertEquals(
    workflow.includes('UGOITE_IMAGE_PREBUILT: "true"'),
    true,
  );
  assertEquals(
    workflow.includes("cancel-in-progress: true"),
    true,
  );
  assertEquals(
    rootMise.includes(
      '[tasks."test:e2e"]\ndepends = ["build:image"]',
    ),
    false,
  );
  assertEquals(
    rootMise.includes(
      '[tasks."test:e2e:smoke"]\ndepends = ["build:image"]',
    ),
    false,
  );
  assertEquals(
    rootMise.match(/docker image inspect/g)?.length,
    2,
  );
  assertEquals(
    rootMise.includes(
      'env = { DOCSITE_ORIGIN = "http://localhost:4321", DOCSITE_BASE = "/" }',
    ),
    false,
  );
  assertEquals(
    rootMise.includes("${DOCSITE_ORIGIN:-http://localhost:4321}"),
    true,
  );
});

Deno.test("Pages promotion consumes trusted artifacts without rebuilding", async () => {
  const workflow = await Deno.readTextFile(
    ".github/workflows/docsite-pages.yml",
  );

  for (
    const forbidden of [
      "actions/checkout@",
      "pull_request_target:",
      "mise run",
      "deno task",
      "docsite:build",
    ]
  ) {
    assertEquals(workflow.includes(forbidden), false, forbidden);
  }
  for (
    const required of [
      "workflow_run:",
      "workflow_dispatch:",
      "permissions: {}",
      "actions: read",
      "pages: write",
      "id-token: write",
      "environment:",
      "name: github-pages",
      "GITHUB_TOKEN: ${{ github.token }}",
      'fail("workflow_dispatch requires GITHUB_TOKEN")',
    ]
  ) {
    assertEquals(workflow.includes(required), true, required);
  }
});
