import path from "node:path";
import { afterEach, expect, test, vi } from "vitest";

const fsMocks = vi.hoisted(() => ({
	readFile: vi.fn(),
	readdir: vi.fn(),
}));

vi.mock("node:fs", async (importOriginal) => {
	const actual = await importOriginal<typeof import("node:fs")>();
	return {
		...actual,
		promises: {
			...actual.promises,
			readFile: fsMocks.readFile,
			readdir: fsMocks.readdir,
		},
	};
});

import {
	getDocsRoot,
	getFeatureGroups,
	getFeatureKindsLinkedToRequirements,
	getPhilosophies,
	getPhilosophyById,
	getPolicies,
	getPoliciesDetailed,
	getPolicyById,
	getRequirementGroups,
	getRequirementToFeatureEdges,
	getSpecifications,
	getSpecRoot,
	getUiPageById,
	getUiPages,
} from "./spec-data";

type MockFsConfig = {
	directories?: Record<string, string[]>;
	files?: Record<string, string>;
};

const specRoot = getSpecRoot();
const requirementDir = path.join(specRoot, "requirements");
const featuresDir = path.join(specRoot, "features");
const uiPagesDir = path.join(specRoot, "ui/pages");

function normalizePath(absolutePath: string): string {
	return absolutePath.replaceAll("\\", "/");
}

function useMockedFs({ directories = {}, files = {} }: MockFsConfig): void {
	const normalizedDirectories = new Map(
		Object.entries(directories).map(([absolutePath, entries]) => [
			normalizePath(absolutePath),
			entries,
		]),
	);
	const normalizedFiles = new Map(
		Object.entries(files).map(([absolutePath, contents]) => [
			normalizePath(absolutePath),
			contents,
		]),
	);

	fsMocks.readdir.mockImplementation(async (absolutePath: string) => {
		const entries = normalizedDirectories.get(
			normalizePath(String(absolutePath)),
		);
		if (entries === undefined) {
			throw new Error(`Unexpected readdir: ${absolutePath}`);
		}
		return entries;
	});
	fsMocks.readFile.mockImplementation(async (absolutePath: string) => {
		const contents = normalizedFiles.get(normalizePath(String(absolutePath)));
		if (contents === undefined) {
			throw new Error(`Unexpected readFile: ${absolutePath}`);
		}
		return contents;
	});
}

afterEach(() => {
	fsMocks.readFile.mockReset();
	fsMocks.readdir.mockReset();
});

test("REQ-OPS-003: philosophy and policy helpers normalize governance YAML data", async () => {
	const foundationPath = path.join(specRoot, "philosophy", "foundation.yaml");
	const policiesPath = path.join(specRoot, "policies", "policies.yaml");

	useMockedFs({
		files: {
			[foundationPath]: `philosophies:
  - id: POL-10
    title: Later philosophy
    statement: "  later summary  "
    linked_policies:
      - POL-9
  - id: POL-2
    title: Earlier philosophy
    product_design_principle: "  core design principle  "
    coding_guideline: "  keep it simple  "
`,
			[policiesPath]: `policies:
  - id: POL-2
    title: Zebra Policy
    summary: zebra summary
    description: zebra description
    linked_philosophies:
      - POL-2
    linked_requirements:
      - REQ-1
    linked_specifications:
      - SPEC-1
  - id: POL-1
    title: Alpha Policy
    summary: alpha summary
`,
		},
	});

	expect(await getPhilosophies()).toEqual([
		{
			id: "POL-2",
			title: "Earlier philosophy",
			summary: "core design principle",
			productDesignPrinciple: "core design principle",
			codingGuideline: "keep it simple",
			linkedPolicies: [],
		},
		{
			id: "POL-10",
			title: "Later philosophy",
			summary: "later summary",
			productDesignPrinciple: undefined,
			codingGuideline: undefined,
			linkedPolicies: ["POL-9"],
		},
	]);
	expect(await getPhilosophyById("POL-10")).toMatchObject({
		id: "POL-10",
		title: "Later philosophy",
	});
	expect(await getPhilosophyById("POL-404")).toBeNull();

	expect(await getPolicies()).toEqual([
		{ id: "POL-1", title: "Alpha Policy", summary: "alpha summary" },
		{ id: "POL-2", title: "Zebra Policy", summary: "zebra summary" },
	]);
	expect(await getPolicyById("POL-1")).toEqual({
		id: "POL-1",
		title: "Alpha Policy",
		summary: "alpha summary",
	});
	expect(await getPolicyById("POL-404")).toBeNull();

	expect(await getPoliciesDetailed()).toEqual([
		{
			id: "POL-2",
			title: "Zebra Policy",
			summary: "zebra summary",
			description: "zebra description",
			linkedPhilosophies: ["POL-2"],
			linkedRequirements: ["REQ-1"],
			linkedSpecifications: ["SPEC-1"],
		},
		{
			id: "POL-1",
			title: "Alpha Policy",
			summary: "alpha summary",
			description: undefined,
			linkedPhilosophies: [],
			linkedRequirements: [],
			linkedSpecifications: [],
		},
	]);

	useMockedFs({
		files: {
			[foundationPath]: "{}",
			[policiesPath]: "{}",
		},
	});

	expect(await getPhilosophies()).toEqual([]);
	expect(await getPolicies()).toEqual([]);
	expect(await getPoliciesDetailed()).toEqual([]);
});

