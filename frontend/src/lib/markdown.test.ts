// REQ-ENTRY-006: Structured data extraction from markdown
import { describe, it, expect } from "vitest";
import { replaceFirstH1, ensureFormFrontmatter, updateH2Section } from "./markdown";

describe("markdown utils", () => {
	it("replaceFirstH1 replaces existing H1", () => {
		const t = "# Old\n\nBody";
		expect(replaceFirstH1(t, "New")).toContain("# New");
	});

	it("replaceFirstH1 prepends H1 if missing", () => {
		const t = "No title\nContent";
		expect(replaceFirstH1(t, "New")).toMatch(/^# New\n/);
	});

	it("ensureFormFrontmatter inserts form when none present", () => {
		const md = "# Title\n\nContent";
		const out = ensureFormFrontmatter(md, "Task");
		expect(out).toContain("form: Task");
	});

	it("ensureFormFrontmatter replaces existing form in frontmatter", () => {
		const md = "---\nform: Old\n---\n\n# Title";
		const out = ensureFormFrontmatter(md, "Task");
		expect(out).toContain("form: Task");
		expect(out).not.toContain("form: Old");
	});

	it("ensureFormFrontmatter handles frontmatter with '---' in content safely", () => {
		const md = "---\ntitle: A\n---\n\nThis line contains three dashes --- in content\n\n# Title";
		const out = ensureFormFrontmatter(md, "Entry");
		expect(out).toContain("form: Entry");
	});

	it("updateH2Section replaces existing section content", () => {
		const md = "# Title\n\n## Section\nOldValue\n\n## Next\nKeepMe";
		const out = updateH2Section(md, "Section", "NewValue");
		expect(out).toContain("## Section\nNewValue");
		expect(out).toContain("## Next\nKeepMe");
	});

	it("updateH2Section appends section if missing", () => {
		const md = "# Title\n\n## Other\nVal";
		const out = updateH2Section(md, "NewSec", "NewVal");
		expect(out).toContain("## NewSec\nNewVal");
	});

	it("updateH2Section handles special regex characters in title", () => {
		const md = "# Title\n\n## Section (Special) [Ref]\nOldValue";
		const out = updateH2Section(md, "Section (Special) [Ref]", "NewValue");
		expect(out).toContain("## Section (Special) [Ref]\nNewValue");
	});
});
