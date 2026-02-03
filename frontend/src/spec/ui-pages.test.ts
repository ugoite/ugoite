/* @vitest-environment node */
import { describe, expect, it } from "vitest";
import { readFileSync, readdirSync, statSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { parse } from "yaml";

type PageSpec = {
	page?: {
		id?: string;
		title?: string;
		route?: string;
		implementation?: string;
	};
	components?: {
		shared?: Array<Record<string, unknown>>;
		body?: Array<Record<string, unknown>>;
	};
};

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(__dirname, "../../..");
const pagesDir = path.join(repoRoot, "docs/spec/ui/pages");

const allowedComponentTypes = new Set([
	"tab-bar",
	"floating-icon-button",
	"heading",
	"icon-button",
	"search-bar",
	"filter-button",
	"dialog",
	"list",
	"grid-list",
	"text-input",
	"sql-editor",
	"button",
	"form",
	"markdown-editor",
	"toolbar",
	"sort-button",
	"data-grid",
	"csv-download-button",
	"settings-panel",
]);

const collectYamlFiles = (dir: string): string[] => {
	const entries = readdirSync(dir);
	const files: string[] = [];
	for (const entry of entries) {
		const fullPath = path.join(dir, entry);
		const stats = statSync(fullPath);
		if (stats.isDirectory()) {
			files.push(...collectYamlFiles(fullPath));
			continue;
		}
		if (entry.endsWith(".yaml")) {
			files.push(fullPath);
		}
	}
	return files;
};

const loadPages = () => {
	const files = collectYamlFiles(pagesDir);
	const pages = files.map((filePath) => {
		const contents = readFileSync(filePath, "utf8");
		return {
			filePath,
			spec: parse(contents) as PageSpec,
		};
	});
	return pages;
};

const collectTargets = (value: unknown, targets: string[]) => {
	if (Array.isArray(value)) {
		for (const item of value) {
			collectTargets(item, targets);
		}
		return;
	}
	if (!value || typeof value !== "object") {
		return;
	}
	for (const [key, entry] of Object.entries(value)) {
		if (key.startsWith("target_page") && typeof entry === "string") {
			targets.push(entry);
			continue;
		}
		collectTargets(entry, targets);
	}
};

const findSharedComponent = (components: PageSpec["components"], id: string) =>
	components?.shared?.find((component) => component.id === id);

const findBottomTabs = (components: PageSpec["components"]) =>
	components?.body?.find(
		(component) => component.type === "tab-bar" && component.position === "bottom-center-floating",
	);

describe("UI spec YAML registry", () => {
	it("REQ-FE-040: loads UI page specs", () => {
		const pages = loadPages();
		expect(pages.length).toBeGreaterThan(0);

		for (const { spec, filePath } of pages) {
			expect(spec.page?.id, filePath).toBeTruthy();
			expect(spec.page?.title, filePath).toBeTruthy();
			expect(spec.page?.route, filePath).toBeTruthy();
			expect(spec.page?.implementation, filePath).toBeTruthy();
			expect(["unimplemented", "implemented", "in-progress"]).toContain(spec.page?.implementation);
		}
	});

	it("REQ-FE-040: validates component types", () => {
		const pages = loadPages();
		for (const { spec, filePath } of pages) {
			const allComponents = [...(spec.components?.shared ?? []), ...(spec.components?.body ?? [])];
			for (const component of allComponents) {
				const type = component.type;
				expect(type, `${filePath} has component missing type`).toBeTruthy();
				expect(
					allowedComponentTypes.has(String(type)),
					`${filePath} uses unsupported component type: ${String(type)}`,
				).toBe(true);
			}
		}
	});

	it("REQ-FE-040: validates page links", () => {
		const pages = loadPages();
		const pageIds = new Set(
			pages.map(({ spec }) => spec.page?.id).filter((id): id is string => Boolean(id)),
		);

		for (const { spec, filePath } of pages) {
			const targets: string[] = [];
			collectTargets(spec, targets);
			for (const target of targets) {
				expect(pageIds.has(target), `${filePath} references missing page: ${target}`).toBe(true);
			}
		}
	});

	it("REQ-FE-040: validates shared workspace chrome", () => {
		const pages = loadPages();
		for (const { spec, filePath } of pages) {
			const topTabs = findSharedComponent(spec.components, "top-tabs");
			expect(topTabs, `${filePath} missing top-tabs`).toBeTruthy();
			expect(topTabs?.type).toBe("tab-bar");
			expect(topTabs?.position).toBe("top-center-floating");

			const settingsButton = findSharedComponent(spec.components, "settings-button");
			expect(settingsButton, `${filePath} missing settings-button`).toBeTruthy();
			expect(settingsButton?.type).toBe("floating-icon-button");
			expect(settingsButton?.icon).toBe("settings");
		}
	});

	it("REQ-FE-040: validates object/grid tabs", () => {
		const pages = loadPages();
		const targetPages = new Set(["workspace-notes-object", "workspace-class-grid"]);
		for (const { spec, filePath } of pages) {
			if (!spec.page?.id || !targetPages.has(spec.page.id)) {
				continue;
			}
			const bottomTabs = findBottomTabs(spec.components);
			expect(bottomTabs, `${filePath} missing bottom view tabs`).toBeTruthy();
			const tabs = Array.isArray(bottomTabs?.tabs) ? bottomTabs?.tabs : [];
			const tabIds = tabs.map((tab: { id?: string }) => tab.id);
			expect(tabIds).toContain("object");
			expect(tabIds).toContain("grid");
		}
	});
});
