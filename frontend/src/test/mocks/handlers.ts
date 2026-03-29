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
	UserPreferences,
	UserPreferencesPatchPayload,
} from "~/lib/types";

// In-memory mock data store
let mockSpaces: Map<string, Space> = new Map();
let mockEntries: Map<string, Map<string, Entry>> = new Map();
let mockEntryIndex: Map<string, Map<string, EntryRecord>> = new Map();
let mockAssets: Map<string, Map<string, Asset>> = new Map();
let mockForms: Map<string, Map<string, Form>> = new Map();
let mockSqlEntries: Map<string, Map<string, Record<string, unknown>>> = new Map();
let mockSqlSessions: Map<string, Map<string, Record<string, unknown>>> = new Map();
let mockDevAuthConfig = {
	mode: "passkey-totp" as "passkey-totp" | "mock-oauth",
	username_hint: "dev-local-user",
	supports_passkey_totp: true,
	supports_mock_oauth: false,
};
let mockPreferences: UserPreferences = {
	selected_space_id: null,
	locale: null,
	ui_theme: null,
	color_mode: null,
	primary_color: null,
};
let preferencePatches: UserPreferencesPatchPayload[] = [];
let revisionCounter = 0;

const generateRevisionId = () => `rev-${++revisionCounter}`;

// Reset function for tests
export const resetMockData = () => {
	mockSpaces = new Map();
	mockEntries = new Map();
	mockEntryIndex = new Map();
	mockAssets = new Map();
	mockForms = new Map();
	mockSqlEntries = new Map();
	mockSqlSessions = new Map();
	mockDevAuthConfig = {
		mode: "passkey-totp",
		username_hint: "dev-local-user",
		supports_passkey_totp: true,
		supports_mock_oauth: false,
	};
	mockPreferences = {
		selected_space_id: null,
		locale: null,
		ui_theme: null,
		color_mode: null,
		primary_color: null,
	};
	preferencePatches = [];
	revisionCounter = 0;
};

// Seed data helpers
export const seedSpace = (space: Space) => {
	mockSpaces.set(space.id, space);
	mockEntries.set(space.id, new Map());
	mockEntryIndex.set(space.id, new Map());
	mockAssets.set(space.id, new Map());
	mockForms.set(space.id, new Map());
	mockSqlEntries.set(space.id, new Map());
	mockSqlSessions.set(space.id, new Map());
};

export const seedEntry = (spaceId: string, entry: Entry, record: EntryRecord) => {
	mockEntries.get(spaceId)?.set(entry.id, entry);
	mockEntryIndex.get(spaceId)?.set(entry.id, record);
};

export const seedForm = (spaceId: string, entryForm: Form) => {
	mockForms.get(spaceId)?.set(entryForm.name, entryForm);
};

export const seedSqlEntry = (spaceId: string, entry: Record<string, unknown> & { id: string }) => {
	if (!mockSqlEntries.has(spaceId)) mockSqlEntries.set(spaceId, new Map());
	mockSqlEntries.get(spaceId)?.set(entry.id, entry);
};

export const seedSqlSession = (
	spaceId: string,
	session: Record<string, unknown> & { id: string },
) => {
	if (!mockSqlSessions.has(spaceId)) mockSqlSessions.set(spaceId, new Map());
	mockSqlSessions.get(spaceId)?.set(session.id, session);
};

export const seedPreferences = (preferences: Partial<UserPreferences>) => {
	mockPreferences = {
		...mockPreferences,
		...preferences,
	};
};

export const seedDevAuthConfig = (
	config: Partial<{
		mode: "passkey-totp" | "mock-oauth";
		username_hint: string;
		supports_passkey_totp: boolean;
		supports_mock_oauth: boolean;
	}>,
) => {
	mockDevAuthConfig = {
		...mockDevAuthConfig,
		...config,
	};
};

export const getPreferencePatches = () => preferencePatches.slice();

