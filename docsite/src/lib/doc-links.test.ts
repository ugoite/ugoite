import { rmSync, symlinkSync } from "node:fs";
import path from "node:path";
import { afterEach, expect, test, vi } from "vitest";

afterEach(() => {
	vi.unstubAllEnvs();
	vi.resetModules();
});

test("REQ-E2E-006: resolveDocHref preserves safe external targets and rejects unsupported schemes", async () => {
	delete process.env.GITHUB_REPOSITORY;
	vi.resetModules();
	const { resolveDocHref } = await import("./doc-links");

	expect(resolveDocHref(null, "guide/cli.md")).toBeNull();
	expect(resolveDocHref("   ", "guide/cli.md")).toBeNull();
	expect(resolveDocHref("#overview", "guide/cli.md")).toBe("#overview");
	expect(resolveDocHref("?tab=links", "guide/cli.md")).toBe("?tab=links");
	expect(resolveDocHref("//cdn.example.com/docs.js", "guide/cli.md")).toBe(
		"//cdn.example.com/docs.js",
	);
	expect(resolveDocHref("https://example.com/docs", "guide/cli.md")).toBe(
		"https://example.com/docs",
	);
	expect(resolveDocHref("mailto:team@example.com", "guide/cli.md")).toBe(
		"mailto:team@example.com",
	);
	expect(resolveDocHref("javascript:alert(1)", "guide/cli.md")).toBeNull();
});

test("REQ-E2E-006: resolveDocHref resolves absolute docsite and application routes against the configured base path", async () => {
	vi.stubEnv("GITHUB_REPOSITORY", "octo/example");
	vi.resetModules();
	const { resolveDocHref } = await import("./doc-links");
	const baseUrl = "/ugoite";

	expect(resolveDocHref("/docs/spec/index", "guide/cli.md", baseUrl)).toBe(
		"/ugoite/docs/spec/index",
	);
	expect(resolveDocHref("/docs/spec/ui", "guide/cli.md", baseUrl)).toBe(
		"/ugoite/docs/spec/ui/README",
	);
	expect(
		resolveDocHref("/docs/spec/ui/README.md#nav", "guide/cli.md", baseUrl),
	).toBe("/ugoite/docs/spec/ui/README#nav");
	expect(
		resolveDocHref("/docs/spec/architecture", "guide/cli.md", baseUrl),
	).toBe("https://github.com/octo/example/tree/main/docs/spec/architecture");
	expect(resolveDocHref("/docs/missing", "guide/cli.md", baseUrl)).toBeNull();
	expect(
		resolveDocHref(
			"/app/frontend/pages/readme.md?tab=pages#links",
			"guide/cli.md",
			baseUrl,
		),
	).toBe("/ugoite/app/frontend/pages/readme?tab=pages#links");
});

test("REQ-E2E-006: resolveDocHref resolves relative docs and repository source links without leaking outside the base path", async () => {
	vi.stubEnv("GITHUB_REPOSITORY", "octo/example");
	vi.resetModules();
	const { resolveDocHref } = await import("./doc-links");
	const baseUrl = "/ugoite";
	const devNullLinkPath = path.resolve(process.cwd(), "../dev-null-link");
	rmSync(devNullLinkPath, { force: true });
	symlinkSync("/dev/null", devNullLinkPath);

	try {
		expect(
			resolveDocHref("../index.md?tab=api#intro", "spec/api/rest.md", baseUrl),
		).toBe("/ugoite/docs/spec/index?tab=api#intro");
		expect(resolveDocHref("../architecture", "spec/api/rest.md", baseUrl)).toBe(
			"https://github.com/octo/example/tree/main/docs/spec/architecture",
		);
		expect(
			resolveDocHref("../missing-dir", "spec/api/rest.md", baseUrl),
		).toBeNull();
		expect(
			resolveDocHref("../../README.md?plain=1", "guide/cli.md", baseUrl),
		).toBe("https://github.com/octo/example/blob/main/README.md?plain=1");
		expect(resolveDocHref("../../", "guide/cli.md", baseUrl)).toBe(
			"https://github.com/octo/example/blob/main/README.md",
		);
		expect(
			resolveDocHref("../../missing.txt", "guide/cli.md", baseUrl),
		).toBeNull();
		expect(
			resolveDocHref("../../dev-null-link", "guide/cli.md", baseUrl),
		).toBeNull();
	} finally {
		rmSync(devNullLinkPath, { force: true });
	}
});
