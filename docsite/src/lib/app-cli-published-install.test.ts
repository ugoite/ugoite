import { readFileSync } from "node:fs";
import path from "node:path";
import { expect, test } from "vitest";

const cliPage = readFileSync(
	path.resolve(process.cwd(), "src/pages/app/cli/index.astro"),
	"utf-8",
);

test("REQ-OPS-018: app CLI landing page exposes the released install path before source builds", () => {
	expect(cliPage).toContain("Published install (recommended)");
	expect(cliPage).toContain(
		"npm install -g ugoite && ugoite-install && ugoite --help",
	);
	expect(cliPage).toContain("Build from source (contributors)");
	expect(cliPage).toContain('href={withBasePath("/docs/guide/cli")}');
});
