import { expect, test } from "vitest";

import {
	browserPathCaveat,
	conceptPrimerCard,
	nextStepCards,
	primaryStartCards,
} from "./onboarding";

test("REQ-E2E-008: onboarding content keeps try, source, and CLI paths as the first entry choices", () => {
	expect(primaryStartCards.map((card) => card.title)).toEqual([
		"Try the published release",
		"Run from source",
		"Use the CLI",
	]);
	expect(primaryStartCards.map((card) => card.href)).toEqual([
		"/docs/guide/container-quickstart",
		"/docs/guide/local-dev-auth-login",
		"/docs/guide/cli",
	]);
	expect(primaryStartCards.map((card) => card.badge)).toEqual([
		"Fastest path",
		"Contributor path",
		"Automation path",
	]);
});

test("REQ-E2E-008: onboarding content keeps browser, auth, and deeper reference docs available after the first step", () => {
	expect(nextStepCards.map((card) => card.title)).toEqual([
		"Explore the browser app",
		"Understand auth and access",
		"Read design and source docs",
	]);
	expect(nextStepCards.map((card) => card.href)).toEqual([
		"/app/frontend",
		"/docs/guide/auth-overview",
		"/docs/spec/index",
	]);
	expect(nextStepCards.map((card) => card.badge)).toEqual([
		"Browser",
		"Access",
		"Reference",
	]);
});

test("REQ-E2E-008: onboarding content keeps browser caveats explicit on browser-first paths", () => {
	expect(browserPathCaveat).toEqual({
		badge: "Browser caveat today",
		description:
			"The current browser route still needs a running backend + frontend stack and an explicit login flow. The CLI in `core` mode is the thinnest local-first path right now.",
		headline:
			"The browser path is still server-backed and login-gated, even though the data stays local-first.",
	});
	expect(primaryStartCards[0]?.description).toContain(
		"frontend + backend stack",
	);
	expect(primaryStartCards[0]?.description).toContain("explicit browser login");
	expect(primaryStartCards[1]?.description).toContain("mise run dev");
	expect(primaryStartCards[1]?.description).toContain("/login");
	expect(nextStepCards[0]?.description).toContain("completed login");
	expect(nextStepCards[0]?.description).toContain("create a form first");
});

test("REQ-E2E-008: onboarding content offers a concepts primer before deeper guides and references", () => {
	expect(conceptPrimerCard).toEqual({
		badge: "Learn First",
		description:
			"Get the plain-language mental model for spaces, entries, forms, and search before choosing a surface.",
		href: "/docs/guide/concepts",
		icon: "💡",
		title: "Understand core concepts",
	});
});
