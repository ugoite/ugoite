import { getPhilosophies, getPoliciesDetailed, getUiPages } from "./spec-data";

export type NavItem = {
	title: string;
	href: string;
	items?: NavItem[];
};

export type NavSection = {
	title: string;
	overviewHref: string;
	items: NavItem[];
};

export const topLinks: NavItem[] = [
	{ title: "Home", href: "/" },
	{ title: "Getting Started", href: "/getting-started" },
	{ title: "Application", href: "/app" },
	{ title: "Design", href: "/design" },
	{ title: "Source Docs", href: "/docs/spec/index" },
];

export const navSections: NavSection[] = [
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
	{
		title: "Application",
		overviewHref: "/app",
		items: [
			{ title: "Overview", href: "/app" },
			{ title: "Frontend", href: "/app/frontend" },
			{ title: "UI Pages", href: "/app/frontend/pages", items: [] },
			{ title: "API & Storage", href: "/app/api-storage" },
			{ title: "CLI", href: "/app/cli" },
			{ title: "CLI Commands", href: "/app/cli/commands" },
			{ title: "Data Model", href: "/app/api-storage/data-model" },
			{ title: "API Catalog", href: "/app/api-storage/apis" },
		],
	},
	{
		title: "Design Principles",
		overviewHref: "/design",
		items: [
			{ title: "Overview", href: "/design" },
			{ title: "Philosophy", href: "/design/philosophy", items: [] },
			{ title: "Policies", href: "/design/policies", items: [] },
			{ title: "Requirements", href: "/design/requirements" },
			{ title: "Features", href: "/design/features" },
			{ title: "Relation Map", href: "/design/relations" },
		],
	},
	{
		title: "Source Docs",
		overviewHref: "/docs/spec/index",
		items: [
			{ title: "Spec Index", href: "/docs/spec/index" },
			{ title: "REST API", href: "/docs/spec/api/rest" },
			{ title: "MCP API", href: "/docs/spec/api/mcp" },
			{ title: "OpenAPI", href: "/docs/spec/api/openapi" },
			{ title: "UI Spec", href: "/docs/spec/ui/README" },
		],
	},
];

export function titleFromSegment(segment: string): string {
	return segment
		.replaceAll("-", " ")
		.replace(/\b\w/g, (char) => char.toUpperCase());
}

export async function getNavSectionsWithChildren(): Promise<NavSection[]> {
	const [philosophies, policies, uiPages] = await Promise.all([
		getPhilosophies(),
		getPoliciesDetailed(),
		getUiPages(),
	]);

	const sections = structuredClone(navSections);

	const design = sections.find(
		(section) => section.title === "Design Principles",
	);
	const app = sections.find((section) => section.title === "Application");

	const philosophyItem = design?.items.find(
		(item) => item.href === "/design/philosophy",
	);
	if (philosophyItem) {
		philosophyItem.items = philosophies.map((item) => ({
			title: item.id,
			href: `/design/philosophy/${item.id.toLowerCase()}`,
		}));
	}

	const policyItem = design?.items.find(
		(item) => item.href === "/design/policies",
	);
	if (policyItem) {
		policyItem.items = policies.map((item) => ({
			title: item.id,
			href: `/design/policies/${item.id.toLowerCase()}`,
		}));
	}

	const uiPagesItem = app?.items.find(
		(item) => item.href === "/app/frontend/pages",
	);
	if (uiPagesItem) {
		uiPagesItem.items = uiPages.map((item) => ({
			title: item.id,
			href: `/app/frontend/pages/${item.id}`,
		}));
	}

	return sections;
}
