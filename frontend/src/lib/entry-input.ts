import { ensureFormFrontmatter, replaceFirstH1, updateH2Section } from "~/lib/markdown";
import type { Form } from "~/lib/types";

export type EntryInputMode = "markdown" | "webform" | "chat";

export const buildEntryMarkdownFromFields = (
	formDef: Form,
	title: string,
	fieldValues: Record<string, string>,
): string => {
	let content = ensureFormFrontmatter(replaceFirstH1(formDef.template, title), formDef.name);
	for (const [name, value] of Object.entries(fieldValues)) {
		if (name.startsWith("__") || !value.trim()) continue;
		content = updateH2Section(content, name, value.trim());
	}
	return content;
};

export const buildEntryMarkdownByMode = (
	formDef: Form,
	title: string,
	values: Record<string, string>,
	mode: EntryInputMode,
): string => {
	if (mode === "markdown") {
		const markdown = values.__markdown?.trim();
		if (markdown) return markdown;
	}
	return buildEntryMarkdownFromFields(formDef, title, values);
};
