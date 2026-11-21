import { assertEquals } from "@std/assert/equals";
import { classifyPaths, makeImpactReport } from "./ci-impact.ts";

const baseSha = "a".repeat(40);
const headSha = "b".repeat(40);

Deno.test("CI impact classifier maps the declared top-level categories", () => {
  assertEquals(
    classifyPaths(["docs/guide.md", "docsite/src/content/index.mdx"]),
    {
      docs: true,
      frontend: false,
      rust: false,
      global: false,
      cp1Acceptance: false,
    },
  );
  assertEquals(classifyPaths(["frontend/src/routes.tsx", "shared/query.ts"]), {
    docs: false,
    frontend: true,
    rust: false,
    global: false,
    cp1Acceptance: false,
  });
  assertEquals(classifyPaths(["crates/ugoite-core/src/lib.rs", "Cargo.toml"]), {
    docs: false,
    frontend: false,
    rust: true,
    global: false,
    cp1Acceptance: false,
  });
  assertEquals(classifyPaths([".github/workflows/ci.yml"]), {
    docs: false,
    frontend: false,
    rust: false,
    global: true,
    cp1Acceptance: false,
  });
});

Deno.test("CP1 query and export paths select the acceptance lane", () => {
  const report = makeImpactReport({
    event: "pull_request",
    baseSha,
    headSha,
    paths: ["frontend/src/lib/entry-query.ts"],
  });
  assertEquals(report.categories.cp1Acceptance, true);
  assertEquals(report.candidateLanes, ["artifacts", "cp1-acceptance", "web"]);

  const queryE2e = makeImpactReport({
    event: "pull_request",
    baseSha,
    headSha,
    paths: ["e2e/query-surfaces-measurement.test.ts"],
  });
  assertEquals(queryE2e.categories.cp1Acceptance, true);
  assertEquals(queryE2e.candidateLanes.includes("artifacts"), true);
  assertEquals(queryE2e.candidateLanes.includes("cp1-acceptance"), true);

  const docsOnly = makeImpactReport({
    event: "pull_request",
    baseSha,
    headSha,
    paths: ["docs/use/search.mdx"],
  });
  assertEquals(docsOnly.candidateLanes.includes("cp1-acceptance"), false);
});

Deno.test("CP1 acceptance planning always includes the artifact build", () => {
  // The hosted fixtures/query/export trio consumes the release image and CLI,
  // so the single cp1-acceptance flag must never plan without artifacts.
  const cases = [
    makeImpactReport({
      event: "pull_request",
      baseSha,
      headSha,
      paths: ["e2e/query-surfaces-measurement.test.ts"],
    }),
    makeImpactReport({
      event: "pull_request",
      baseSha,
      headSha,
      paths: ["crates/ugoite-server/src/lib.rs"],
    }),
    makeImpactReport({ event: "merge_group" }),
    makeImpactReport({
      event: "push",
      baseSha,
      headSha,
      paths: ["docs/guide.md"],
    }),
  ];
  for (const report of cases) {
    if (report.candidateLanes.includes("cp1-acceptance")) {
      assertEquals(report.candidateLanes.includes("artifacts"), true);
    }
  }
  assertEquals(
    cases.every((report) => report.candidateLanes.includes("artifacts")),
    true,
  );
});

Deno.test("docs and frontend diffs select only their required lanes", () => {
  const report = makeImpactReport({
    event: "pull_request",
    baseSha,
    headSha,
    paths: ["docs/guide.md", "frontend/src/page.tsx"],
  });
  assertEquals(report.scope, "scoped");
  assertEquals(report.categories, {
    docs: true,
    frontend: true,
    rust: false,
    global: false,
    cp1Acceptance: false,
  });
  assertEquals(report.candidateLanes, ["artifacts", "docsite-nav", "web"]);
  assertEquals(report.executionMode, "selective-pr");
  assertEquals(report.jobsSkipped, true);

  const docsOnly = makeImpactReport({
    event: "pull_request",
    baseSha,
    headSha,
    paths: ["docs/guide.md"],
  });
  assertEquals(docsOnly.candidateLanes, ["docsite-nav", "web"]);

  const frontendOnly = makeImpactReport({
    event: "pull_request",
    baseSha,
    headSha,
    paths: ["frontend/src/page.tsx"],
  });
  assertEquals(frontendOnly.candidateLanes, ["artifacts", "web"]);
});

Deno.test("main pushes retain every lane and merge groups remain unconditional", () => {
  const push = makeImpactReport({
    event: "push",
    baseSha,
    headSha,
    paths: ["docs/guide.md"],
  });
  assertEquals(push.scope, "all");
  assertEquals(push.candidateLanes, [
    "artifacts",
    "cp1-acceptance",
    "docsite-nav",
    "rust-check",
    "rust-test",
    "web",
  ]);
  assertEquals(push.jobsSkipped, false);
  assertEquals(push.runOlderCliPreservation, true);
  const mergeGroup = makeImpactReport({ event: "merge_group" });
  assertEquals(mergeGroup.candidateLanes, push.candidateLanes);
  assertEquals(mergeGroup.runOlderCliPreservation, true);
  assertEquals(
    makeImpactReport({ event: "local" }).runOlderCliPreservation,
    true,
    "local execution must run the preservation check by default",
  );
  assertEquals(
    makeImpactReport({ event: "workflow_dispatch" }).runOlderCliPreservation,
    true,
    "manual release workflows must not skip the preservation check",
  );
});