export const handlers = [
	http.get("http://localhost:3000/api/auth/config", () => {
		return HttpResponse.json(mockDevAuthConfig);
	}),

	http.post("http://localhost:3000/api/auth/login", async ({ request }) => {
		const body = (await request.json()) as { username?: string; totp_code?: string };
		if (
			mockDevAuthConfig.mode !== "passkey-totp" ||
			body.username !== mockDevAuthConfig.username_hint ||
			body.totp_code !== "123456"
		) {
			return HttpResponse.json({ detail: "Invalid username or 2FA code." }, { status: 401 });
		}
		return HttpResponse.json({
			bearer_token: "frontend-test-token",
			user_id: mockDevAuthConfig.username_hint,
			expires_at: 1_900_000_000,
		});
	}),

	http.post("http://localhost:3000/api/auth/mock-oauth", () => {
		if (mockDevAuthConfig.mode !== "mock-oauth") {
			return HttpResponse.json(
				{ detail: "mock-oauth login is not enabled for this session." },
				{ status: 409 },
			);
		}
		return HttpResponse.json({
			bearer_token: "frontend-test-token",
			user_id: mockDevAuthConfig.username_hint,
			expires_at: 1_900_000_000,
		});
	}),

	http.get("http://localhost:3000/api/preferences/me", () => {
		return HttpResponse.json(mockPreferences);
	}),

	http.patch("http://localhost:3000/api/preferences/me", async ({ request }) => {
		const body = (await request.json()) as UserPreferencesPatchPayload;
		preferencePatches.push(body);
		mockPreferences = {
			...mockPreferences,
			...body,
		};
		return HttpResponse.json(mockPreferences);
	}),

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
		mockForms.set(id, new Map());

		return HttpResponse.json({ id, name: body.name }, { status: 201 });
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

	// List form types (must be before forms/:formName to avoid capture)
	http.get("http://localhost:3000/api/spaces/:spaceId/forms/types", ({ params }) => {
		const spaceId = params.spaceId as string;
		if (!mockSpaces.has(spaceId)) {
			return HttpResponse.json({ detail: "Space not found" }, { status: 404 });
		}
		return HttpResponse.json(["date", "text", "number", "boolean", "row_reference"]);
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

	// List assets
	http.get("http://localhost:3000/api/spaces/:spaceId/assets", ({ params }) => {
		const spaceId = params.spaceId as string;
		if (!mockSpaces.has(spaceId)) {
			return HttpResponse.json({ detail: "Space not found" }, { status: 404 });
		}
		const assets = Array.from(mockAssets.get(spaceId)?.values() || []);
		return HttpResponse.json(assets);
	}),

	// Entry history
	http.get("http://localhost:3000/api/spaces/:spaceId/entries/:entryId/history", ({ params }) => {
		const spaceId = params.spaceId as string;
		const entryId = params.entryId as string;
		const entry = mockEntries.get(spaceId)?.get(entryId);
		if (!entry) {
			return HttpResponse.json({ detail: "Entry not found" }, { status: 404 });
		}
		return HttpResponse.json({
			revisions: [{ id: entry.revision_id, created_at: entry.updated_at, author: null }],
		});
	}),

	// Get specific revision
	http.get(
		"http://localhost:3000/api/spaces/:spaceId/entries/:entryId/history/:revisionId",
		({ params }) => {
			const spaceId = params.spaceId as string;
			const entryId = params.entryId as string;
			const revisionId = params.revisionId as string;
			const entry = mockEntries.get(spaceId)?.get(entryId);
			if (!entry || entry.revision_id !== revisionId) {
				return HttpResponse.json({ detail: "Revision not found" }, { status: 404 });
			}
			return HttpResponse.json(entry);
		},
	),

	// Restore entry
	http.post(
		"http://localhost:3000/api/spaces/:spaceId/entries/:entryId/restore",
		async ({ params, request }) => {
			const spaceId = params.spaceId as string;
			const entryId = params.entryId as string;
			const entry = mockEntries.get(spaceId)?.get(entryId);
			if (!entry) {
				return HttpResponse.json({ detail: "Entry not found" }, { status: 404 });
			}
			const _body = (await request.json()) as { revision_id: string };
			const newRevisionId = generateRevisionId();
			entry.revision_id = newRevisionId;
			return HttpResponse.json({ ...entry, revision_id: newRevisionId });
		},
	),

	// SQL CRUD
	http.get("http://localhost:3000/api/spaces/:spaceId/sql", ({ params }) => {
		const spaceId = params.spaceId as string;
		const entries = Array.from(mockSqlEntries.get(spaceId)?.values() || []);
		return HttpResponse.json(entries);
	}),
	http.get("http://localhost:3000/api/spaces/:spaceId/sql/:sqlId", ({ params }) => {
		const spaceId = params.spaceId as string;
		const sqlId = params.sqlId as string;
		const entry = mockSqlEntries.get(spaceId)?.get(sqlId);
		if (!entry) return HttpResponse.json({ detail: "Not found" }, { status: 404 });
		return HttpResponse.json(entry);
	}),
	http.post("http://localhost:3000/api/spaces/:spaceId/sql", async ({ params, request }) => {
		const spaceId = params.spaceId as string;
		const body = (await request.json()) as { name: string; sql: string; variables?: string[] };
		const id = crypto.randomUUID();
		const revisionId = generateRevisionId();
		const entry = {
			id,
			name: body.name,
			sql: body.sql,
			variables: body.variables || [],
			space_id: spaceId,
		};
		if (!mockSqlEntries.has(spaceId)) mockSqlEntries.set(spaceId, new Map());
		mockSqlEntries.get(spaceId)?.set(id, entry);
		return HttpResponse.json({ id, revision_id: revisionId }, { status: 201 });
	}),
	http.put("http://localhost:3000/api/spaces/:spaceId/sql/:sqlId", async ({ params, request }) => {
		const spaceId = params.spaceId as string;
		const sqlId = params.sqlId as string;
		const entry = mockSqlEntries.get(spaceId)?.get(sqlId);
		if (!entry) return HttpResponse.json({ detail: "Not found" }, { status: 404 });
		const body = (await request.json()) as { name?: string; sql?: string };
		Object.assign(entry, body);
		const revisionId = generateRevisionId();
		return HttpResponse.json({ id: sqlId, revision_id: revisionId });
	}),
	http.delete("http://localhost:3000/api/spaces/:spaceId/sql/:sqlId", ({ params }) => {
		const spaceId = params.spaceId as string;
		const sqlId = params.sqlId as string;
		const store = mockSqlEntries.get(spaceId);
		if (!store || !store.has(sqlId))
			return HttpResponse.json({ detail: "Not found" }, { status: 404 });
		store.delete(sqlId);
		return HttpResponse.json({ status: "deleted" });
	}),

	// SQL Sessions
	http.post(
		"http://localhost:3000/api/spaces/:spaceId/sql-sessions",
		async ({ params, request }) => {
			const spaceId = params.spaceId as string;
			const body = (await request.json()) as { sql: string };
			const id = crypto.randomUUID();
			const session = {
				id,
				sql: body.sql,
				status: "ready",
				error: null,
				created_at: new Date().toISOString(),
			};
			if (!mockSqlSessions.has(spaceId)) mockSqlSessions.set(spaceId, new Map());
			mockSqlSessions.get(spaceId)?.set(id, session);
			return HttpResponse.json(session, { status: 201 });
		},
	),
	http.get("http://localhost:3000/api/spaces/:spaceId/sql-sessions/:sessionId", ({ params }) => {
		const spaceId = params.spaceId as string;
		const sessionId = params.sessionId as string;
		const session = mockSqlSessions.get(spaceId)?.get(sessionId);
		if (!session) return HttpResponse.json({ detail: "Not found" }, { status: 404 });
		return HttpResponse.json(session);
	}),
	http.get(
		"http://localhost:3000/api/spaces/:spaceId/sql-sessions/:sessionId/count",
		({ params }) => {
			const spaceId = params.spaceId as string;
			const sessionId = params.sessionId as string;
			if (!mockSqlSessions.get(spaceId)?.has(sessionId))
				return HttpResponse.json({ detail: "Not found" }, { status: 404 });
			return HttpResponse.json({ count: 3 });
		},
	),
	http.get(
		"http://localhost:3000/api/spaces/:spaceId/sql-sessions/:sessionId/rows",
		({ params, request }) => {
			const spaceId = params.spaceId as string;
			const sessionId = params.sessionId as string;
			if (!mockSqlSessions.get(spaceId)?.has(sessionId))
				return HttpResponse.json({ detail: "Not found" }, { status: 404 });
			const url = new URL(request.url);
			const offset = Number(url.searchParams.get("offset") || 0);
			const limit = Number(url.searchParams.get("limit") || 25);
			return HttpResponse.json({ rows: [], offset, limit, total_count: 3 });
		},
	),

	// Space Members
	http.get("http://localhost:3000/api/spaces/:spaceId/members", ({ params }) => {
		const spaceId = params.spaceId as string;
		if (!mockSpaces.has(spaceId))
			return HttpResponse.json({ detail: "Not found" }, { status: 404 });
		return HttpResponse.json([]);
	}),
	http.post(
		"http://localhost:3000/api/spaces/:spaceId/members/invitations",
		async ({ params, request }) => {
			const spaceId = params.spaceId as string;
			if (!mockSpaces.has(spaceId))
				return HttpResponse.json({ detail: "Not found" }, { status: 404 });
			const body = (await request.json()) as { user_id: string; role: string };
			return HttpResponse.json(
				{
					invitation: {
						token: "test-token",
						user_id: body.user_id,
						role: body.role,
						state: "pending",
					},
				},
				{ status: 201 },
			);
		},
	),
	http.post(
		"http://localhost:3000/api/spaces/:spaceId/members/accept",
		async ({ params, request }) => {
			const spaceId = params.spaceId as string;
			if (!mockSpaces.has(spaceId))
				return HttpResponse.json({ detail: "Not found" }, { status: 404 });
			const body = (await request.json()) as { token: string; user_id: string };
			return HttpResponse.json({
				member: { user_id: body.user_id, role: "editor", state: "active" },
			});
		},
	),
	http.post(
		"http://localhost:3000/api/spaces/:spaceId/members/:userId/role",
		async ({ params, request }) => {
			const spaceId = params.spaceId as string;
			if (!mockSpaces.has(spaceId))
				return HttpResponse.json({ detail: "Not found" }, { status: 404 });
			const body = (await request.json()) as { role: string };
			return HttpResponse.json({
				member: { user_id: params.userId, role: body.role, state: "active" },
			});
		},
	),
	http.delete("http://localhost:3000/api/spaces/:spaceId/members/:userId", ({ params }) => {
		const spaceId = params.spaceId as string;
		if (!mockSpaces.has(spaceId))
			return HttpResponse.json({ detail: "Not found" }, { status: 404 });
		return HttpResponse.json({ member: { user_id: params.userId, state: "revoked" } });
	}),

	// Root API endpoint (used by HelloWorld)
	http.get("http://localhost:3000/api/", () => {
		return HttpResponse.json({ message: "Hello from mock backend!" });
	}),
];
