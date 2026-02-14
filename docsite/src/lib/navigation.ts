export type NavItem = {
	title: string;
	href: string;
	description?: string;
};

export type NavSection = {
	title: string;
	items: NavItem[];
};

export const topLinks: NavItem[] = [
	{ title: "Overview", href: "/" },
	{ title: "Specification Index", href: "/docs/spec/index" },
	{ title: "Tasks", href: "/docs/tasks/tasks" },
	{ title: "CLI Guide", href: "/docs/guide/cli" },
];

export const navSections: NavSection[] = [
	{
		title: "Getting Started",
		items: [
			{ title: "README", href: "/docs/README" },
			{ title: "CLI Guide", href: "/docs/guide/cli" },
		],
	},
	{
		title: "Specifications",
		items: [
			{ title: "Spec Index", href: "/docs/spec/index" },
			{ title: "Architecture", href: "/docs/spec/architecture/overview" },
			{ title: "Data Model", href: "/docs/spec/data-model/overview" },
			{ title: "UI Specs", href: "/docs/spec/ui/README" },
		],
	},
	{
		title: "APIs",
		items: [
			{ title: "REST API", href: "/docs/spec/api/rest" },
			{ title: "MCP API", href: "/docs/spec/api/mcp" },
			{ title: "OpenAPI", href: "/docs/spec/api/openapi" },
		],
	},
	{
		title: "Requirements",
		items: [
			{ title: "Requirements Overview", href: "/docs/spec/requirements/README" },
			{ title: "Frontend Requirements", href: "/docs/spec/requirements/frontend" },
			{ title: "API Requirements", href: "/docs/spec/requirements/api" },
		],
	},
];