Deno.test("older-CLI preservation skips only clearly unrelated scoped PRs", () => {
  for (
    const path of [
      "docs/use/search.mdx",
      "frontend/src/routes/settings.tsx",
      "crates/ugoite-identity/src/node_identity.rs",
      "crates/ugoite-server/src/lib.rs",
      "crates/ugoite-cli/src/commands/auth.rs",
    ]
  ) {
    const report = makeImpactReport({
      event: "pull_request",
      baseSha,
      headSha,
      paths: [path],
    });
    assertEquals(report.scope, "scoped", path);
    assertEquals(report.runOlderCliPreservation, false, path);
  }

  for (
    const path of [
      "Cargo.toml",
      "Cargo.lock",
      "crates/ugoite-core/src/composition.rs",
      "crates/ugoite-domain/src/composition/yaml.rs",
      "crates/ugoite-iceberg/src/service.rs",
      "crates/ugoite-iceberg/tests/test_older_cli_knowledge_preservation.rs",
      "crates/ugoite-storage/src/lib.rs",
    ]
  ) {
    const report = makeImpactReport({
      event: "pull_request",
      baseSha,
      headSha,
      paths: [path],
    });
    assertEquals(report.scope, "scoped", path);
    assertEquals(report.runOlderCliPreservation, true, path);
  }

  assertEquals(
    makeImpactReport({ event: "pull_request", baseSha, headSha })
      .runOlderCliPreservation,
    true,
    "unknown paths must fail closed",
  );
  assertEquals(
    makeImpactReport({
      event: "pull_request",
      baseSha,
      headSha,
      diffError: "git failed",
    }).runOlderCliPreservation,
    true,
    "uncertain diffs must fail closed",
  );
});

Deno.test("merge groups, uncertain inputs, large diffs, and global paths plan all", () => {
  const cases = [
    makeImpactReport({ event: "merge_group" }),
    makeImpactReport({ event: "push", baseSha: "0".repeat(40), headSha }),
    makeImpactReport({
      event: "push",
      baseSha,
      headSha,
      diffError: "git failed",
    }),
    makeImpactReport({
      event: "push",
      baseSha,
      headSha,
      paths: Array.from({ length: 101 }, (_, index) => `docs/${index}.md`),
    }),
    makeImpactReport({
      event: "pull_request",
      baseSha,
      headSha,
      paths: ["mise.toml"],
    }),
    makeImpactReport({
      event: "pull_request",
      baseSha,
      headSha,
      paths: ["new-surface/file.rs"],
    }),
  ];
  for (const report of cases) {
    assertEquals(report.scope, "all");
    assertEquals(report.candidateLanes, [
      "artifacts",
      "cp1-acceptance",
      "docsite-nav",
      "rust-check",
      "rust-test",
      "web",
    ]);
    assertEquals(report.jobsSkipped, false);
  }
});

Deno.test("dependency and toolchain paths retain their required CI lanes", () => {
  const fullLanes = [
    "artifacts",
    "cp1-acceptance",
    "docsite-nav",
    "rust-check",
    "rust-test",
    "web",
  ];
  const onlyGlobal = {
    docs: false,
    frontend: false,
    rust: false,
    global: true,
    cp1Acceptance: false,
  };
  const cases = [
    {
      path: "deno.json",
      categories: onlyGlobal,
      scope: "all",
      lanes: fullLanes,
      jobsSkipped: false,
    },
    {
      path: "deno.lock",
      categories: onlyGlobal,
      scope: "all",
      lanes: fullLanes,
      jobsSkipped: false,
    },
    {
      path: "frontend/deno.json",
      categories: {
        docs: false,
        frontend: true,
        rust: false,
        global: false,
        cp1Acceptance: false,
      },
      scope: "scoped",
      lanes: ["artifacts", "web"],
      jobsSkipped: true,
    },
    {
      path: "docsite/deno.json",
      categories: {
        docs: true,
        frontend: false,
        rust: false,
        global: false,
        cp1Acceptance: false,
      },
      scope: "scoped",
      lanes: ["docsite-nav", "web"],
      jobsSkipped: true,
    },
    // These manifests currently use the conservative unclassified-path fallback.
    {
      path: "tools/deno.json",
      categories: onlyGlobal,
      scope: "all",
      lanes: fullLanes,
      jobsSkipped: false,
    },
    {
      path: "e2e/deno.json",
      categories: onlyGlobal,
      scope: "all",
      lanes: fullLanes,
      jobsSkipped: false,
    },
    {
      path: "mise.toml",
      categories: onlyGlobal,
      scope: "all",
      lanes: fullLanes,
      jobsSkipped: false,
    },
    {
      path: "Cargo.lock",
      categories: {
        docs: false,
        frontend: false,
        rust: true,
        global: false,
        cp1Acceptance: false,
      },
      scope: "scoped",
      lanes: ["artifacts", "rust-check", "rust-test", "web"],
      jobsSkipped: true,
    },
  ] as const;

  for (const fixture of cases) {
    const report = makeImpactReport({
      event: "pull_request",
      baseSha,
      headSha,
      paths: [fixture.path],
    });
    assertEquals(report.categories, fixture.categories, fixture.path);
    assertEquals(report.scope, fixture.scope, fixture.path);
    assertEquals(report.candidateLanes, fixture.lanes, fixture.path);
    assertEquals(report.jobsSkipped, fixture.jobsSkipped, fixture.path);
    if (fixture.path === "deno.lock") {
      assertEquals(
        report.candidateLanes,
        fullLanes,
        "shared Deno lockfile changes must keep every CI lane",
      );
    }
  }
});
