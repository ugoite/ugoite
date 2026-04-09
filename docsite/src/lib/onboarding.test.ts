import { expect, test } from "vitest";

import {
	browserPathCaveat,
	conceptPrimerCard,
	coreConceptSummaries,
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
		"Fastest browser path",
		"Highest setup cost",
		"Lowest setup cost",
	]);
});

test("REQ-E2E-008: onboarding content keeps browser, auth, and deeper reference docs available after the first step", () => {
	expect(nextStepCards.map((card) => card.title)).toEqual([
		"Create your first space, form, and entry",
		"Understand auth and access",
		"Read design and source docs",
		"Run and troubleshoot the stack",
	]);
	expect(nextStepCards.map((card) => card.href)).toEqual([
		"/docs/guide/browser-first-entry",
		"/docs/guide/auth-overview",
		"/docs/spec/index",
		"/docs/guide/operations",
	]);
	expect(nextStepCards.map((card) => card.badge)).toEqual([
		"Browser walkthrough",
		"Access",
		"Reference",
		"Ops",
	]);
});

test("REQ-E2E-008: onboarding content keeps browser caveats explicit on browser-first paths", () => {
	expect(browserPathCaveat).toEqual({
		badge: "Browser caveat today",
		description:
			"The current browser route still needs a running backend + frontend stack and an explicit login flow. It also costs more setup than the CLI in `core` mode, which is still the lowest-setup-cost local-first path right now.",
		headline:
			"The browser path is still server-backed and login-gated, even though the data stays local-first. It also has higher setup cost than CLI `core` mode.",
	});
	expect(primaryStartCards[0]?.description).toContain(
		"frontend + backend stack",
	);
	expect(primaryStartCards[0]?.description).toContain("Docker");
	expect(primaryStartCards[0]?.description).toContain("published image pulls");
	expect(primaryStartCards[1]?.description).toContain("mise run dev");
	expect(primaryStartCards[1]?.description).toContain("/login");
	expect(primaryStartCards[2]?.description).toContain(
		"avoid container infrastructure",
	);
	expect(nextStepCards[0]?.description).toContain("After login");
	expect(nextStepCards[3]?.description).toContain("health checks");
	expect(nextStepCards[0]?.description).toContain("starter entry");
	expect(nextStepCards[0]?.description).toContain("browser surface");
});

test("REQ-E2E-008: onboarding content keeps the browser next step aligned to the walkthrough", () => {
	expect(nextStepCards[0]).toMatchObject({
		badge: "Browser walkthrough",
		href: "/docs/guide/browser-first-entry",
	});
	expect(nextStepCards[0]?.description).toContain("starter entry");
	expect(nextStepCards[0]?.description).toContain("After login");
	expect(nextStepCards[0]?.description).not.toContain("create a form first");
});

test("REQ-E2E-008: onboarding content offers a concepts primer before deeper guides and references", () => {
	expect(conceptPrimerCard).toEqual({
		badge: "Concept primer",
		description:
			"Get the plain-language mental model for spaces, entries, forms, search, and surface choice after you pick a path and before you go deeper into auth or specs.",
		href: "/docs/guide/concepts",
		icon: "💡",
		title: "Understand core concepts",
	});
});

test("REQ-E2E-008: onboarding content keeps core concepts ready for the follow-up primer", () => {
	expect(coreConceptSummaries.map((concept) => concept.title)).toEqual([
		"Space",
		"Entry",
		"Form",
		"Markdown, extraction, and search",
		"Browser, CLI, and API",
	]);
	expect(coreConceptSummaries[0]?.description).toContain("portable workspace");
	expect(coreConceptSummaries[1]?.description).toContain(
		"Markdown-backed record",
	);
	expect(coreConceptSummaries[2]?.description).toContain("schema and template");
	expect(coreConceptSummaries[3]?.description).toContain(
		"search/indexes are derived",
	);
	expect(coreConceptSummaries[4]?.description).toContain(
		"thinnest local-first automation path",
	);
});
