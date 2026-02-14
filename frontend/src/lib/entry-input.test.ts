import { describe, expect, it } from "vitest";

import { buildEntryMarkdownByMode } from "~/lib/entry-input";
import type { Form } from "~/lib/types";

describe("buildEntryMarkdownByMode", () => {
	it("REQ-FE-037: preserves user markdown whitespace in markdown mode", () => {
		const formDef: Form = {
			name: "Meeting",
			version: 1,
			template: "# Meeting\n\n## Date\n",
			fields: {
				Date: { type: "date", required: true },
			},
		};

		const markdown = "# Entry\n\n---\nform: Meeting\n---\n\n## Date\n2026-02-14\n";
		const result = buildEntryMarkdownByMode(formDef, "Entry", { __markdown: markdown }, "markdown");

		expect(result).toBe(markdown);
	});
});
