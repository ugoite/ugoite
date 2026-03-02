// REQ-FE-010: Entries route context
import { describe, it, expect } from "vitest";
import { createRoot } from "solid-js";
import { EntriesRouteContext, useEntriesRouteContext } from "./entries-route-context";

describe("useEntriesRouteContext", () => {
	it("throws when called outside provider", () => {
		createRoot((dispose) => {
			expect(() => useEntriesRouteContext()).toThrow("EntriesRouteContext is missing");
			dispose();
		});
	});

	it("exports EntriesRouteContext", () => {
		expect(EntriesRouteContext).toBeDefined();
	});
});
