import { afterEach, expect, test, vi } from "vitest";

afterEach(() => {
	vi.unstubAllEnvs();
	vi.resetModules();
});

test("REQ-E2E-006: withBasePath preserves external links and prefixes internal routes", async () => {
	vi.resetModules();
	const { withBasePath } = await import("./base-path");

	expect(withBasePath("")).toBe("/");
	expect(withBasePath("/")).toBe("/");
	expect(withBasePath("#overview")).toBe("#overview");
	expect(withBasePath("?tab=links")).toBe("?tab=links");
	expect(withBasePath("//cdn.example.com/docs.js")).toBe(
		"//cdn.example.com/docs.js",
	);
	expect(withBasePath("https://example.com/docs")).toBe(
		"https://example.com/docs",
	);
	expect(withBasePath("/docs/spec/index")).toBe("/docs/spec/index");
	expect(withBasePath("docs/spec/index")).toBe("/docs/spec/index");
});

test("REQ-E2E-006: withBasePath keeps configured non-root base paths stable", async () => {
	vi.stubEnv("BASE_URL", "/ugoite");
	vi.resetModules();
	const { withBasePath } = await import("./base-path");

	expect(withBasePath("")).toBe("/ugoite/");
	expect(withBasePath("/")).toBe("/ugoite/");
	expect(withBasePath("/docs/spec/index")).toBe("/ugoite/docs/spec/index");
	expect(withBasePath("design/philosophy")).toBe("/ugoite/design/philosophy");
	expect(withBasePath("/ugoite")).toBe("/ugoite");
	expect(withBasePath("/ugoite/docs/spec/index")).toBe(
		"/ugoite/docs/spec/index",
	);
});
