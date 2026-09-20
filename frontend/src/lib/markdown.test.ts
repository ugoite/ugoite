import { describe, expect, it } from "vitest";
import { renderMarkdownPreview } from "./markdown";

describe("markdown preview", () => {
  it("escapes raw HTML while keeping simple formatting", () => {
    const preview = renderMarkdownPreview(
      '# Preview\n\n<img src=x onerror="alert(1)">\n\n**bold** `code`',
    );

    expect(preview).toContain(
      '<h1 class="text-2xl font-bold mb-2">Preview</h1>',
    );
    expect(preview).toContain("&lt;img src=x onerror=&quot;alert(1)&quot;&gt;");
    expect(preview).not.toContain("<img");
    expect(preview).toContain("<strong>bold</strong>");
    expect(preview).toContain('<code class="ui-code">code</code>');
  });
});
