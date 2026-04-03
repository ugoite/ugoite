export type OnboardingCard = {
	badge: string;
	description: string;
	href: string;
	icon: string;
	title: string;
};

export const browserPathCaveat = {
	badge: "Browser caveat today",
	description:
		"The current browser route still needs a running backend + frontend stack and an explicit login flow. The CLI in `core` mode is the thinnest local-first path right now.",
	headline:
		"The browser path is still server-backed and login-gated, even though the data stays local-first.",
} as const;

export const conceptPrimerCard = {
	badge: "Learn First",
	description:
		"Get the plain-language mental model for spaces, entries, forms, and search before choosing a surface.",
	href: "/docs/guide/concepts",
	icon: "💡",
	title: "Understand core concepts",
} as const satisfies OnboardingCard;

export const primaryStartCards = [
	{
		badge: "Fastest path",
		description:
			"Launch the released frontend + backend stack without cloning or building from source, then continue through the explicit browser login.",
		href: "/docs/guide/container-quickstart",
		icon: "🚀",
		title: "Try the published release",
	},
	{
		badge: "Contributor path",
		description:
			"Run the current workspace with mise run dev when you want the latest backend, frontend, and docsite together, then sign in explicitly at /login.",
		href: "/docs/guide/local-dev-auth-login",
		icon: "🛠️",
		title: "Run from source",
	},
	{
		badge: "Automation path",
		description:
			"Install the released CLI or use it from source when the terminal is your main surface.",
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
] as const satisfies readonly OnboardingCard[];
