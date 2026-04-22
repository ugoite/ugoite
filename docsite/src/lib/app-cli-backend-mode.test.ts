import { readFileSync } from "node:fs";
import path from "node:path";
import { expect, test } from "vitest";

const cliPage = readFileSync(
	path.resolve(process.cwd(), "src/pages/app/cli/index.astro"),
	"utf-8",
);

test("REQ-OPS-015: CLI landing page shows backend-mode routing and auth before remote space commands", () => {
	const backendConfigCommand =
		"cargo run -q -p ugoite-cli -- config set --mode backend --backend-url http://127.0.0.1:8000";
	const backendAuthCommand =
		"cargo run -q -p ugoite-cli -- auth login --mock-oauth";
	const backendRemoteCommand =
		"cargo run -q -p ugoite-cli -- config current && cargo run -q -p ugoite-cli -- space list";

	expect(cliPage).toContain(backendConfigCommand);
	expect(cliPage).toContain(backendAuthCommand);
	expect(cliPage).toContain(backendRemoteCommand);
	expect(cliPage).toContain(
		"Backend mode drops the local-root argument because the CLI now talks to the server.",
	);
	expect(cliPage).toContain("href={localDevAuthGuideHref}");
	expect(cliPage.indexOf(backendConfigCommand)).toBeLessThan(
		cliPage.indexOf(backendAuthCommand),
	);
	expect(cliPage.indexOf(backendAuthCommand)).toBeLessThan(
		cliPage.indexOf(backendRemoteCommand),
	);
});
