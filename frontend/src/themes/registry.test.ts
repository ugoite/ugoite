import { describe, expect, it } from "vitest";
import fs from "node:fs";
import path from "node:path";
import { PRIMARY_COLORS } from "./color-registry";
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
			for (const token of REQUIRED_THEME_TOKENS) {
				expect(content).toContain(token);
			}
			expect(hasJapanese(content)).toBe(false);
		}
	});

	it("REQ-FE-041: imports each theme stylesheet", () => {
		const appCssPath = path.resolve(process.cwd(), "src/app.css");
		const content = fs.readFileSync(appCssPath, "utf-8");
		expect(content).toContain('@import "./themes/colors.css";');
		for (const theme of UI_THEMES) {
			expect(content).toContain(`@import "./themes/${theme.id}.css";`);
		}
	});

	it("REQ-FE-041: uses English-only theme labels", () => {
		for (const theme of UI_THEMES) {
			expect(hasJapanese(theme.label)).toBe(false);
		}
	});

	it("REQ-FE-041: maps neutral colors to theme tokens", () => {
		const colorCssPath = path.resolve(process.cwd(), "src/themes/colors.css");
		const content = fs.readFileSync(colorCssPath, "utf-8");
		expect(content).toContain("--ui-surface-1");
		expect(content).toContain("--ui-foreground");
		expect(content).toContain('data-color-mode="dark"');
		expect(content).toContain('data-primary-color="violet"');
	});

	it("REQ-FE-041: defines primary color options in English", () => {
		for (const color of PRIMARY_COLORS) {
			expect(hasJapanese(color.label)).toBe(false);
		}
	});
});
