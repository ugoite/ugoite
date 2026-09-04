import { assertEquals } from "@std/assert/equals";
import { validateKnowledgeCompatibilityReview } from "./knowledge_compatibility.ts";

const contract = await Deno.readTextFile(
  "docs/spec/versions/v0.1-knowledge-compatibility.md",
);
const template = await Deno.readTextFile(".github/pull_request_template.md");
const validator = await Deno.readTextFile("tools/create_pr.ts");
const compatibilityValidator = await Deno.readTextFile(
  "tools/knowledge_compatibility.ts",
);
const workflow = await Deno.readTextFile(
  ".github/workflows/pr-require-close-issue.yml",
);
const ciWorkflow = await Deno.readTextFile(".github/workflows/ci.yml");
const codeqlWorkflow = await Deno.readTextFile(".github/workflows/codeql.yml");
const requiredStatusChecks = JSON.parse(
  await Deno.readTextFile(".github/required-status-checks.json"),
) as {
  required_status_checks?: Array<{
    context?: string;
    workflow?: string;
    job_id?: string;
    events?: string[];
  }>;
};

function requireText(source: string, expected: string, owner: string): void {
  assertEquals(
    source.includes(expected),
    true,
    `${owner} must contain ${expected}`,
  );
}

// REQ-OPS-043
Deno.test("Knowledge Compatibility Review is a checked PR gate", () => {
  requireText(template, "Knowledge Compatibility Review", "PR template");
  requireText(
    template,
    "No effect on the v0.1 Knowledge semantic contract",
    "PR template",
  );
  requireText(template, "Preserving implementation change", "PR template");
  requireText(template, "Breaking semantic change", "PR template");
  requireText(validator, "knowledge_compatibility.ts", "PR validator");
  requireText(
    compatibilityValidator,
    "select exactly one valid classification",
    "canonical validator",
  );
  requireText(workflow, "actions/checkout@", "PR validator");
  requireText(
    workflow,
    "ref: ${{ github.event.pull_request.base.sha || github.event.merge_group.base_sha }}",
    "PR validator",
  );
  requireText(workflow, "tools/knowledge_compatibility.ts", "PR validator");
  requireText(
    workflow,
    'process.versions.node.startsWith("24.")',
    "PR validator",
  );
  requireText(workflow, "pathToFileURL", "PR validator");
  requireText(workflow, 'await import("node:url")', "PR validator");
  requireText(workflow, "await import(", "PR validator");
  requireText(
    workflow,
    "validateKnowledgeCompatibilityReview(body)",
    "PR validator",
  );
  assertEquals(
    workflow.includes("readFileSync") ||
      workflow.includes("stripTypeScriptTypes") ||
      workflow.includes("data:text/javascript"),
    false,
    "workflow must use native Node TypeScript loading",
  );
  assertEquals(
    workflow.includes("knowledge_compatibility_node_fixture.ts"),
    false,
    "privileged workflow must not execute PR-added fixture code",
  );
  assertEquals(
    workflow.includes("No effect on the v0\\.1 Knowledge semantic contract"),
    false,
    "workflow must not duplicate classification regexes",
  );
  assertEquals(
    workflow.includes("checkedClassifications"),
    false,
    "workflow must not implement classification semantics",
  );
  const requiredStatus = requiredStatusChecks.required_status_checks?.find(
    (check) => check.context === "require-close-issue-link",
  );
  assertEquals(
    requiredStatus?.workflow,
    ".github/workflows/pr-require-close-issue.yml",
  );
  assertEquals(requiredStatus?.job_id, "require-close-issue-link");
  assertEquals(requiredStatus?.events, ["pull_request", "merge_group"]);
  requireText(workflow, "merge_group:", "PR validator");
  requireText(workflow, "github.rest.pulls.get", "PR validator");
  requireText(workflow, "pr-(\\d+)", "PR validator");
  requireText(workflow, "context.ref", "PR validator");
  requireText(
    workflow,
    "let pullRequestAuthor = context.payload.pull_request?.user?.login;",
    "PR validator",
  );
  requireText(workflow, "response.data.user?.login", "PR validator");
  requireText(
    workflow,
    'pullRequestAuthor === "dependabot[bot]"',
    "PR validator",
  );
  requireText(
    workflow,
    "skipping the human PR template gate",
    "PR validator",
  );
  requireText(
    contract,
    "Every pull request that can affect Space ownership",
    "compatibility contract",
  );
  requireText(
    contract,
    "breaking semantic change",
    "compatibility contract",
  );
  requireText(
    contract,
    "template placeholder",
    "compatibility contract",
  );
  const webJobStart = ciWorkflow.indexOf("  web:\n");
  const artifactsJobStart = ciWorkflow.indexOf("  artifacts:\n", webJobStart);
  assertEquals(webJobStart >= 0, true, "CI web job must exist");
  const webJob = ciWorkflow.slice(
    webJobStart,
    artifactsJobStart >= 0 ? artifactsJobStart : undefined,
  );
  requireText(webJob, "actions/setup-node@", "CI web job");
  requireText(webJob, "node-version: 24", "CI web job");
  requireText(
    codeqlWorkflow,
    "upload: ${{ github.event_name == 'merge_group' && 'never' || 'always' }}",
    "CodeQL workflow",
  );
});

