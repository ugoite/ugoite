import { assertEquals } from "@std/assert/equals";

const contract = await Deno.readTextFile(
  "docs/spec/versions/v0.1-knowledge-compatibility.md",
);
const template = await Deno.readTextFile(".github/pull_request_template.md");
const validator = await Deno.readTextFile("tools/create_pr.ts");
const workflow = await Deno.readTextFile(
  ".github/workflows/pr-require-close-issue.yml",
);

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
  requireText(template, "- [ ]", "PR template");
  for (const source of [validator, workflow]) {
    requireText(source, "Knowledge Compatibility Review", "PR validator");
    requireText(source, "\\[x\\]", "PR validator");
  }
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
