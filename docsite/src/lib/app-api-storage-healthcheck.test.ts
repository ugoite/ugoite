import { readFileSync } from "node:fs";
import path from "node:path";
import { expect, test } from "vitest";

const apiStoragePage = readFileSync(
	path.resolve(process.cwd(), "src/pages/app/api-storage/index.astro"),
	"utf-8",
);

test("REQ-OPS-010: app API & Storage page verifies readiness with the health endpoint first", () => {
	expect(apiStoragePage).toContain("curl -sS http://localhost:8000/health");
	expect(apiStoragePage).not.toContain(
		'TerminalCommand command="curl http://localhost:8000/spaces"',
	);
	expect(apiStoragePage).toContain(
		"Authenticated routes such as <code>/spaces</code> require",
	);
});