test("REQ-OPS-003: requirement and feature helpers sort governance sources predictably", async () => {
	const alphaRequirementsPath = path.join(requirementDir, "alpha.yaml");
	const betaRequirementsPath = path.join(requirementDir, "beta.yaml");
	const emptyRequirementsPath = path.join(requirementDir, "empty.yaml");
	const featureRegistryPath = path.join(featuresDir, "features.yaml");
	const linksFeaturePath = path.join(featuresDir, "links.yaml");
	const fallbackFeaturePath = path.join(featuresDir, "fallback.yaml");
	const emptyFeaturePath = path.join(featuresDir, "empty.yaml");

	useMockedFs({
		directories: {
			[requirementDir]: ["beta.yaml", "notes.txt", "empty.yaml", "alpha.yaml"],
		},
		files: {
			[alphaRequirementsPath]: `requirements:
  - set_id: REQCAT-A
    source_file: requirements/alpha.yaml
    id: RID-A-001
    title: Gamma requirement
`,
			[betaRequirementsPath]: `requirements:
  - set_id: REQCAT-B
    source_file: requirements/beta.yaml
    scope: Beta scope
    id: RID-B-002
    title: Zebra requirement
    priority: high
    status: implemented
  - set_id: REQCAT-B
    source_file: requirements/beta.yaml
    scope: Beta scope
    id: RID-B-001
    title: Alpha requirement
`,
			[emptyRequirementsPath]: "{}",
			[featureRegistryPath]: `files:
  - kind: links
    file: links.yaml
    linked_requirements:
      - REQ-1
  - kind: fallback
    file: fallback.yaml
    linked_requirements:
      - REQ-2
  - kind: empty
    file: empty.yaml
`,
			[linksFeaturePath]: `kind: entries
apis:
  - id: entry.list
    method: GET
    backend:
      path: /entries
      file: backend.py
      function: list_entries
    ugoite_core:
      file: core.rs
      function: list_entries
`,
			[fallbackFeaturePath]: `apis:
  - id: fallback.list
    method: GET
    backend:
      path: /fallback
      file: backend.py
      function: list_fallback
    ugoite_core:
      file: core.rs
      function: list_fallback
`,
			[emptyFeaturePath]: "kind: empty",
		},
	});

	expect(await getRequirementGroups()).toEqual([
		{
			setId: "REQCAT-A",
			sourceFile: "requirements/alpha.yaml",
			scope: "",
			requirements: [
				{
					id: "RID-A-001",
					title: "Gamma requirement",
					priority: undefined,
					status: undefined,
				},
			],
		},
		{
			setId: "REQCAT-B",
			sourceFile: "requirements/beta.yaml",
			scope: "Beta scope",
			requirements: [
				{
					id: "RID-B-001",
					title: "Alpha requirement",
					priority: undefined,
					status: undefined,
				},
				{
					id: "RID-B-002",
					title: "Zebra requirement",
					priority: "high",
					status: "implemented",
				},
			],
		},
	]);
	expect(await getFeatureGroups()).toEqual([
		{ kind: "empty", file: "empty.yaml", apis: [], linkedRequirements: [] },
		{
			kind: "entries",
			file: "links.yaml",
			apis: [
				{
					id: "entry.list",
					method: "GET",
					backend: {
						path: "/entries",
						file: "backend.py",
						function: "list_entries",
					},
					ugoite_core: {
						file: "core.rs",
						function: "list_entries",
					},
				},
			],
			linkedRequirements: ["REQ-1"],
		},
		{
			kind: "fallback",
			file: "fallback.yaml",
			apis: [
				{
					id: "fallback.list",
					method: "GET",
					backend: {
						path: "/fallback",
						file: "backend.py",
						function: "list_fallback",
					},
					ugoite_core: {
						file: "core.rs",
						function: "list_fallback",
					},
				},
			],
			linkedRequirements: ["REQ-2"],
		},
	]);

	useMockedFs({
		directories: {
			[requirementDir]: [],
		},
		files: {
			[featureRegistryPath]: "{}",
		},
	});

	expect(await getRequirementGroups()).toEqual([]);
	expect(await getFeatureGroups()).toEqual([]);
});

