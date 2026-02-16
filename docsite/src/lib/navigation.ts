export type NavItem = {
	title: string;
	href: string;
};

export type NavSection = {
	title: string;
	items: NavItem[];
};

export const topLinks: NavItem[] = [
	{ title: "Home", href: "/" },
	{ title: "Design", href: "/design" },
	{ title: "Application", href: "/app" },
	{ title: "Source Docs", href: "/docs/spec/index" },
];

export const navSections: NavSection[] = [
	{
		title: "Design Principles",
		items: [
			{ title: "Overview", href: "/design" },
			{ title: "Philosophy", href: "/design/philosophy" },
			{ title: "Policies", href: "/design/policies" },
			{ title: "Requirements", href: "/design/requirements" },
			{ title: "Features", href: "/design/features" },
			{ title: "Relation Map", href: "/design/relations" },
		],
	},
	{
		title: "Application",
		items: [
			{ title: "Overview", href: "/app" },
			{ title: "API & Storage", href: "/app/api-storage" },
			{ title: "API Catalog", href: "/app/api-storage/apis" },
			{ title: "Data Model", href: "/app/api-storage/data-model" },
			{ title: "CLI", href: "/app/cli" },
			{ title: "CLI Commands", href: "/app/cli/commands" },
			{ title: "Frontend", href: "/app/frontend" },
			{ title: "UI Pages", href: "/app/frontend/pages" },
		],
	},
	{
		title: "Source Docs",
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
