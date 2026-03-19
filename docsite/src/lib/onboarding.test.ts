import { expect, test } from "vitest";

import { nextStepCards, primaryStartCards } from "./onboarding";

test("REQ-E2E-008: onboarding content keeps try, source, and CLI paths as the first entry choices", () => {
	expect(primaryStartCards.map((card) => card.title)).toEqual([
		"Try the published release",
		"Run from source",
		"Use the CLI",
	]);
	expect(primaryStartCards.map((card) => card.href)).toEqual([
		"/docs/guide/container-quickstart",
		"/docs/guide/docker-compose",
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
