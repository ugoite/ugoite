// REQ-FE-010: Link store (removed stub)
import { describe, it, expect } from "vitest";
import { createRoot } from "solid-js";
import { createLinkStore } from "./link-store";

describe("createLinkStore", () => {
	it("initializes with links removed error message", () => {
		createRoot((dispose) => {
			const store = createLinkStore(() => "ws");
			expect(store.links()).toEqual([]);
			expect(store.loading()).toBe(false);
			expect(store.error()).toContain("Links API has been removed");
			dispose();
		});
	});

	it("loadLinks sets empty links and error", async () => {
		await createRoot(async (dispose) => {
			const store = createLinkStore(() => "ws");
			await store.loadLinks();
			expect(store.links()).toEqual([]);
			expect(store.error()).toContain("Links API has been removed");
			dispose();
		});
	});

	it("createLink throws error", async () => {
		await createRoot(async (dispose) => {
			const store = createLinkStore(() => "ws");
			await expect(store.createLink({ source: "a", target: "b", kind: "ref" })).rejects.toThrow(
				"Links API has been removed",
			);
			dispose();
		});
	});

	it("deleteLink throws error", async () => {
		await createRoot(async (dispose) => {
			const store = createLinkStore(() => "ws");
			await expect(store.deleteLink("link-1")).rejects.toThrow("Links API has been removed");
			dispose();
		});
	});
});
