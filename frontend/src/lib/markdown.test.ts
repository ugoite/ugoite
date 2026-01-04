import { describe, it, expect } from "vitest";
import { replaceFirstH1, ensureClassFrontmatter } from "./markdown";

describe("markdown utils", () => {
	it("replaceFirstH1 replaces existing H1", () => {
		const t = "# Old\n\nBody";
		expect(replaceFirstH1(t, "New")).toContain("# New");
	});

	it("replaceFirstH1 prepends H1 if missing", () => {
		const t = "No title\nContent";
		expect(replaceFirstH1(t, "New")).toMatch(/^# New\n/);
	});

	it("ensureClassFrontmatter inserts class when none present", () => {
		const md = "# Title\n\nContent";
		const out = ensureClassFrontmatter(md, "Task");
		expect(out).toContain("class: Task");
	});

	it("ensureClassFrontmatter replaces existing class in frontmatter", () => {
		const md = "---\nclass: Old\n---\n\n# Title";
		const out = ensureClassFrontmatter(md, "Task");
		expect(out).toContain("class: Task");
		expect(out).not.toContain("class: Old");
	});

	it("ensureClassFrontmatter handles frontmatter with '---' in content safely", () => {
		const md = "---\ntitle: A\n---\n\nThis line contains three dashes --- in content\n\n# Title";
		const out = ensureClassFrontmatter(md, "Note");
		expect(out).toContain("class: Note");
	});
});
