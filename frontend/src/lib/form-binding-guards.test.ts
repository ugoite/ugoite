import { describe, expect, it } from "vitest";
import {
  replaceSqlFormBindingGuard,
  retainReferencedSqlFormBindingGuards,
} from "./form-binding-guards";

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

  it("retains an explicit guard for a relation with the legacy stable-ID shape", async () => {
    const relation = "form_0123456789abcdef0123456789abcdef";
    const selected = { name: relation, formId: "selected-form-id" };

    await expect(
      replaceSqlFormBindingGuard(
        'SELECT * FROM "' + relation + '"',
        [],
        selected,
      ),
    ).resolves.toEqual([selected]);
  });

  it("keeps guards while Rust cannot parse the current draft", async () => {
    await expect(
      retainReferencedSqlFormBindingGuards("SELECT (", [a]),
    ).resolves.toEqual([a]);
  });

  it("replaces the selected identity while preserving another referenced Form guard", async () => {
    await expect(
      replaceSqlFormBindingGuard(
        'SELECT * FROM "Entry" AS e JOIN "Other" AS o ON TRUE',
        [a, b],
        { name: "Entry", formId: "form-c" },
      ),
    ).resolves.toEqual([
      b,
      { name: "Entry", formId: "form-c" },
    ]);
  });
});
