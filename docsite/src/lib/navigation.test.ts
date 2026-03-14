import { afterEach, expect, test, vi } from "vitest";

const specDataMocks = vi.hoisted(() => ({
	getPhilosophies: vi.fn(),
	getPoliciesDetailed: vi.fn(),
	getUiPages: vi.fn(),
}));

vi.mock("./spec-data", () => specDataMocks);

import {
	getNavSectionsWithChildren,
	navSections,
	titleFromSegment,
	topLinks,
} from "./navigation";

const originalNavSections = structuredClone(navSections);

afterEach(() => {
	specDataMocks.getPhilosophies.mockReset();
	specDataMocks.getPoliciesDetailed.mockReset();
	specDataMocks.getUiPages.mockReset();
	navSections.splice(
		0,
		navSections.length,
		...structuredClone(originalNavSections),
	);
});

test("REQ-E2E-006: navigation helpers build child docsite links from spec data", async () => {
	specDataMocks.getPhilosophies.mockResolvedValue([
		{ id: "POL-002", title: "Second philosophy" },
	]);
	specDataMocks.getPoliciesDetailed.mockResolvedValue([
		{ id: "POL-010", title: "Policy detail" },
	]);
	specDataMocks.getUiPages.mockResolvedValue([
		{ id: "space-dashboard", title: "Dashboard", route: "/dashboard" },
	]);

	expect(titleFromSegment("local-first-ai")).toBe("Local First Ai");
	expect(topLinks.map((item) => item.href)).toContain("/docs/spec/index");

	const sections = await getNavSectionsWithChildren();
	const philosophyItems = sections[0]?.items.find(
		(item) => item.href === "/design/philosophy",
	)?.items;
	const policyItems = sections[0]?.items.find(
		(item) => item.href === "/design/policies",
	)?.items;
	const uiPageItems = sections[1]?.items.find(
		(item) => item.href === "/app/frontend/pages",
	)?.items;

	expect(philosophyItems).toEqual([
		{ title: "POL-002", href: "/design/philosophy/pol-002" },
	]);
	expect(policyItems).toEqual([
		{ title: "POL-010", href: "/design/policies/pol-010" },
	]);
	expect(uiPageItems).toEqual([
		{ title: "space-dashboard", href: "/app/frontend/pages/space-dashboard" },
	]);
	expect(
		navSections[0]?.items.find((item) => item.href === "/design/philosophy")
			?.items,
	).toEqual([]);
});

test("REQ-E2E-006: navigation helpers tolerate missing child anchors without mutating shared nav state", async () => {
	specDataMocks.getPhilosophies.mockResolvedValue([
		{ id: "POL-002", title: "Second philosophy" },
	]);
	specDataMocks.getPoliciesDetailed.mockResolvedValue([
		{ id: "POL-010", title: "Policy detail" },
	]);
	specDataMocks.getUiPages.mockResolvedValue([
		{ id: "space-dashboard", title: "Dashboard", route: "/dashboard" },
	]);

	const designSection = navSections[0];
	const appSection = navSections[1];
	if (!designSection || !appSection) {
		throw new Error("Expected default docsite navigation sections");
	}

	designSection.items = designSection.items.filter(
		(item) =>
			item.href !== "/design/philosophy" && item.href !== "/design/policies",
	);
	appSection.items = appSection.items.filter(
		(item) => item.href !== "/app/frontend/pages",
	);

	const sections = await getNavSectionsWithChildren();

	expect(sections[0]?.items.map((item) => item.href)).not.toContain(
		"/design/philosophy",
	);
	expect(sections[0]?.items.map((item) => item.href)).not.toContain(
		"/design/policies",
	);
	expect(sections[1]?.items.map((item) => item.href)).not.toContain(
		"/app/frontend/pages",
	);
	expect(specDataMocks.getPhilosophies).toHaveBeenCalledTimes(1);
	expect(specDataMocks.getPoliciesDetailed).toHaveBeenCalledTimes(1);
	expect(specDataMocks.getUiPages).toHaveBeenCalledTimes(1);
});
