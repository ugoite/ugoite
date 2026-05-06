import { readFileSync } from "node:fs";
import path from "node:path";
import { expect, test } from "vitest";

const cliPage = readFileSync(
	path.resolve(process.cwd(), "src/pages/app/cli/index.astro"),
	"utf-8",
);

test("REQ-OPS-018: app CLI landing page exposes the released install path before source builds", () => {
	const verifyFirstHeading = "Verify-first archive install (recommended)";
	const bootstrapHeading = "Bootstrap helper (secondary)";
	const sourceHeading = "Build from source (contributors)";

	expect(cliPage).toContain(verifyFirstHeading);
	expect(cliPage).toContain("VERSION=0.0.1-beta.13");
	expect(cliPage).toContain(
		"npm install -g ugoite && ugoite-install && ugoite --help",
	);
	expect(cliPage).toContain(sourceHeading);
	expect(cliPage).toContain('href={withBasePath("/docs/guide/cli")}');
	expect(cliPage.indexOf(verifyFirstHeading)).toBeLessThan(
		cliPage.indexOf(bootstrapHeading),
	);
	expect(cliPage.indexOf(bootstrapHeading)).toBeLessThan(
		cliPage.indexOf(sourceHeading),
	);
});
