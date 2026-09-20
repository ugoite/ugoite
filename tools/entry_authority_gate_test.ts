import { assertEquals } from "@std/assert/equals";
import {
  findEntryAuthorityViolations,
  readEntryAuthorityViolations,
} from "./entry_authority_gate.ts";

Deno.test("Entry authoring keeps Knowledge semantics in structured fields", async () => {
  assertEquals(await readEntryAuthorityViolations(), []);
  const pane = await Deno.readTextFile(
    "frontend/src/components/EntryDetailPane.tsx",
  );
  assertEquals(pane.includes("validateEntryDraftViaWasm"), true);
  assertEquals(pane.includes("parseSourceToDraftViaWasm"), false);
  assertEquals(pane.includes("renderDraftToSourceViaWasm"), false);
});

Deno.test("authority gate rejects a new semantic Markdown helper dependency", () => {
  const negativeFixture = `
    import { parseMarkdownToStructuredDraft as parseDraft } from "~/lib/entry-input";
    export function saveEntry(markdown: string) {
      const draft = parseDraft(markdown);
      return updateH2Section(draft, "Status", "open");
    }
  `;
  assertEquals(
    findEntryAuthorityViolations(
      "frontend/src/components/NewEntryAuthoring.tsx",
      negativeFixture,
    ),
    {
      path: "frontend/src/components/NewEntryAuthoring.tsx",
      symbols: ["parseMarkdownToStructuredDraft", "updateH2Section"],
    },
  );
});

Deno.test("authority gate permits presentation-only Markdown compatibility modules", () => {
  assertEquals(
    findEntryAuthorityViolations(
      "frontend/src/lib/entry-input.ts",
      "replaceFirstH1(markdown, title)",
    ),
    undefined,
  );
  assertEquals(
    findEntryAuthorityViolations(
      "frontend/src/components/FormTable.tsx",
      "updateH2Section(markdown, field, value)",
    ),
    undefined,
  );
  assertEquals(
    findEntryAuthorityViolations(
      "frontend/src/components/MarkdownEditor.tsx",
      "renderMarkdownPreview(markdown)",
    ),
    undefined,
  );
});
