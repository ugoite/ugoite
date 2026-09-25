import { assertEquals } from "@std/assert/equals";
import { validateKnowledgeCompatibilityReview } from "./knowledge_compatibility.ts";

const contract = await Deno.readTextFile(
  "docs/architecture/release/v0.1-knowledge-compatibility.md",
);
const template = await Deno.readTextFile(".github/pull_request_template.md");
const validator = await Deno.readTextFile("tools/create_pr.ts");
const compatibilityValidator = await Deno.readTextFile(
  "tools/knowledge_compatibility.ts",
);
const workflow = await Deno.readTextFile(
  ".github/workflows/pr-require-close-issue.yml",
);
const gate = await Deno.readTextFile("tools/pr_body_gate.ts");
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
  requireText(workflow, "mise.toml", "PR validator");
  requireText(workflow, "tools/knowledge_compatibility.ts", "PR validator");
  requireText(workflow, "tools/pr_body_gate.ts", "PR validator");
  requireText(workflow, "jdx/mise-action@", "PR validator");
  requireText(workflow, "deno run", "PR validator");
  requireText(workflow, "--allow-net=api.github.com", "PR validator");
  requireText(workflow, "GITHUB_TOKEN", "PR validator");
  assertEquals(
    workflow.includes("actions/github-script@"),
    false,
    "PR validator must not execute the gate through Node",
  );
  assertEquals(
    workflow.includes("process.versions"),
    false,
    "PR validator must not require a Node runtime",
  );
  assertEquals(
    workflow.includes("validateKnowledgeCompatibilityReview(body)"),
    false,
    "PR validator must not implement classification semantics in YAML",
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
  requireText(gate, "validateKnowledgeCompatibilityReview", "PR gate");
  requireText(gate, "pr-(\\d+)", "PR gate");
  requireText(gate, "api.github.com/repos/", "PR gate");
  requireText(gate, "dependabot[bot]", "PR gate");
  requireText(
    gate,
    "skipping the human PR template gate",
    "PR gate",
  );
  requireText(gate, "## Summary", "PR gate");
  requireText(gate, "close: #123", "PR gate");
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
  requireText(ciWorkflow, "  pr-context-report:", "PR Context Report CI job");
  requireText(
    ciWorkflow,
    "ref: ${{ github.event.pull_request.head.sha }}",
    "PR Context Report head checkout",
  );
  requireText(
    ciWorkflow,
    '--base "$BASE_SHA" --head "$HEAD_SHA"',
    "PR Context Report revisions",
  );
  requireText(ciWorkflow, "--format json", "PR Context Report JSON output");
  requireText(
    ciWorkflow,
    "--format markdown",
    "PR Context Report Markdown output",
  );
  requireText(
    ciWorkflow,
    "name: pr-context-report",
    "PR Context Report artifact",
  );
  const webJobStart = ciWorkflow.indexOf("  web:\n");
  const artifactsJobStart = ciWorkflow.indexOf("  artifacts:\n", webJobStart);
  assertEquals(webJobStart >= 0, true, "CI web job must exist");
  assertEquals(
    artifactsJobStart > webJobStart,
    true,
    "CI artifacts job must exist",
  );
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
