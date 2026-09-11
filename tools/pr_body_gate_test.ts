import { assertEquals } from "@std/assert/equals";
import {
  extractPrNumberFromMergeGroupRef,
  isExemptAuthor,
  resolvePullRequestPointer,
  sectionText,
  validatePrBody,
} from "./pr_body_gate.ts";

const validBody = [
  "## Summary",
  "",
  "Migrate the timestamp contract.",
  "",
  "## Related Issue (required)",
  "",
  "close: #2462",
  "",
  "## Knowledge Compatibility Review",
  "",
  "- [x] No effect on the v0.1 Knowledge semantic contract.",
  "",
  "## Testing",
  "",
  "- [x] `mise run mitase:check`",
  "",
].join("\n");

Deno.test("pr_body_gate accepts a complete PR body", () => {
  assertEquals(validatePrBody(validBody), []);
});

Deno.test("pr_body_gate requires every template section", () => {
  const errors = validatePrBody("## Summary\n\nFilled.\n");
  assertEquals(
    errors.some((error) => error.includes("## Related Issue (required)")),
    true,
  );
  assertEquals(
    errors.some((error) => error.includes("## Knowledge Compatibility Review")),
    true,
  );
  assertEquals(
    errors.some((error) => error.includes("## Testing")),
    true,
  );
});

Deno.test("pr_body_gate rejects the Summary placeholder", () => {
  assertEquals(
    validatePrBody(validBody.replace(
      "Migrate the timestamp contract.",
      "-",
    )).some((error) => error.includes("Summary section must be filled in")),
    true,
  );
});

Deno.test("pr_body_gate accepts close and closes issue links", () => {
  assertEquals(
    validatePrBody(validBody.replace("close: #2462", "closes #2462")),
    [],
  );
});

Deno.test("pr_body_gate rejects a missing issue link", () => {
  assertEquals(
    validatePrBody(validBody.replace("close: #2462", "see #2462")).some(
      (error) => error.includes("Related Issue must include"),
    ),
    true,
  );
});

Deno.test("pr_body_gate delegates classification to the canonical validator", () => {
  const twoChecked = validBody.replace(
    "- [x] No effect on the v0.1 Knowledge semantic contract.",
    "- [x] No effect on the v0.1 Knowledge semantic contract.\n- [x] Preserving implementation change; the canonical fixture and focused tests remain passing.\nEvidence: done.",
  );
  assertEquals(
    validatePrBody(twoChecked).some((error) =>
      error.includes("exactly one valid classification")
    ),
    true,
  );
});

Deno.test("pr_body_gate requires a Testing checklist item", () => {
  const withoutChecklist = validBody.replace(
    "- [x] `mise run mitase:check`",
    "Ran the checks.",
  );
  assertEquals(
    validatePrBody(withoutChecklist).some((error) =>
      error.includes("Testing section must include at least one checklist item")
    ),
    true,
  );
});

Deno.test("pr_body_gate extracts sections case-insensitively", () => {
  assertEquals(
    sectionText(validBody, "summary"),
    "Migrate the timestamp contract.",
  );
  assertEquals(sectionText("no headings", "Summary"), "");
});

Deno.test("pr_body_gate exempts only the Dependabot author", () => {
  assertEquals(isExemptAuthor("dependabot[bot]"), true);
  assertEquals(isExemptAuthor("tohboeh5"), false);
  assertEquals(isExemptAuthor(undefined), false);
});

Deno.test("pr_body_gate parses merge-group refs for PR numbers", () => {
  assertEquals(
    extractPrNumberFromMergeGroupRef(
      "refs/heads/gh-readonly-queue/main/pr-2462-abc123",
    ),
    2462,
  );
  assertEquals(extractPrNumberFromMergeGroupRef("refs/heads/main"), null);
});

Deno.test("pr_body_gate resolves pull_request event bodies", async () => {
  const eventPath = await Deno.makeTempFile({ prefix: "pr-event-" });
  try {
    await Deno.writeTextFile(
      eventPath,
      JSON.stringify({
        pull_request: { body: validBody, user: { login: "tohboeh5" } },
      }),
    );
    const pointer = await resolvePullRequestPointer({
      GITHUB_EVENT_NAME: "pull_request_target",
      GITHUB_EVENT_PATH: eventPath,
    });
    assertEquals(pointer.body, validBody);
    assertEquals(pointer.author, "tohboeh5");
    assertEquals(validatePrBody(pointer.body), []);
  } finally {
    await Deno.remove(eventPath);
  }
});

Deno.test("pr_body_gate yields an empty body without a merge-group PR number", async () => {
  const pointer = await resolvePullRequestPointer({
    GITHUB_EVENT_NAME: "merge_group",
    GITHUB_REF: "refs/heads/gh-readonly-queue/main",
    GITHUB_REPOSITORY: "ugoite/ugoite",
    GITHUB_TOKEN: "test-token",
  });
  assertEquals(pointer, { body: "" });
  assertEquals(validatePrBody(pointer.body).length > 0, true);
});
