import { readFileSync } from "node:fs";
import path from "node:path";
import { expect, test } from "vitest";

const appOverviewPage = readFileSync(
	path.resolve(process.cwd(), "src/pages/app/index.astro"),
	"utf-8",
);
const appMcpPage = readFileSync(
	path.resolve(process.cwd(), "src/pages/app/mcp/index.astro"),
	"utf-8",
);

test("REQ-API-013: app MCP page keeps the shipped surface resource-first and clearly scoped", () => {
	expect(appOverviewPage).toContain('href={withBasePath("/app/mcp")}');
	expect(appMcpPage).toContain("resource-first baseline");
	expect(appMcpPage).toContain("ugoite://&#123;space_id&#125;/entries/list");
	expect(appMcpPage).toContain("no MCP tools or prompts yet");
	expect(appMcpPage).toContain("planned as v0.2 work");
	expect(appMcpPage).toContain('href={withBasePath("/docs/spec/api/mcp")}');
});
