/**
 * Markdown utilities used by notes route.
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
 * Ensure the markdown has frontmatter with a `class:` entry set to the provided className.
 * Preserves existing frontmatter if present and replaces or inserts the class field.
 */
export function ensureClassFrontmatter(markdown: string, className: string): string {
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
			const hasClass = fmLines.some((l) => l.trimStart().startsWith("class:"));
			const nextFmLines = hasClass
				? fmLines.map((l) => (l.trimStart().startsWith("class:") ? `class: ${className}` : l))
				: [`class: ${className}`, ...fmLines];
			return [lines[startIdx], ...nextFmLines, lines[endIdx], ...lines.slice(endIdx + 1)].join(
				"\n",
			);
		}
	}
	return `---\nclass: ${className}\n---\n\n${markdown}`;
}

// Named exports only to match project conventions
