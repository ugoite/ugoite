import { http, HttpResponse } from "msw";
import type {
	Asset,
	Entry,
	EntryCreatePayload,
	EntryRecord,
	EntryUpdatePayload,
	Form,
	FormCreatePayload,
	Space,
	SpaceLink,
	SampleSpaceJob,
	SampleSpaceScenario,
	SampleSpaceSummary,
} from "~/lib/types";

// In-memory mock data store
let mockSpaces: Map<string, Space> = new Map();
let mockEntries: Map<string, Map<string, Entry>> = new Map();
let mockEntryIndex: Map<string, Map<string, EntryRecord>> = new Map();
let mockAssets: Map<string, Map<string, Asset>> = new Map();
let mockLinks: Map<string, Map<string, SpaceLink>> = new Map();
let mockForms: Map<string, Map<string, Form>> = new Map();
let mockSampleJobs: Map<string, SampleSpaceJob> = new Map();
let revisionCounter = 0;

const generateRevisionId = () => `rev-${++revisionCounter}`;

// Reset function for tests
export const resetMockData = () => {
	mockSpaces = new Map();
	mockEntries = new Map();
	mockEntryIndex = new Map();
	mockAssets = new Map();
	mockLinks = new Map();
	mockForms = new Map();
	mockSampleJobs = new Map();
	revisionCounter = 0;
};

const sampleScenarios: SampleSpaceScenario[] = [
	{
		id: "renewable-ops",
		label: "Renewable operations",
		description: "Operations data for renewable energy sites.",
	},
	{
		id: "supply-chain",
		label: "Supply chain operations",
		description: "Warehouse, shipment, and supplier performance logs.",
	},
	{
		id: "municipal-infra",
		label: "Municipal infrastructure",
		description: "Asset inspections and maintenance work orders.",
	},
	{
		id: "fleet-ops",
		label: "Fleet operations",
		description: "Vehicle usage, service tickets, and fuel reports.",
	},
	{
		id: "lab-qa",
		label: "Laboratory QA",
		description: "Batch testing, calibrations, and nonconformance tracking.",
	},
	{
		id: "retail-ops",
		label: "Retail operations",
		description: "Store performance, stock alerts, and delivery logs.",
	},
];

// Seed data helpers
export const seedSpace = (space: Space) => {
	mockSpaces.set(space.id, space);
	mockEntries.set(space.id, new Map());
	mockEntryIndex.set(space.id, new Map());
	mockAssets.set(space.id, new Map());
	mockLinks.set(space.id, new Map());
	mockForms.set(space.id, new Map());
};

export const seedEntry = (spaceId: string, entry: Entry, record: EntryRecord) => {
	mockEntries.get(spaceId)?.set(entry.id, entry);
	mockEntryIndex.get(spaceId)?.set(entry.id, record);
};

export const seedForm = (spaceId: string, entryForm: Form) => {
	mockForms.get(spaceId)?.set(entryForm.name, entryForm);
};

