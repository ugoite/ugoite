import { describe, expect, it } from "vitest";
import { retainReferencedSqlFormBindingGuards } from "./form-binding-guards";

describe("SQL Form binding guard references", () => {
  const a = { name: "Entry", formId: "form-a" };
  const b = { name: "Other", formId: "form-b" };

  it("drops an obsolete picker guard when Rust parses a manual edit without its relation", async () => {
    await expect(
      retainReferencedSqlFormBindingGuards("SELECT 1", [a]),
    ).resolves.toEqual([]);
  });

  it("retains only still-referenced guards, independent of same-name replacements", async () => {
    await expect(
      retainReferencedSqlFormBindingGuards('SELECT * FROM "Entry"', [a, b]),
    ).resolves.toEqual([a]);
  });

  it("keeps guards while Rust cannot parse the current draft", async () => {
    await expect(
      retainReferencedSqlFormBindingGuards("SELECT (", [a]),
    ).resolves.toEqual([a]);
  });
});
