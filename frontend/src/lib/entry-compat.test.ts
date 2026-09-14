import { describe, expect, it } from "vitest";
import {
  parseSourceToDraftViaWasm,
  renderDraftToSourceViaWasm,
} from "~/lib/entry-compat";
import { validateEntryDraftViaWasm } from "~/lib/entry-validation";
import { UgoiteApiError } from "~/lib/ugoite-client/protocol";
import type { Form } from "~/lib/types";

const testForm = (): Form => ({
  id: "00000000-0000-0000-0000-000000000001",
  name: "Note",
  version: 1,
  template: "# Note",
  fields: {
    Body: { id: 100, type: "string", required: true },
    Done: { id: 101, type: "boolean", required: false },
    Count: { id: 102, type: "integer", required: false },
    Labels: { id: 103, type: "list", required: false },
    Rows: { id: 104, type: "object_list", required: false },
  },
});

describe("entry-compat", () => {
  it("parses source-edited legacy Markdown back to a structured draft", async () => {
    const markdown =
      "---\nform: Note\n---\n# Title\n\n## Body\n\nhello\n\n## Done\nyes\n\n## Count\n42\n";
    const draft = await parseSourceToDraftViaWasm(markdown, "fallback");
    expect(draft.title).toBe("Title");
    expect(draft.fields.Body).toBe("hello");
    // Rust canonicalizes boolean aliases; TS helpers do not decide this.
    expect(draft.fields.Done).toBe("yes");
  });

  it("accepts typed non-scalar values at the compatibility render boundary", async () => {
    const source = await renderDraftToSourceViaWasm(
      testForm(),
      "Website",
      [],
      {
        Body: "hello",
        Labels: ["one", "two"],
        Rows: [{ step: "one" }],
      },
    );
    expect(source).toContain("## Labels");
    expect(source).toContain("## Rows");
    expect(source).not.toContain("[object Object]");
  });

  it("preserves the typed MARKDOWN_CONVERSION_LOSS envelope", async () => {
    await expect(
      parseSourceToDraftViaWasm(
        "---\nform: Note\n---\n# Note\n\nPreamble\n\n## Body\nkept\n",
        "fallback",
        { strict: true },
      ),
    ).rejects.toBeInstanceOf(UgoiteApiError);
    try {
      await parseSourceToDraftViaWasm(
        "---\nform: Note\n---\n# Note\n\nPreamble\n\n## Body\nkept\n",
        "fallback",
        { strict: true },
      );
    } catch (error) {
      expect(error).toBeInstanceOf(UgoiteApiError);
      expect((error as UgoiteApiError).code).toBe("MARKDOWN_CONVERSION_LOSS");
      expect((error as UgoiteApiError).detail).toMatchObject({
        diagnostics: [{ code: "markdown_unassigned_preamble" }],
      });
    }
  });

  it("keeps fields->source->fields normalized values unchanged", async () => {
    const form = testForm();
    const fields = { Body: "hello", Done: "yes", Count: "42" };
    const source = await renderDraftToSourceViaWasm(
      form,
      "Title",
      [],
      fields,
    );
    expect(source).toContain("## Body");
    const reparsed = await parseSourceToDraftViaWasm(source, "Title");
    const first = await validateEntryDraftViaWasm(form, {
      title: "Title",
      tags: [],
      fields,
    });
    const second = await validateEntryDraftViaWasm(form, {
      title: reparsed.title,
      tags: reparsed.tags,
      fields: reparsed.fields,
    });
    expect(first.ok).toBe(true);
    expect(second.ok).toBe(true);
    if (first.ok && second.ok) {
      expect(second.normalized).toEqual(first.normalized);
    }
  });

  it("keeps source view available when the bridge renders formless notes", async () => {
    const form = testForm();
    const source = await renderDraftToSourceViaWasm(
      form,
      "",
      [],
      { Body: "hello" },
    );
    expect(typeof source).toBe("string");
    expect(source.length).toBeGreaterThan(0);
  });
});
