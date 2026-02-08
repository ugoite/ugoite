import fs from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";

const SRC_ROOT = path.resolve(process.cwd(), "src");

const COLOR_UTILITY_REGEX =
	/\b(?:bg|text|border|ring|fill|stroke|from|to|via)-(?:slate|gray|zinc|neutral|stone|red|orange|amber|yellow|lime|green|emerald|teal|cyan|sky|blue|indigo|violet|purple|fuchsia|pink|rose|black|white)(?:-\d{2,3})?(?:\/\d{1,3})?\b/g;

const INLINE_COLOR_STYLE_REGEX =
	/style=\{\{[^}]*\b(color|background|backgroundColor|borderColor|boxShadow|fill|stroke)\b[^}]*\}\}/g;

const collectFiles = (dir: string): string[] => {
	const entries = fs.readdirSync(dir, { withFileTypes: true });
	const files: string[] = [];
	for (const entry of entries) {
		const fullPath = path.join(dir, entry.name);
		if (entry.isDirectory()) {
			files.push(...collectFiles(fullPath));
			continue;
		}
		if (!entry.name.endsWith(".tsx")) continue;
		if (entry.name.endsWith(".test.tsx")) continue;
		files.push(fullPath);
	}
	return files;
};

describe("Theme usage", () => {
	it("REQ-FE-042: avoids raw color utilities in themed components", () => {
		const targetDirs = ["components", "routes"].map((dir) => path.join(SRC_ROOT, dir));
		const offenders: Array<{ file: string; matches: string[] }> = [];
		for (const dir of targetDirs) {
			for (const file of collectFiles(dir)) {
				const content = fs.readFileSync(file, "utf-8");
				const matches = content.match(COLOR_UTILITY_REGEX);
				if (matches && matches.length > 0) {
					offenders.push({ file, matches: Array.from(new Set(matches)) });
				}
			}
		}

		expect(offenders, `Raw color utilities found: ${JSON.stringify(offenders, null, 2)}`).toEqual(
			[],
		);
	});

	it("REQ-FE-042: avoids inline color styles in themed components", () => {
		const targetDirs = ["components", "routes"].map((dir) => path.join(SRC_ROOT, dir));
		const offenders: Array<{ file: string; matches: string[] }> = [];
		for (const dir of targetDirs) {
			for (const file of collectFiles(dir)) {
				const content = fs.readFileSync(file, "utf-8");
				const matches = content.match(INLINE_COLOR_STYLE_REGEX);
				if (matches && matches.length > 0) {
					offenders.push({ file, matches: Array.from(new Set(matches)) });
				}
			}
		}

		expect(offenders, `Inline color styles found: ${JSON.stringify(offenders, null, 2)}`).toEqual(
			[],
		);
	});
});
