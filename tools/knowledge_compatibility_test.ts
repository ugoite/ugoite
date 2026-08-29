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
  for (const source of [validator, workflow]) {
    requireText(source, "Knowledge Compatibility Review", "PR validator");
  }
  requireText(
    compatibilityValidator,
    "select exactly one valid classification",
    "PR validator",
  );
  requireText(validator, "knowledge_compatibility.ts", "PR validator");
  requireText(
    workflow,
    "select exactly one valid classification",
    "PR validator",
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
});

Deno.test("Knowledge Compatibility Review validates one classification and evidence", () => {
  const preserving = `
## Knowledge Compatibility Review

- [x] Preserving implementation change; the canonical fixture and focused tests remain passing.
Evidence: fixture and reopened Space assertions pass.
`;
  assertEquals(validateKnowledgeCompatibilityReview(preserving), []);

  const noEffect = `
## Knowledge Compatibility Review

- [x] No effect on the v0.1 Knowledge semantic contract.
`;
  assertEquals(validateKnowledgeCompatibilityReview(noEffect), []);

  const multiple = `
## Knowledge Compatibility Review

- [x] No effect on the v0.1 Knowledge semantic contract.
- [x] Preserving implementation change; the canonical fixture and focused tests remain passing.
Evidence: fixture passes.
`;
  assertEquals(validateKnowledgeCompatibilityReview(multiple).length, 1);

  const missingEvidence = `
## Knowledge Compatibility Review

- [x] Preserving implementation change; the canonical fixture and focused tests remain passing.
`;
  assertEquals(validateKnowledgeCompatibilityReview(missingEvidence).length, 1);
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
