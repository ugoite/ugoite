import { afterEach, expect, test, vi } from "vitest";

const specDataMocks = vi.hoisted(() => ({
	getPhilosophies: vi.fn(),
	getPoliciesDetailed: vi.fn(),
	getUiPages: vi.fn(),
}));

vi.mock("./spec-data", () => specDataMocks);

import {
	getNewcomerNavSections,
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
	const designSection = sections.find(
		(section) => section.title === "Design Principles",
	);
	const applicationSection = sections.find(
		(section) => section.title === "Application",
	);
	const philosophyItems = designSection?.items.find(
		(item) => item.href === "/design/philosophy",
	)?.items;
	const policyItems = designSection?.items.find(
		(item) => item.href === "/design/policies",
	)?.items;
	const uiPageItems = applicationSection?.items.find(
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
		navSections
			.find((section) => section.title === "Design Principles")
			?.items.find((item) => item.href === "/design/philosophy")?.items,
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

	const designSection = navSections.find(
		(section) => section.title === "Design Principles",
	);
	const appSection = navSections.find(
		(section) => section.title === "Application",
	);
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
	const hydratedDesignSection = sections.find(
		(section) => section.title === "Design Principles",
	);
	const hydratedAppSection = sections.find(
		(section) => section.title === "Application",
	);

	expect(hydratedDesignSection?.items.map((item) => item.href)).not.toContain(
		"/design/philosophy",
	);
	expect(hydratedDesignSection?.items.map((item) => item.href)).not.toContain(
		"/design/policies",
	);
	expect(hydratedAppSection?.items.map((item) => item.href)).not.toContain(
		"/app/frontend/pages",
	);
	expect(specDataMocks.getPhilosophies).toHaveBeenCalledTimes(1);
	expect(specDataMocks.getPoliciesDetailed).toHaveBeenCalledTimes(1);
	expect(specDataMocks.getUiPages).toHaveBeenCalledTimes(1);
});

test("REQ-E2E-008: newcomer navigation limits deep sections to getting-started content", () => {
	expect(getNewcomerNavSections()).toEqual([
		{
			title: "Getting Started",
			overviewHref: "/getting-started",
			items: [
				{ title: "Overview", href: "/getting-started" },
				{
					title: "Core Concepts",
					href: "/docs/guide/concepts",
				},
				{
					title: "Container Quickstart",
					href: "/docs/guide/container-quickstart",
				},
				{ title: "Docker Compose", href: "/docs/guide/docker-compose" },
				{ title: "Auth Overview", href: "/docs/guide/auth-overview" },
				{ title: "CLI Guide", href: "/docs/guide/cli" },
			],
		},
	]);
	expect(navSections).toEqual(originalNavSections);
});
