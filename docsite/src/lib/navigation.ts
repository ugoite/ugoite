export type NavItem = {
	title: string;
	href: string;
	description?: string;
};

export type NavSection = {
	id: string;
	title: string;
	description?: string;
	items: NavItem[];
};

export const topLinks: NavItem[] = [
	{ title: "Home", href: "/" },
	{ title: "Specification Index", href: "/docs/spec/index" },
	{ title: "Requirements", href: "/docs/spec/requirements/README" },
	{ title: "REST API", href: "/docs/spec/api/rest" },
	{ title: "Tasks", href: "/docs/tasks/tasks" },
];

export const navSections: NavSection[] = [
	{
		id: "getting-started",
		title: "Getting Started",
		description: "Entry points for setup, operation, and contribution.",
		items: [
			{ title: "README", href: "/docs/README" },
			{ title: "CLI Guide", href: "/docs/guide/cli" },
			{ title: "Tasks", href: "/docs/tasks/tasks" },
			{ title: "Roadmap", href: "/docs/tasks/roadmap" },
		],
	},
	{
		id: "architecture",
		title: "Architecture & Design",
		description: "Core boundaries, rationale, and long-term direction.",
		items: [
			{ title: "Architecture Overview", href: "/docs/spec/architecture/overview" },
			{ title: "Technology Stack", href: "/docs/spec/architecture/stack" },
			{ title: "Architecture Decisions", href: "/docs/spec/architecture/decisions" },
			{
				title: "Frontend-Backend Interface",
				href: "/docs/spec/architecture/frontend-backend-interface",
			},
			{ title: "Future-Proofing", href: "/docs/spec/architecture/future-proofing" },
		],
	},
	{
		id: "features",
		title: "Features & Stories",
		description: "Feature registry and user-scenario coverage.",
		items: [
			{ title: "Features Registry", href: "/docs/spec/features/README" },
			{ title: "Ugoite SQL", href: "/docs/spec/features/sql" },
			{ title: "Core Stories", href: "/docs/spec/stories/core" },
			{ title: "Advanced Stories", href: "/docs/spec/stories/advanced" },
		],
	},
	{
		id: "data-api",
		title: "Data & API",
		description: "Storage model and external interaction contracts.",
		items: [
			{ title: "Data Model Overview", href: "/docs/spec/data-model/overview" },
			{ title: "Directory Structure", href: "/docs/spec/data-model/directory-structure" },
			{ title: "SQL Sessions", href: "/docs/spec/data-model/sql-sessions" },
			{ title: "REST API", href: "/docs/spec/api/rest" },
			{ title: "MCP API", href: "/docs/spec/api/mcp" },
			{ title: "OpenAPI", href: "/docs/spec/api/openapi" },
		],
	},
	{
		id: "requirements",
		title: "Requirements & Governance",
		description: "Traceability, taxonomy, and machine-readable contracts.",
		items: [
			{ title: "Requirements Overview", href: "/docs/spec/requirements/README" },
			{ title: "API Requirements", href: "/docs/spec/requirements/api" },
			{ title: "Frontend Requirements", href: "/docs/spec/requirements/frontend" },
			{ title: "Specifications Catalog", href: "/docs/spec/specifications" },
			{ title: "Policies", href: "/docs/spec/policies/policies" },
			{ title: "Philosophy", href: "/docs/spec/philosophy/foundation" },
		],
		},
	{
		id: "quality",
		title: "Security & Quality",
		description: "Security posture, verification, and resilience practices.",
		items: [
			{ title: "Security Overview", href: "/docs/spec/security/overview" },
			{ title: "Testing Strategy", href: "/docs/spec/testing/strategy" },
			{ title: "CI/CD", href: "/docs/spec/testing/ci-cd" },
			{ title: "Error Handling", href: "/docs/spec/quality/error-handling" },
			{ title: "Success Metrics", href: "/docs/spec/product/success-metrics" },
			{ title: "CLI Guide", href: "/docs/guide/cli" },
		],
	},
];

export const homeHeroLinks: NavItem[] = [
	{ title: "Read Spec Index", href: "/docs/spec/index" },
	{ title: "Review REST API", href: "/docs/spec/api/rest" },
	{ title: "Track Requirements", href: "/docs/spec/requirements/README" },
];

export const homeCapabilityCards: Array<{
	title: string;
	summary: string;
	href: string;
	tag: string;
}> = [
	{
		title: "Architecture First",
		summary: "Understand module boundaries and responsibility contracts before coding.",
		href: "/docs/spec/architecture/overview",
		tag: "Core",
	},
	{
		title: "API Contract Clarity",
		summary: "Jump from user stories to REST and MCP contracts in one flow.",
		href: "/docs/spec/api/rest",
		tag: "Interface",
	},
	{
		title: "Requirement Traceability",
		summary: "Track REQ-* items with test mappings and governance metadata.",
		href: "/docs/spec/requirements/README",
		tag: "Quality",
	},
	{
		title: "Data Model Grounding",
		summary: "Model markdown fields, forms, and SQL sessions with concrete specs.",
		href: "/docs/spec/data-model/overview",
		tag: "Storage",
	},
];
