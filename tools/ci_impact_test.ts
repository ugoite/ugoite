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
    },
  );
  assertEquals(classifyPaths(["frontend/src/routes.tsx", "shared/query.ts"]), {
    docs: false,
    frontend: true,
    rust: false,
    global: false,
  });
  assertEquals(classifyPaths(["crates/ugoite-core/src/lib.rs", "Cargo.toml"]), {
    docs: false,
    frontend: false,
    rust: true,
    global: false,
  });
  assertEquals(classifyPaths([".github/workflows/ci.yml"]), {
    docs: false,
    frontend: false,
    rust: false,
    global: true,
  });
});

Deno.test("ordinary docs and frontend diffs produce a shadow-only scoped report", () => {
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
  });
  assertEquals(report.candidateLanes, ["artifacts", "docsite-nav", "web"]);
  assertEquals(report.executionMode, "shadow-only");
  assertEquals(report.jobsSkipped, false);
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
      "docsite-nav",
      "rust-check",
      "rust-test",
      "web",
    ]);
    assertEquals(report.jobsSkipped, false);
  }
});
