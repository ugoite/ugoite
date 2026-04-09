export type OnboardingCard = {
	badge: string;
	description: string;
	href: string;
	icon: string;
	title: string;
};

export type ConceptSummary = {
	description: string;
	title: string;
};

export const landingLead =
	"A private, portable knowledge space you can keep on infrastructure you control: CLI `core` mode is the lowest-setup local-first path today, while the current browser route still runs on the backend + frontend stack you launch with Docker or `mise run dev`.";

export const browserPathCaveat = {
	badge: "Browser caveat today",
	description:
		"The current browser route still needs a running backend + frontend stack and an explicit login flow. It also costs more setup than the CLI in `core` mode, which is still the lowest-setup-cost local-first path right now.",
	headline:
		"The browser path is still server-backed and login-gated, even though the data stays local-first. It also has higher setup cost than CLI `core` mode.",
} as const;

export const conceptPrimerCard = {
	badge: "Concept primer",
	description:
		"Get the plain-language mental model for spaces, entries, forms, search, and surface choice after you pick a path and before you go deeper into auth or specs.",
	href: "/docs/guide/concepts",
	icon: "💡",
	title: "Understand core concepts",
} as const satisfies OnboardingCard;

export const coreConceptSummaries = [
	{
		description:
			"A portable workspace that owns its entries, forms, assets, settings, and derived indexes for one project, team, or knowledge base.",
		title: "Space",
	},
	{
		description:
			"One Markdown-backed record inside a space, such as a note, task, meeting log, or person page.",
		title: "Entry",
	},
	{
		description:
			"The schema and template for an entry type, so extracted fields stay predictable and queryable.",
		title: "Form",
	},
	{
		description:
			"You write Markdown first, forms extract typed fields when you want structure, and search/indexes are derived from that source data.",
		title: "Markdown, extraction, and search",
	},
	{
		description:
			"Use the browser for guided exploration, the CLI for the thinnest local-first automation path, and backend/API surfaces when you intentionally want server-backed behavior.",
		title: "Browser, CLI, and API",
	},
] as const satisfies readonly ConceptSummary[];

export const primaryStartCards = [
	{
		badge: "Fastest browser path",
		description:
			"Launch the released frontend + backend stack with Docker and published image pulls, then continue through the explicit browser login.",
		href: "/docs/guide/container-quickstart",
		icon: "🚀",
		title: "Try the published release",
	},
	{
		badge: "Highest setup cost",
		description:
			"Run the current workspace with mise run dev when you want the latest backend, frontend, and docsite together, then sign in explicitly at /login.",
		href: "/docs/guide/local-dev-auth-login",
		icon: "🛠️",
		title: "Run from source",
	},
	{
		badge: "Lowest setup cost",
		description:
			"Install the released CLI or use it from source when the terminal is your main surface and you want to avoid container infrastructure.",
		href: "/docs/guide/cli",
		icon: "⌨️",
		title: "Use the CLI",
	},
] as const satisfies readonly OnboardingCard[];

export const nextStepCards = [
	{
		badge: "Browser",
		description:
			"After the stack is running and you have completed login, open a space, create a form first, then add entries and explore search from that shared structure.",
		href: "/app/frontend",
		icon: "🖥️",
		title: "Explore the browser app",
	},
	{
		badge: "Access",
		description:
			"Review browser, CLI, and API sign-in flows before rollout or scripting.",
		href: "/docs/guide/auth-overview",
		icon: "🔐",
		title: "Understand auth and access",
	},
	{
		badge: "Reference",
		description:
			"Go deeper into philosophy, requirements, APIs, and machine-readable specs when you need the full contract.",
		href: "/docs/spec/index",
		icon: "📚",
		title: "Read design and source docs",
	},
	{
		badge: "Ops",
		description:
			"Open the operational guide hub for health checks, env vars, deployment, log redaction, storage cleanup, migration, and unauthorized-space fixes.",
		href: "/docs/guide/operations",
		icon: "🧭",
		title: "Run and troubleshoot the stack",
	},
] as const satisfies readonly OnboardingCard[];
