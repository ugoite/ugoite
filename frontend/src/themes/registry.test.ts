import { describe, expect, it } from "vitest";
import fs from "node:fs";
import path from "node:path";
import { UI_THEMES } from "./registry";
import { REQUIRED_THEME_TOKENS } from "./tokens";

const themeCssPath = (id: string) => path.resolve(process.cwd(), "src/themes", `${id}.css`);

const hasJapanese = (value: string) => /[\u3040-\u30FF\u4E00-\u9FFF]/u.test(value);

describe("Theme registry", () => {
	it("REQ-FE-041: defines a CSS file for each theme", () => {
		for (const theme of UI_THEMES) {
			const cssPath = themeCssPath(theme.id);
			expect(fs.existsSync(cssPath)).toBe(true);
			const content = fs.readFileSync(cssPath, "utf-8");
			expect(content).toContain(`data-ui-theme="${theme.id}"`);
			expect(content).toContain("--ui-surface-1");
			expect(content).toContain('data-color-mode="dark"');
			for (const token of REQUIRED_THEME_TOKENS) {
				expect(content).toContain(token);
			}
			expect(hasJapanese(content)).toBe(false);
		}
	});

	it("REQ-FE-041: uses English-only theme labels", () => {
		for (const theme of UI_THEMES) {
			expect(hasJapanese(theme.label)).toBe(false);
		}
	});

	it("REQ-FE-041: maps neutral colors to theme tokens", () => {
		const appCssPath = path.resolve(process.cwd(), "src/app.css");
		const content = fs.readFileSync(appCssPath, "utf-8");
		expect(content).toContain("--color-white: var(--ui-surface-1)");
		expect(content).toContain("--color-black: var(--ui-foreground)");
	});
});
