import { assertEquals } from "@std/assert/equals";
import {
  findEntryAuthorityViolations,
  readEntryAuthorityViolations,
} from "./entry_authority_gate.ts";

Deno.test("Entry authoring keeps Knowledge semantics behind the Rust bridge", async () => {
  assertEquals(await readEntryAuthorityViolations(), []);
  const pane = await Deno.readTextFile(
    "frontend/src/components/EntryDetailPane.tsx",
  );
  assertEquals(pane.includes("parseSourceToDraftViaWasm"), true);
  assertEquals(pane.includes("renderDraftToSourceViaWasm"), true);
});

Deno.test("authority gate rejects a new semantic Markdown helper dependency", () => {
  const negativeFixture = `
    export function saveEntry(markdown: string) {
      return updateH2Section(markdown, "Status", "open");
    }
  `;
  assertEquals(
    findEntryAuthorityViolations(
      "frontend/src/components/NewEntryAuthoring.tsx",
      negativeFixture,
    ),
    {
      path: "frontend/src/components/NewEntryAuthoring.tsx",
      symbols: ["updateH2Section"],
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
      "frontend/src/components/MarkdownEditor.tsx",
      "renderMarkdownPreview(markdown)",
    ),
    undefined,
  );
});
