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

	it("REQ-FE-037: builds from fields when __markdown is empty in markdown mode", () => {
		const formDef: Form = {
			name: "Task",
			version: 1,
			template: "# Task\n\n## Status\n",
			fields: { Status: { type: "text" } },
		};
		const result = buildEntryMarkdownByMode(
			formDef as Form,
			"My Task",
			{ __markdown: "" },
			"markdown",
		);
		expect(result).toContain("My Task");
	});
});