test("REQ-OPS-003: ui page and relationship helpers expose only visible spec data", async () => {
	const alphaPagePath = path.join(uiPagesDir, "alpha.yaml");
	const betaPagePath = path.join(uiPagesDir, "beta.yaml");
	const hiddenPagePath = path.join(uiPagesDir, "hidden.yaml");
	const brokenPagePath = path.join(uiPagesDir, "broken.yaml");
	const specificationsPath = path.join(specRoot, "specifications.yaml");
	const featureRegistryPath = path.join(featuresDir, "features.yaml");
	const linksFeaturePath = path.join(featuresDir, "links.yaml");
	const fallbackFeaturePath = path.join(featuresDir, "fallback.yaml");

	useMockedFs({
		directories: {
			[uiPagesDir]: [
				"hidden.yaml",
				"beta.yaml",
				"alpha.yaml",
				"broken.yaml",
				"notes.txt",
			],
		},
		files: {
			[alphaPagePath]: `page:
  id: alpha-page
  title: Alpha Page
  route: /alpha
  implementation: src/pages/alpha.astro
version: v1
updated: 2025-01-01
extra: true
`,
			[betaPagePath]: `page:
  id: beta-page
  title: Beta Page
  route: /beta
  implementation: src/pages/beta.astro
version: 2
updated: false
`,
			[hiddenPagePath]: `page:
  id: space-links
  title: Hidden Page
  route: /hidden
`,
			[brokenPagePath]: "{}",
			[specificationsPath]: `- id: SPEC-ALPHA
  source_file: features/links.yaml
  linked_policies:
    - POL-1
  linked_requirements:
    - REQ-1
    - REQ-2
- id: SPEC-BETA
  source_file: features/fallback.yaml
  linked_requirements:
    - REQ-1
- id: SPEC-GAMMA
  source_file: features/missing.yaml
`,
			[featureRegistryPath]: `files:
  - kind: links
    file: links.yaml
    linked_requirements:
      - REQ-1
      - REQ-2
  - kind: fallback
    file: fallback.yaml
    linked_requirements:
      - REQ-1
`,
			[linksFeaturePath]: `kind: entries
apis: []
`,
			[fallbackFeaturePath]: `apis: []
`,
		},
	});

	expect(getDocsRoot()).toMatch(/\/docs$/);
	expect(getSpecRoot()).toMatch(/\/docs\/spec$/);
	expect(await getUiPages()).toEqual([
		{
			fileName: "alpha.yaml",
			id: "alpha-page",
			title: "Alpha Page",
			route: "/alpha",
			implementation: "src/pages/alpha.astro",
		},
		{
			fileName: "beta.yaml",
			id: "beta-page",
			title: "Beta Page",
			route: "/beta",
			implementation: "src/pages/beta.astro",
		},
	]);

	expect(await getUiPageById("alpha-page")).toMatchObject({
		fileName: "alpha.yaml",
		id: "alpha-page",
		title: "Alpha Page",
		route: "/alpha",
		implementation: "src/pages/alpha.astro",
		version: "v1",
		updated: "2025-01-01",
		raw: expect.objectContaining({ extra: true }),
	});
	expect(await getUiPageById("beta-page")).toMatchObject({
		fileName: "beta.yaml",
		id: "beta-page",
		version: undefined,
		updated: undefined,
	});
	expect(await getUiPageById("space-links")).toBeNull();
	expect(await getUiPageById("missing-page")).toBeNull();

	expect(await getSpecifications()).toEqual([
		{
			id: "SPEC-ALPHA",
			sourceFile: "features/links.yaml",
			linkedPolicies: ["POL-1"],
			linkedRequirements: ["REQ-1", "REQ-2"],
		},
		{
			id: "SPEC-BETA",
			sourceFile: "features/fallback.yaml",
			linkedPolicies: [],
			linkedRequirements: ["REQ-1"],
		},
		{
			id: "SPEC-GAMMA",
			sourceFile: "features/missing.yaml",
			linkedPolicies: [],
			linkedRequirements: [],
		},
	]);

	expect(await getRequirementToFeatureEdges()).toEqual([
		{ requirement: "REQ-1", feature: "entries" },
		{ requirement: "REQ-2", feature: "entries" },
		{ requirement: "REQ-1", feature: "fallback" },
	]);
	expect(await getFeatureKindsLinkedToRequirements(["REQ-1"])).toEqual([
		"entries",
		"fallback",
	]);
	expect(await getFeatureKindsLinkedToRequirements(["REQ-404"])).toEqual([]);

	useMockedFs({
		directories: {
			[uiPagesDir]: [],
		},
		files: {
			[specificationsPath]: "",
			[featureRegistryPath]: "{}",
		},
	});

	expect(await getSpecifications()).toEqual([]);
	expect(await getRequirementToFeatureEdges()).toEqual([]);
});

