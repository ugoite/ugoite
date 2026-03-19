export type OnboardingCard = {
	badge: string;
	description: string;
	href: string;
	icon: string;
	title: string;
};

export const primaryStartCards = [
	{
		badge: "Fastest path",
		description:
			"Launch the released browser stack without cloning or building from source.",
		href: "/docs/guide/container-quickstart",
		icon: "🚀",
		title: "Try the published release",
	},
	{
		badge: "Contributor path",
		description:
			"Run the current workspace when you want the latest backend, frontend, and docsite together.",
		href: "/docs/guide/docker-compose",
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
			"See how spaces, entries, forms, and search fit together in the UI.",
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
