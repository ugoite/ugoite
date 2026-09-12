import { describe, expect, it } from "vitest";
import {
  parseSourceToDraftViaWasm,
  renderDraftToSourceViaWasm,
} from "~/lib/entry-compat";
import { validateEntryDraftViaWasm } from "~/lib/entry-validation";
import type { Form } from "~/lib/types";

const testForm = (): Form => ({
  name: "Note",
  version: 1,
  template: "# Note",
  fields: {
    Body: { type: "string", required: true },
    Done: { type: "boolean", required: false },
    Count: { type: "integer", required: false },
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
