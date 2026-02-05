/**
 * Markdown utilities used by entries route.
 */

/**
 * Replace the first level-1 heading in a markdown template with the provided title.
 * If no H1 is present, prepend one to the template.
 */
export function replaceFirstH1(template: string, title: string): string {
	const lines = template.split(/\r?\n/);
	for (let i = 0; i < lines.length; i += 1) {
		if (lines[i].startsWith("# ") || lines[i].startsWith("#\t")) {
			lines[i] = `# ${title}`;
			return lines.join("\n");
		}
	}
	return `# ${title}\n\n${template}`;
}

/**
 * Ensure the markdown has frontmatter with a `form:` entry set to the provided formName.
 * Preserves existing frontmatter if present and replaces or inserts the form field.
 */
export function ensureFormFrontmatter(markdown: string, formName: string): string {
	const trimmed = markdown.trimStart();
	const lines = markdown.split(/\r?\n/);

	function findFrontmatterRange(lines: string[]): [number, number] | null {
		let start = -1;
		for (let i = 0; i < lines.length; i += 1) {
			if (lines[i].trim() === "---") {
				start = i;
				break;
			}
		}
		if (start === -1) return null;
		const openingIndent = lines[start].match(/^\s*/)?.[0] ?? "";
		for (let i = start + 1; i < lines.length; i += 1) {
			if (lines[i].trim() === "---" && lines[i].startsWith(openingIndent)) {
				return [start, i];
			}
		}
		return null;
	}

	if (trimmed.startsWith("---")) {
		const range = findFrontmatterRange(lines);
		if (range) {
			const [startIdx, endIdx] = range;
			const fmLines = lines.slice(startIdx + 1, endIdx);
			const hasForm = fmLines.some((l) => l.trimStart().startsWith("form:"));
			const nextFmLines = hasForm
				? fmLines.map((l) => (l.trimStart().startsWith("form:") ? `form: ${formName}` : l))
				: [`form: ${formName}`, ...fmLines];
			return [lines[startIdx], ...nextFmLines, lines[endIdx], ...lines.slice(endIdx + 1)].join(
				"\n",
			);
		}
	}
	return `---\nform: ${formName}\n---\n\n${markdown}`;
}

/**
 * Helper to escape regex special characters.
 */
function escapeRegExp(text: string): string {
	return text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

/**
 * Update the content of an H2 section.
 * If the section exists, replace its content until the next header.
 * If it doesn't exist, append it to the end.
 */
export function updateH2Section(markdown: string, sectionTitle: string, newValue: string): string {
	const lines = markdown.split(/\r?\n/);
	const escapedTitle = escapeRegExp(sectionTitle);
	const h2Regex = new RegExp(`^##\\s+${escapedTitle}\\s*$`, "i");

	let sectionIdx = -1;
	for (let i = 0; i < lines.length; i++) {
		if (h2Regex.test(lines[i])) {
			sectionIdx = i;
			break;
		}
	}

	if (sectionIdx !== -1) {
		// Find next header
		let nextHeaderIdx = -1;
		for (let i = sectionIdx + 1; i < lines.length; i++) {
			if (lines[i].startsWith("#")) {
				nextHeaderIdx = i;
				break;
			}
		}

		const before = lines.slice(0, sectionIdx + 1);
		const after = nextHeaderIdx === -1 ? [] : lines.slice(nextHeaderIdx);

		return [...before, newValue, ...after].join("\n");
	}

	// Append to end if not found
	return `${markdown.trimEnd()}\n\n## ${sectionTitle}\n${newValue}\n`;
}

// Named exports only to match project conventions
