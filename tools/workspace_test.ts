import { assertEquals } from "@std/assert/equals";

Deno.test("Phase 1 workspace has one root toolchain and Deno lockfile", async () => {
  const rootMise = await Deno.readTextFile("mise.toml");
  assertEquals(rootMise.includes('deno = "2.8.3"'), true);
  assertEquals(rootMise.includes('rust = "1.94.0"'), true);
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

Deno.test("devcontainer provides an isolated Docker engine for local E2E", async () => {
  const config = JSON.parse(
    await Deno.readTextFile(".devcontainer/devcontainer.json"),
  ) as {
    features?: Record<string, Record<string, unknown>>;
    privileged?: boolean;
  };
  const dockerFeature = config.features
    ?.["ghcr.io/devcontainers/features/docker-in-docker:4"];

  assertEquals(dockerFeature !== undefined, true);
  assertEquals(dockerFeature?.moby, true);
  assertEquals(dockerFeature?.dockerDashComposeVersion, "v2");
  assertEquals(dockerFeature?.installDockerBuildx, true);
  assertEquals(config.privileged, true);

  const developmentGuide = await Deno.readTextFile(
    "docs/guide/develop/index.md",
  );
  assertEquals(developmentGuide.includes("docker info"), true);
  assertEquals(developmentGuide.includes("mise run e2e:smoke"), true);
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

Deno.test("local runtime data uses one ignored repository mount", async () => {
  const gitignore = await Deno.readTextFile(".gitignore");
  const dev = await Deno.readTextFile("tools/dev.ts");
  const sourceCompose = await Deno.readTextFile("docker-compose.yaml");
  const releaseCompose = await Deno.readTextFile("docker-compose.release.yaml");

  assertEquals(gitignore.includes("/data/"), true);
  assertEquals(dev.includes("const devDataRoot = `${repoRoot}data`;"), true);
  assertEquals(dev.includes("?? devDataRoot"), true);
  assertEquals(sourceCompose.includes("UGOITE_DATA_DIR:-./data}:/data"), true);
  assertEquals(releaseCompose.includes("UGOITE_DATA_DIR:-./data}:/data"), true);
  assertEquals(sourceCompose.includes("./spaces:/data/spaces"), false);
  assertEquals(sourceCompose.includes("./node:/data/_ugoite"), false);
  assertEquals(releaseCompose.includes("UGOITE_SPACES_DIR"), false);
  assertEquals(releaseCompose.includes("UGOITE_NODE_DIR"), false);
});

Deno.test("sample-data seeding is exposed through the root mise task", async () => {
  const rootMise = await Deno.readTextFile("mise.toml");
  const taskHeader = "[tasks.seed]";
  const taskStart = rootMise.indexOf(taskHeader);
  assertEquals(taskStart >= 0, true, "root seed task must exist");
  const taskEnd = rootMise.indexOf("\n[tasks", taskStart + taskHeader.length);
  const task = rootMise.slice(taskStart, taskEnd < 0 ? undefined : taskEnd);
  assertEquals(
    task.includes('run = "bash scripts/dev-seed.sh"'),
    true,
    "seed task must invoke the existing helper",
  );
});

Deno.test("dev-seed forwards arguments and protects UUID-backed Spaces", async () => {
  const root = await Deno.makeTempDir({ prefix: "ugoite-seed-forwarding-" });
  const fakeBin = await Deno.makeTempDir({ prefix: "ugoite-seed-fake-bin-" });
  const argsLog = `${fakeBin}/cargo-args`;
  const fakeCargo = `${fakeBin}/cargo`;
  const helperArgs = [
    "--root",
    root,
    "--space-id",
    "forwarded-space",
    "--scenario",
    "lab-qa",
    "--entry-count",
    "7",
    "--seed",
    "42",
  ];
  await Deno.writeTextFile(
    fakeCargo,
    `#!/bin/sh
printf '%s\\n' "$@" > "$UGOITE_FAKE_CARGO_ARGS"
space_dir="\${15}/spaces/019f0000-0000-7000-8000-000000000001"
mkdir -p "$space_dir"
printf '{"slug":"%s","space_uid":"019f0000-0000-7000-8000-000000000001"}' "\${16}" > "$space_dir/meta.json"
printf '{"created":true,"id":"019f0000-0000-7000-8000-000000000001","slug":"%s","scenario":"%s","entry_count":%s}\\n' "\${16}" "$9" "\${11}"
`,
  );
  await Deno.chmod(fakeCargo, 0o755);

  try {
    const env = {
      ...Deno.env.toObject(),
      PATH: `${fakeBin}:${Deno.env.get("PATH") ?? ""}`,
      UGOITE_FAKE_CARGO_ARGS: argsLog,
    };
    const result = await new Deno.Command("bash", {
      args: ["scripts/dev-seed.sh", ...helperArgs],
      env,
      stdout: "piped",
      stderr: "piped",
    }).output();
    assertEquals(result.success, true, new TextDecoder().decode(result.stderr));
    const summary = JSON.parse(new TextDecoder().decode(result.stdout));
    assertEquals(summary.slug, "forwarded-space");
    assertEquals(summary.scenario, "lab-qa");
    assertEquals(summary.entry_count, 7);

    const spaces = [];
    for await (const entry of Deno.readDir(`${root}/spaces`)) {
      if (entry.isDirectory) spaces.push(entry.name);
    }
    assertEquals(spaces, ["019f0000-0000-7000-8000-000000000001"]);
    const meta = JSON.parse(
      await Deno.readTextFile(`${root}/spaces/${spaces[0]}/meta.json`),
    );
    assertEquals(meta.slug, "forwarded-space");
    assertEquals(meta.space_uid, spaces[0]);

    const second = await new Deno.Command("bash", {
      args: ["scripts/dev-seed.sh", ...helperArgs],
      env,
      stdout: "piped",
      stderr: "piped",
    }).output();
    assertEquals(second.success, false);
    assertEquals(
      new TextDecoder().decode(second.stderr).includes(
        "Refusing to overwrite existing local sample space",
      ),
      true,
    );
    assertEquals(
      await Deno.readTextFile(argsLog),
      "run\n-q\n-p\nugoite-cli\n--\nspace\nsample-data\n--scenario\nlab-qa\n--entry-count\n7\n--seed\n42\n--\n" +
        `${root}\nforwarded-space\n`,
    );

    const legacyRoot = await Deno.makeTempDir({
      prefix: "ugoite-seed-legacy-",
    });
    try {
      const legacySpace = `${legacyRoot}/spaces/legacy-space`;
      await Deno.mkdir(legacySpace, { recursive: true });
      await Deno.writeTextFile(
        `${legacySpace}/meta.json`,
        '{"slug":"legacy-space"}',
      );
      const legacy = await new Deno.Command("bash", {
        args: [
          "scripts/dev-seed.sh",
          "--root",
          legacyRoot,
          "--space-id",
          "legacy-space",
        ],
        env,
        stdout: "piped",
        stderr: "piped",
      }).output();
      assertEquals(legacy.success, false);
      assertEquals(
        new TextDecoder().decode(legacy.stderr).includes(
          "Refusing to overwrite existing local sample space",
        ),
        true,
      );
    } finally {
      await Deno.remove(legacyRoot, { recursive: true });
    }

    const legacyDirectoryRoot = await Deno.makeTempDir({
      prefix: "ugoite-seed-legacy-directory-",
    });
    try {
      const legacySpace = `${legacyDirectoryRoot}/spaces/directory-space`;
      await Deno.mkdir(legacySpace, { recursive: true });
      await Deno.writeTextFile(
        `${legacySpace}/meta.json`,
        '{"id":"directory-space","name":"directory-space"}',
      );
      const legacy = await new Deno.Command("bash", {
        args: [
          "scripts/dev-seed.sh",
          "--root",
          legacyDirectoryRoot,
          "--space-id",
          "directory-space",
        ],
        env,
        stdout: "piped",
        stderr: "piped",
      }).output();
      assertEquals(legacy.success, false);
      assertEquals(
        new TextDecoder().decode(legacy.stderr).includes(
          "Refusing to overwrite existing local sample space",
        ),
        true,
      );
    } finally {
      await Deno.remove(legacyDirectoryRoot, { recursive: true });
    }
  } finally {
    await Deno.remove(root, { recursive: true });
    await Deno.remove(fakeBin, { recursive: true });
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
  for (const task of ["test:e2e", "test:e2e:smoke", "test:e2e:asset-owned"]) {
    const header = `[tasks."${task}"]`;
    const start = rootMise.indexOf(header);
    assertEquals(start >= 0, true, `${task} must exist`);
    const end = rootMise.indexOf("\n[tasks", start + header.length);
    const block = rootMise.slice(start, end < 0 ? undefined : end);
    assertEquals(
      block.includes("docker image inspect") &&
        block.includes("UGOITE_IMAGE_TAG:-ugoite:e2e"),
      true,
      `${task} must use the prebuilt E2E image`,
    );
    assertEquals(
      block.includes("E2E_BUILD_IMAGES=false"),
      true,
      `${task} must not rebuild the E2E image`,
    );
  }
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
  const rootDeno = await Deno.readTextFile("deno.json");
  assertEquals(
    rootDeno.includes(
      '"e2e:asset-owned": "bash e2e/scripts/run-e2e-parity.sh asset-owned"',
    ),
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
