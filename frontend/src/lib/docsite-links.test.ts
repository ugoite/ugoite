import { afterEach, describe, expect, it, vi } from "vitest";
import { getDocsiteHref } from "./docsite-links";

afterEach(() => {
	vi.unstubAllGlobals();
});

describe("docsite links", () => {
	it("REQ-FE-056: auth guidance defaults to the published docsite outside development", () => {
		expect(
			getDocsiteHref("/docs/guide/local-dev-auth-login", "docs/guide/local-dev-auth-login.md", {
				NODE_ENV: "test",
			}),
		).toBe("https://ugoite.github.io/ugoite/docs/guide/local-dev-auth-login");
	});

	it("REQ-FE-056: auth guidance uses the local docsite origin during development", () => {
		expect(
			getDocsiteHref("docs/guide/container-quickstart", "docs/guide/container-quickstart.md", {
				NODE_ENV: "development",
			}),
		).toBe("http://localhost:4321/docs/guide/container-quickstart");
	});

	it("REQ-FE-017: doc links honor explicit docsite origins and GitHub fallbacks when docsite is unavailable", () => {
		expect(
			getDocsiteHref("/docs/guide/storage-migration", "docs/guide/storage-migration.md", {
				DOCSITE_ORIGIN: "https://docs.example.test/base/",
			}),
		).toBe("https://docs.example.test/base/docs/guide/storage-migration");
		expect(
			getDocsiteHref("/docs/guide/storage-migration", "docs/guide/storage-migration.md", {
				DOCSITE_ORIGIN: "",
			}),
		).toBe("https://github.com/ugoite/ugoite/blob/main/docs/guide/storage-migration.md");
		expect(
			getDocsiteHref("docs/guide/storage-migration", undefined, {
				DOCSITE_ORIGIN: "",
			}),
		).toBe("/docs/guide/storage-migration");
	});

	it("REQ-FE-056: auth guidance still resolves when process env is unavailable", () => {
		vi.stubGlobal("process", undefined);
		expect(getDocsiteHref("/docs/guide/local-dev-auth-login")).toBe(
			"https://ugoite.github.io/ugoite/docs/guide/local-dev-auth-login",
		);
	});
});