export const handlers = [
	// List spaces
	http.get("http://localhost:3000/api/spaces", () => {
		const spaces = Array.from(mockSpaces.values());
		return HttpResponse.json(spaces);
	}),

	// Create space
	http.post("http://localhost:3000/api/spaces", async ({ request }) => {
		const body = (await request.json()) as { name: string };
		const id = body.name;

		if (mockSpaces.has(id)) {
			return HttpResponse.json({ detail: "Space already exists" }, { status: 409 });
		}

		const space: Space = {
			id,
			name: body.name,
			created_at: new Date().toISOString(),
		};
		mockSpaces.set(id, space);
		mockEntries.set(id, new Map());
		mockEntryIndex.set(id, new Map());
		mockAssets.set(id, new Map());
		mockLinks.set(id, new Map());
		mockForms.set(id, new Map());

		return HttpResponse.json({ id, name: body.name }, { status: 201 });
	}),

	// Create sample-data space
	http.post("http://localhost:3000/api/spaces/sample-data", async ({ request }) => {
		const body = (await request.json()) as {
			space_id: string;
			scenario?: string;
			entry_count?: number;
			seed?: number;
		};
		if ((body.entry_count ?? 5000) < 100) {
			return HttpResponse.json(
				{
					detail: [{ msg: "entry_count must be >= 100", type: "value_error" }],
				},
				{ status: 422 },
			);
		}
		const id = body.space_id;
		if (mockSpaces.has(id)) {
			return HttpResponse.json({ detail: "Space already exists" }, { status: 409 });
		}

		const space: Space = {
			id,
			name: id,
			created_at: new Date().toISOString(),
		};
		mockSpaces.set(id, space);
		mockEntries.set(id, new Map());
		mockEntryIndex.set(id, new Map());
		mockAssets.set(id, new Map());
		mockLinks.set(id, new Map());
		mockForms.set(id, new Map());

		const forms = ["Site", "Array", "Inspection", "MaintenanceTicket", "EnergyReport"];
		const summary: SampleSpaceSummary = {
			space_id: id,
			scenario: body.scenario ?? "renewable-ops",
			entry_count: body.entry_count ?? 5000,
			form_count: forms.length,
			forms,
		};
		return HttpResponse.json(summary, { status: 201 });
	}),

	// List sample-data scenarios
	http.get("http://localhost:3000/api/spaces/sample-data/scenarios", () => {
		return HttpResponse.json(sampleScenarios);
	}),

	// Create sample-data job
	http.post("http://localhost:3000/api/spaces/sample-data/jobs", async ({ request }) => {
		const body = (await request.json()) as {
			space_id: string;
			scenario?: string;
			entry_count?: number;
			seed?: number;
		};
		if ((body.entry_count ?? 5000) < 100) {
			return HttpResponse.json(
				{
					detail: [{ msg: "entry_count must be >= 100", type: "value_error" }],
				},
				{ status: 422 },
			);
		}
		const id = body.space_id;
		if (mockSpaces.has(id)) {
			return HttpResponse.json({ detail: "Space already exists" }, { status: 409 });
		}
		const forms = ["Site", "Array", "Inspection", "MaintenanceTicket", "EnergyReport"];
		const summary: SampleSpaceSummary = {
			space_id: id,
			scenario: body.scenario ?? "renewable-ops",
			entry_count: body.entry_count ?? 5000,
			form_count: forms.length,
			forms,
		};
		const job: SampleSpaceJob = {
			job_id: `job-${id}`,
			space_id: id,
			scenario: summary.scenario,
			entry_count: summary.entry_count,
			seed: body.seed,
			status: "completed",
			status_message: "Completed",
			processed_entries: summary.entry_count,
			total_entries: summary.entry_count,
			started_at: new Date().toISOString(),
			completed_at: new Date().toISOString(),
			error: null,
			summary,
		};
		mockSampleJobs.set(job.job_id, job);
		return HttpResponse.json(job, { status: 202 });
	}),

	// Get sample-data job
	http.get("http://localhost:3000/api/spaces/sample-data/jobs/:jobId", ({ params }) => {
		const jobId = params.jobId as string;
		const job = mockSampleJobs.get(jobId);
		if (!job) {
			return HttpResponse.json({ detail: "Job not found" }, { status: 404 });
		}
		return HttpResponse.json(job);
	}),

	// List forms
	http.get("http://localhost:3000/api/spaces/:spaceId/forms", ({ params }) => {
		const spaceId = params.spaceId as string;
		if (!mockSpaces.has(spaceId)) {
			return HttpResponse.json({ detail: "Space not found" }, { status: 404 });
		}
		const forms = Array.from(mockForms.get(spaceId)?.values() || []);
		return HttpResponse.json(forms);
	}),

	// Get form
	http.get("http://localhost:3000/api/spaces/:spaceId/forms/:formName", ({ params }) => {
		const spaceId = params.spaceId as string;
		const formName = params.formName as string;
		if (!mockSpaces.has(spaceId)) {
			return HttpResponse.json({ detail: "Space not found" }, { status: 404 });
		}
		const entryForm = mockForms.get(spaceId)?.get(formName);
		if (!entryForm) {
			return HttpResponse.json({ detail: "Form not found" }, { status: 404 });
		}
		return HttpResponse.json(entryForm);
	}),

	// Create form
	http.post("http://localhost:3000/api/spaces/:spaceId/forms", async ({ params, request }) => {
		const spaceId = params.spaceId as string;
		if (!mockSpaces.has(spaceId)) {
			return HttpResponse.json({ detail: "Space not found" }, { status: 404 });
		}
		const body = (await request.json()) as FormCreatePayload;
		const entryForm: Form = {
			name: body.name,
			version: body.version ?? 1,
			template: body.template,
			fields: body.fields,
			defaults: body.defaults,
		};
		mockForms.get(spaceId)?.set(entryForm.name, entryForm);
		return HttpResponse.json(entryForm, { status: 201 });
	}),

	// Get space
	http.get("http://localhost:3000/api/spaces/:spaceId", ({ params }) => {
		const space = mockSpaces.get(params.spaceId as string);
		if (!space) {
			return HttpResponse.json({ detail: "Space not found" }, { status: 404 });
		}
		return HttpResponse.json(space);
	}),

	// Patch space
	http.patch("http://localhost:3000/api/spaces/:spaceId", async ({ params, request }) => {
		const spaceId = params.spaceId as string;
		const space = mockSpaces.get(spaceId);
		if (!space) {
			return HttpResponse.json({ detail: "Space not found" }, { status: 404 });
		}
		const body = (await request.json()) as Partial<Space>;
		const updated: Space = {
			...space,
			...body,
			storage_config: body.storage_config ?? space.storage_config,
			settings: body.settings ?? space.settings,
		};
		mockSpaces.set(spaceId, updated);
		return HttpResponse.json(updated);
	}),

	// Test connection
	http.post(
		"http://localhost:3000/api/spaces/:spaceId/test-connection",
		async ({ params, request }) => {
			const spaceId = params.spaceId as string;
			if (!mockSpaces.has(spaceId)) {
				return HttpResponse.json({ detail: "Space not found" }, { status: 404 });
			}
			const body = (await request.json()) as { storage_config?: { uri?: string } };
			if (!body.storage_config?.uri) {
				return HttpResponse.json({ detail: "Missing uri" }, { status: 400 });
			}
			return HttpResponse.json({ status: "ok" });
		},
	),

	// List entries in space
	http.get("http://localhost:3000/api/spaces/:spaceId/entries", ({ params }) => {
		const spaceId = params.spaceId as string;
		if (!mockSpaces.has(spaceId)) {
			return HttpResponse.json({ detail: "Space not found" }, { status: 404 });
		}
		const entries = Array.from(mockEntryIndex.get(spaceId)?.values() || []);
		return HttpResponse.json(entries);
	}),

	// Create entry
	http.post("http://localhost:3000/api/spaces/:spaceId/entries", async ({ params, request }) => {
		const spaceId = params.spaceId as string;
		if (!mockSpaces.has(spaceId)) {
			return HttpResponse.json({ detail: "Space not found" }, { status: 404 });
		}

		const body = (await request.json()) as EntryCreatePayload;
		const entryId = body.id || crypto.randomUUID();
		const revisionId = generateRevisionId();
		const now = new Date().toISOString();

		// Extract title from markdown (first H1 or first line)
		const titleMatch = body.content.match(/^#\s+(.+)$/m);
		const title = titleMatch ? titleMatch[1] : body.content.split("\n")[0] || "Untitled";

		// Extract properties from H2 headers
		const properties: Record<string, string> = {};
		const h2Regex = /^##\s+(.+)\n([\s\S]*?)(?=^##\s|$(?![\r\n]))/gm;
		for (const match of body.content.matchAll(h2Regex)) {
			const key = match[1].trim();
			const value = match[2].trim();
			properties[key] = value;
		}

		const entry: Entry = {
			id: entryId,
			content: body.content,
			revision_id: revisionId,
			created_at: now,
			updated_at: now,
			assets: [],
			links: [],
		};

		const record: EntryRecord = {
			id: entryId,
			title,
			updated_at: now,
			properties,
			tags: [],
			links: [],
			assets: [],
		};

		mockEntries.get(spaceId)?.set(entryId, entry);
		mockEntryIndex.get(spaceId)?.set(entryId, record);

		return HttpResponse.json({ id: entryId, revision_id: revisionId }, { status: 201 });
	}),

	// Get entry
	http.get("http://localhost:3000/api/spaces/:spaceId/entries/:entryId", ({ params }) => {
		const spaceId = params.spaceId as string;
		const entryId = params.entryId as string;

		const entry = mockEntries.get(spaceId)?.get(entryId);
		if (!entry) {
			return HttpResponse.json({ detail: "Entry not found" }, { status: 404 });
		}
		return HttpResponse.json(entry);
	}),

	// Update entry
	http.put(
		"http://localhost:3000/api/spaces/:spaceId/entries/:entryId",
		async ({ params, request }) => {
			const spaceId = params.spaceId as string;
			const entryId = params.entryId as string;

			const entry = mockEntries.get(spaceId)?.get(entryId);
			if (!entry) {
				return HttpResponse.json({ detail: "Entry not found" }, { status: 404 });
			}

			const body = (await request.json()) as EntryUpdatePayload;

			// Check revision (optimistic concurrency)
			if (body.parent_revision_id !== entry.revision_id) {
				return HttpResponse.json(
					{
						detail: "Revision mismatch",
						current_revision_id: entry.revision_id,
					},
					{ status: 409 },
				);
			}

			const newRevisionId = generateRevisionId();
			const now = new Date().toISOString();

			// Extract title from markdown
			const titleMatch = body.markdown.match(/^#\s+(.+)$/m);
			const title = titleMatch ? titleMatch[1] : body.markdown.split("\n")[0] || "Untitled";

			// Extract properties from H2 headers
			const properties: Record<string, string> = {};
			const h2Regex = /^##\s+(.+)\n([\s\S]*?)(?=^##\s|$(?![\r\n]))/gm;
			for (const match of body.markdown.matchAll(h2Regex)) {
				const key = match[1].trim();
				const value = match[2].trim();
				properties[key] = value;
			}

			// Update entry
			entry.content = body.markdown;
			entry.revision_id = newRevisionId;
			entry.updated_at = now;
			entry.assets = body.assets ?? entry.assets ?? [];

			// Update index
			const record = mockEntryIndex.get(spaceId)?.get(entryId);
			if (record) {
				record.title = title;
				record.updated_at = now;
				record.properties = properties;
				if (body.canvas_position) {
					record.canvas_position = body.canvas_position;
				}
				if (body.assets) {
					record.assets = body.assets;
				}
			}

			return HttpResponse.json({
				id: entryId,
				revision_id: newRevisionId,
			});
		},
	),

	// Delete entry
	http.delete("http://localhost:3000/api/spaces/:spaceId/entries/:entryId", ({ params }) => {
		const spaceId = params.spaceId as string;
		const entryId = params.entryId as string;

		if (!mockEntries.get(spaceId)?.has(entryId)) {
			return HttpResponse.json({ detail: "Entry not found" }, { status: 404 });
		}

		mockEntries.get(spaceId)?.delete(entryId);
		mockEntryIndex.get(spaceId)?.delete(entryId);

		return HttpResponse.json({ success: true });
	}),

	// Query entries
	http.post("http://localhost:3000/api/spaces/:spaceId/query", async ({ params, request }) => {
		const spaceId = params.spaceId as string;
		if (!mockSpaces.has(spaceId)) {
			return HttpResponse.json({ detail: "Space not found" }, { status: 404 });
		}

		const body = (await request.json()) as { filter: Record<string, unknown> };
		const entries = Array.from(mockEntryIndex.get(spaceId)?.values() || []);

		// Simple filtering
		const filtered = entries.filter((entry) => {
			for (const [key, value] of Object.entries(body.filter)) {
				if (key === "form" && entry.form !== value) return false;
				if (entry.properties[key] !== value) return false;
			}
			return true;
		});

		return HttpResponse.json(filtered);
	}),

	// Search entries
	http.get("http://localhost:3000/api/spaces/:spaceId/search", ({ params, request }) => {
		const spaceId = params.spaceId as string;
		if (!mockSpaces.has(spaceId)) {
			return HttpResponse.json({ detail: "Space not found" }, { status: 404 });
		}
		const url = new URL(request.url);
		const q = url.searchParams.get("q")?.toLowerCase() ?? "";
		const entries = Array.from(mockEntries.get(spaceId)?.values() || []);
		const index = Array.from(mockEntryIndex.get(spaceId)?.values() || []);
		const matches = index.filter((record) => {
			const entryContent = entries.find((n) => n.id === record.id)?.content ?? "";
			const haystack =
				`${record.title}\n${JSON.stringify(record.properties)}\n${entryContent}`.toLowerCase();
			return haystack.includes(q);
		});
		return HttpResponse.json(matches);
	}),

	// Upload asset
	http.post("http://localhost:3000/api/spaces/:spaceId/assets", async ({ params }) => {
		const spaceId = params.spaceId as string;
		if (!mockSpaces.has(spaceId)) {
			return HttpResponse.json({ detail: "Space not found" }, { status: 404 });
		}
		// In mock tests, we don't actually need to parse FormData which can hang in CI
		// The file name is not critical for these tests
		const name = "test-file.bin";
		const id = crypto.randomUUID();
		const asset: Asset = { id, name, path: `assets/${id}_${name}` };
		const store = mockAssets.get(spaceId);
		store?.set(id, asset);
		return HttpResponse.json(asset, { status: 201 });
	}),

	// Delete asset
	http.delete("http://localhost:3000/api/spaces/:spaceId/assets/:assetId", ({ params }) => {
		const spaceId = params.spaceId as string;
		const assetId = params.assetId as string;
		const store = mockAssets.get(spaceId);
		if (!store || !store.has(assetId)) {
			return HttpResponse.json({ detail: "Not found" }, { status: 404 });
		}

		// Check references
		const entries = mockEntries.get(spaceId) || new Map();
		for (const entry of entries.values()) {
			if ((entry.assets || []).some((a) => a.id === assetId)) {
				return HttpResponse.json({ detail: "Asset is referenced by a entry" }, { status: 409 });
			}
		}

		store.delete(assetId);
		return HttpResponse.json({ status: "deleted", id: assetId });
	}),

	// Create link
	http.post("http://localhost:3000/api/spaces/:spaceId/links", async ({ params, request }) => {
		const spaceId = params.spaceId as string;
		const { source, target, kind } = (await request.json()) as SpaceLink;
		const entriesStore = mockEntries.get(spaceId);
		if (!entriesStore?.has(source) || !entriesStore?.has(target)) {
			return HttpResponse.json({ detail: "Entry not found" }, { status: 404 });
		}
		const id = crypto.randomUUID();
		const link: SpaceLink = { id, source, target, kind };
		mockLinks.get(spaceId)?.set(id, link);

		const updateLinks = (entryId: string, linkEntry: SpaceLink) => {
			const entry = entriesStore.get(entryId);
			if (entry) {
				entry.links = [...(entry.links || []), linkEntry];
			}
			const record = mockEntryIndex.get(spaceId)?.get(entryId);
			if (record) {
				record.links = [
					...record.links,
					{
						id: linkEntry.id,
						target: linkEntry.target,
						kind: linkEntry.kind,
						source: linkEntry.source,
					},
				];
			}
		};

		updateLinks(source, link);
		updateLinks(target, { ...link, source: target, target: source });

		return HttpResponse.json(link, { status: 201 });
	}),

	// List links
	http.get("http://localhost:3000/api/spaces/:spaceId/links", ({ params }) => {
		const spaceId = params.spaceId as string;
		if (!mockSpaces.has(spaceId)) {
			return HttpResponse.json({ detail: "Space not found" }, { status: 404 });
		}
		const links = Array.from(mockLinks.get(spaceId)?.values() || []);
		return HttpResponse.json(links);
	}),

	// Delete link
	http.delete("http://localhost:3000/api/spaces/:spaceId/links/:linkId", ({ params }) => {
		const spaceId = params.spaceId as string;
		const linkId = params.linkId as string;
		const linksStore = mockLinks.get(spaceId);
		if (!linksStore?.has(linkId)) {
			return HttpResponse.json({ detail: "Link not found" }, { status: 404 });
		}
		linksStore.delete(linkId);

		const entriesStore = mockEntries.get(spaceId) || new Map();
		for (const entry of entriesStore.values()) {
			entry.links = (entry.links || []).filter((l) => l.id !== linkId);
		}
		const indexStore = mockEntryIndex.get(spaceId) || new Map();
		for (const record of indexStore.values()) {
			record.links = record.links.filter((l) => l.id !== linkId);
		}

		return HttpResponse.json({ status: "deleted", id: linkId });
	}),
];