test("REQ-API-004: feature manifest requirement links drive policy feature edges", async () => {
	const featureRegistryPath = path.join(featuresDir, "features.yaml");
	const searchFeaturePath = path.join(featuresDir, "search.yaml");
	const sqlFeaturePath = path.join(featuresDir, "sql.yaml");

	useMockedFs({
		files: {
			[featureRegistryPath]: `files:
  - kind: search
    file: search.yaml
    linked_requirements:
      - REQCAT-SEARCH
  - kind: sql
    file: sql.yaml
    linked_requirements:
      - REQCAT-SEARCH
      - REQCAT-API
`,
			[searchFeaturePath]: "kind: search\napis: []\n",
			[sqlFeaturePath]: "kind: sql\napis: []\n",
		},
	});

	expect(await getFeatureGroups()).toEqual([
		{
			kind: "search",
			file: "search.yaml",
			apis: [],
			linkedRequirements: ["REQCAT-SEARCH"],
		},
		{
			kind: "sql",
			file: "sql.yaml",
			apis: [],
			linkedRequirements: ["REQCAT-SEARCH", "REQCAT-API"],
		},
	]);
	expect(await getRequirementToFeatureEdges()).toEqual([
		{ requirement: "REQCAT-SEARCH", feature: "search" },
		{ requirement: "REQCAT-SEARCH", feature: "sql" },
		{ requirement: "REQCAT-API", feature: "sql" },
	]);
	expect(await getFeatureKindsLinkedToRequirements(["REQCAT-SEARCH"])).toEqual([
		"search",
		"sql",
	]);
});