const compatibilityReviewCases = [
  {
    name: "no-effect classification",
    valid: true,
    review: "- [x] No effect on the v0.1 Knowledge semantic contract.",
  },
  {
    name: "preserving classification with evidence",
    valid: true,
    review: [
      "- [x] Preserving implementation change; the canonical fixture and focused tests remain passing.",
      "Evidence: canonical fixture passes.",
    ].join("\n"),
  },
  {
    name: "breaking classification with decision",
    valid: true,
    review: [
      "- [x] Breaking semantic change; an explicit versioned contract or migration/re-encoding decision is documented.",
      "Decision: versioned re-encoding approved.",
    ].join("\n"),
  },
  {
    name: "no classification",
    valid: false,
    review: "- [ ] No effect on the v0.1 Knowledge semantic contract.",
  },
  {
    name: "two classifications",
    valid: false,
    review: [
      "- [x] No effect on the v0.1 Knowledge semantic contract.",
      "- [x] Preserving implementation change; the canonical fixture and focused tests remain passing.",
      "Evidence: canonical fixture passes.",
    ].join("\n"),
  },
  {
    name: "preserving classification without evidence",
    valid: false,
    review:
      "- [x] Preserving implementation change; the canonical fixture and focused tests remain passing.",
  },
  {
    name: "preserving classification with empty evidence",
    valid: false,
    review: [
      "- [x] Preserving implementation change; the canonical fixture and focused tests remain passing.",
      "Evidence:",
    ].join("\n"),
  },
  {
    name: "preserving classification with untouched placeholder",
    valid: false,
    review: [
      "- [x] Preserving implementation change; the canonical fixture and focused tests remain passing.",
      "Evidence: <required for preserving changes>",
    ].join("\n"),
  },
  {
    name: "breaking classification without decision",
    valid: false,
    review:
      "- [x] Breaking semantic change; an explicit versioned contract or migration/re-encoding decision is documented.",
  },
  {
    name: "breaking classification with empty decision",
    valid: false,
    review: [
      "- [x] Breaking semantic change; an explicit versioned contract or migration/re-encoding decision is documented.",
      "Decision:",
    ].join("\n"),
  },
  {
    name: "breaking classification with untouched placeholder",
    valid: false,
    review: [
      "- [x] Breaking semantic change; an explicit versioned contract or migration/re-encoding decision is documented.",
      "Decision: <required for breaking changes>",
    ].join("\n"),
  },
];

Deno.test("Knowledge Compatibility Review uses one canonical validation matrix", () => {
  for (const testCase of compatibilityReviewCases) {
    const body = `## Knowledge Compatibility Review\n\n${testCase.review}`;
    assertEquals(
      validateKnowledgeCompatibilityReview(body).length === 0,
      testCase.valid,
      testCase.name,
    );
  }
});

Deno.test("Node 24 loads and executes trusted TypeScript adapters", async () => {
  const nodeSmoke = String.raw`
import assert from "node:assert/strict";
import { pathToFileURL } from "node:url";

assert.equal(Number.parseInt(process.versions.node, 10) >= 24, true);
const [validatorPath, typedFixturePath] = process.argv.slice(2);
const { validateKnowledgeCompatibilityReview } = await import(
  pathToFileURL(validatorPath).href
);
assert.equal(typeof validateKnowledgeCompatibilityReview, "function");
assert.deepEqual(
  validateKnowledgeCompatibilityReview(
    "## Knowledge Compatibility Review\n\n- [x] No effect on the v0.1 Knowledge semantic contract.",
  ),
  [],
);
assert.notDeepEqual(
  validateKnowledgeCompatibilityReview(
    "## Knowledge Compatibility Review\n\n- [x] Preserving implementation change; the canonical fixture and focused tests remain passing.",
  ),
    [],
);
const { runAdapterFixture } = await import(pathToFileURL(typedFixturePath).href);
assert.deepEqual(runAdapterFixture("loaded"), { loaded: true });
assert.deepEqual(runAdapterFixture("rejected"), { loaded: false });
`;
  const result = await new Deno.Command("node", {
    args: [
      "--input-type=module",
      "--eval",
      nodeSmoke,
      "knowledge-compatibility-adapter-smoke",
      `${Deno.cwd()}/tools/knowledge_compatibility.ts`,
      `${Deno.cwd()}/tools/knowledge_compatibility_node_fixture.ts`,
    ],
    stdout: "piped",
    stderr: "piped",
  }).output();
  assertEquals(
    result.success,
    true,
    new TextDecoder().decode(result.stderr) ||
      new TextDecoder().decode(result.stdout),
  );
});

// REQ-STO-014
Deno.test("v0.1 fixture is documented as a semantic compatibility oracle", async () => {
  const fixture = JSON.parse(
    await Deno.readTextFile(
      "crates/ugoite-iceberg/tests/fixtures/v0.1-knowledge.json",
    ),
  ) as {
    fixture_version?: number;
    release?: string;
    space?: {
      entries?: unknown[];
      update?: { expected_history_length?: number };
    };
  };
  assertEquals(fixture.fixture_version, 1);
  assertEquals(fixture.release, "v0.1");
  assertEquals(
    fixture.space?.entries?.length,
    fixture.space?.update?.expected_history_length,
  );
  requireText(contract, "canonical semantic fixture", "compatibility contract");
  requireText(contract, "must remain readable", "compatibility contract");
});
