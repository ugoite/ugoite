import { http, HttpResponse } from "msw";
import type {
	Attachment,
	Note,
	NoteCreatePayload,
	NoteRecord,
	NoteUpdatePayload,
	Schema,
	SchemaCreatePayload,
	Workspace,
	WorkspaceLink,
} from "~/lib/types";

// In-memory mock data store
let mockWorkspaces: Map<string, Workspace> = new Map();
let mockNotes: Map<string, Map<string, Note>> = new Map();
let mockNoteIndex: Map<string, Map<string, NoteRecord>> = new Map();
let mockAttachments: Map<string, Map<string, Attachment>> = new Map();
let mockLinks: Map<string, Map<string, WorkspaceLink>> = new Map();
let mockSchemas: Map<string, Map<string, Schema>> = new Map();
let revisionCounter = 0;

const generateRevisionId = () => `rev-${++revisionCounter}`;

// Reset function for tests
export const resetMockData = () => {
	mockWorkspaces = new Map();
	mockNotes = new Map();
	mockNoteIndex = new Map();
	mockAttachments = new Map();
	mockLinks = new Map();
	mockSchemas = new Map();
	revisionCounter = 0;
};

// Seed data helpers
export const seedWorkspace = (workspace: Workspace) => {
	mockWorkspaces.set(workspace.id, workspace);
	mockNotes.set(workspace.id, new Map());
	mockNoteIndex.set(workspace.id, new Map());
	mockAttachments.set(workspace.id, new Map());
	mockLinks.set(workspace.id, new Map());
	mockSchemas.set(workspace.id, new Map());
};

export const seedNote = (workspaceId: string, note: Note, record: NoteRecord) => {
	mockNotes.get(workspaceId)?.set(note.id, note);
	mockNoteIndex.get(workspaceId)?.set(note.id, record);
};

export const handlers = [
	// List workspaces
	http.get("http://localhost:3000/api/workspaces", () => {
		const workspaces = Array.from(mockWorkspaces.values());
		return HttpResponse.json(workspaces);
	}),

	// Create workspace
	http.post("http://localhost:3000/api/workspaces", async ({ request }) => {
		const body = (await request.json()) as { name: string };
		const id = body.name;

		if (mockWorkspaces.has(id)) {
			return HttpResponse.json({ detail: "Workspace already exists" }, { status: 409 });
		}

		const workspace: Workspace = {
			id,
			name: body.name,
			created_at: new Date().toISOString(),
		};
		mockWorkspaces.set(id, workspace);
		mockNotes.set(id, new Map());
		mockNoteIndex.set(id, new Map());
		mockAttachments.set(id, new Map());
		mockLinks.set(id, new Map());
		mockSchemas.set(id, new Map());

		return HttpResponse.json({ id, name: body.name }, { status: 201 });
	}),

	// List schemas
	http.get("http://localhost:3000/api/workspaces/:workspaceId/schemas", ({ params }) => {
		const workspaceId = params.workspaceId as string;
		if (!mockWorkspaces.has(workspaceId)) {
			return HttpResponse.json({ detail: "Workspace not found" }, { status: 404 });
		}
		const schemas = Array.from(mockSchemas.get(workspaceId)?.values() || []);
		return HttpResponse.json(schemas);
	}),

	// Get schema
	http.get("http://localhost:3000/api/workspaces/:workspaceId/schemas/:className", ({ params }) => {
		const workspaceId = params.workspaceId as string;
		const className = params.className as string;
		if (!mockWorkspaces.has(workspaceId)) {
			return HttpResponse.json({ detail: "Workspace not found" }, { status: 404 });
		}
		const schema = mockSchemas.get(workspaceId)?.get(className);
		if (!schema) {
			return HttpResponse.json({ detail: "Schema not found" }, { status: 404 });
		}
		return HttpResponse.json(schema);
	}),

	// Create schema
	http.post(
		"http://localhost:3000/api/workspaces/:workspaceId/schemas",
		async ({ params, request }) => {
			const workspaceId = params.workspaceId as string;
			if (!mockWorkspaces.has(workspaceId)) {
				return HttpResponse.json({ detail: "Workspace not found" }, { status: 404 });
			}
			const body = (await request.json()) as SchemaCreatePayload;
			const schema: Schema = {
				name: body.name,
				version: body.version ?? 1,
				template: body.template,
				fields: body.fields,
				defaults: body.defaults,
			};
			mockSchemas.get(workspaceId)?.set(schema.name, schema);
			return HttpResponse.json(schema, { status: 201 });
		},
	),

	// Get workspace
	http.get("http://localhost:3000/api/workspaces/:workspaceId", ({ params }) => {
		const workspace = mockWorkspaces.get(params.workspaceId as string);
		if (!workspace) {
			return HttpResponse.json({ detail: "Workspace not found" }, { status: 404 });
		}
		return HttpResponse.json(workspace);
	}),

	// Patch workspace
	http.patch("http://localhost:3000/api/workspaces/:workspaceId", async ({ params, request }) => {
		const workspaceId = params.workspaceId as string;
		const workspace = mockWorkspaces.get(workspaceId);
		if (!workspace) {
			return HttpResponse.json({ detail: "Workspace not found" }, { status: 404 });
		}
		const body = (await request.json()) as Partial<Workspace>;
		const updated: Workspace = {
			...workspace,
			...body,
			storage_config: body.storage_config ?? workspace.storage_config,
			settings: body.settings ?? workspace.settings,
		};
		mockWorkspaces.set(workspaceId, updated);
		return HttpResponse.json(updated);
	}),

	// Test connection
	http.post(
		"http://localhost:3000/api/workspaces/:workspaceId/test-connection",
		async ({ params, request }) => {
			const workspaceId = params.workspaceId as string;
			if (!mockWorkspaces.has(workspaceId)) {
				return HttpResponse.json({ detail: "Workspace not found" }, { status: 404 });
			}
			const body = (await request.json()) as { storage_config?: { uri?: string } };
			if (!body.storage_config?.uri) {
				return HttpResponse.json({ detail: "Missing uri" }, { status: 400 });
			}
			return HttpResponse.json({ status: "ok" });
		},
	),

	// List notes in workspace
	http.get("http://localhost:3000/api/workspaces/:workspaceId/notes", ({ params }) => {
		const workspaceId = params.workspaceId as string;
		if (!mockWorkspaces.has(workspaceId)) {
			return HttpResponse.json({ detail: "Workspace not found" }, { status: 404 });
		}
		const notes = Array.from(mockNoteIndex.get(workspaceId)?.values() || []);
		return HttpResponse.json(notes);
	}),

	// Create note
	http.post(
		"http://localhost:3000/api/workspaces/:workspaceId/notes",
		async ({ params, request }) => {
			const workspaceId = params.workspaceId as string;
			if (!mockWorkspaces.has(workspaceId)) {
				return HttpResponse.json({ detail: "Workspace not found" }, { status: 404 });
			}

			const body = (await request.json()) as NoteCreatePayload;
			const noteId = body.id || crypto.randomUUID();
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

			const note: Note = {
				id: noteId,
				content: body.content,
				revision_id: revisionId,
				created_at: now,
				updated_at: now,
				attachments: [],
				links: [],
			};

			const record: NoteRecord = {
				id: noteId,
				title,
				updated_at: now,
				properties,
				tags: [],
				links: [],
				attachments: [],
			};

			mockNotes.get(workspaceId)?.set(noteId, note);
			mockNoteIndex.get(workspaceId)?.set(noteId, record);

			return HttpResponse.json({ id: noteId, revision_id: revisionId }, { status: 201 });
		},
	),

	// Get note
	http.get("http://localhost:3000/api/workspaces/:workspaceId/notes/:noteId", ({ params }) => {
		const workspaceId = params.workspaceId as string;
		const noteId = params.noteId as string;

		const note = mockNotes.get(workspaceId)?.get(noteId);
		if (!note) {
			return HttpResponse.json({ detail: "Note not found" }, { status: 404 });
		}
		return HttpResponse.json(note);
	}),

	// Update note
	http.put(
		"http://localhost:3000/api/workspaces/:workspaceId/notes/:noteId",
		async ({ params, request }) => {
			const workspaceId = params.workspaceId as string;
			const noteId = params.noteId as string;

			const note = mockNotes.get(workspaceId)?.get(noteId);
			if (!note) {
				return HttpResponse.json({ detail: "Note not found" }, { status: 404 });
			}

			const body = (await request.json()) as NoteUpdatePayload;

			// Check revision (optimistic concurrency)
			if (body.parent_revision_id !== note.revision_id) {
				return HttpResponse.json(
					{
						detail: "Revision mismatch",
						current_revision_id: note.revision_id,
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

			// Update note
			note.content = body.markdown;
			note.revision_id = newRevisionId;
			note.updated_at = now;
			note.attachments = body.attachments ?? note.attachments ?? [];

			// Update index
			const record = mockNoteIndex.get(workspaceId)?.get(noteId);
			if (record) {
				record.title = title;
				record.updated_at = now;
				record.properties = properties;
				if (body.canvas_position) {
					record.canvas_position = body.canvas_position;
				}
				if (body.attachments) {
					record.attachments = body.attachments;
				}
			}

			return HttpResponse.json({
				id: noteId,
				revision_id: newRevisionId,
			});
		},
	),

	// Delete note
	http.delete("http://localhost:3000/api/workspaces/:workspaceId/notes/:noteId", ({ params }) => {
		const workspaceId = params.workspaceId as string;
		const noteId = params.noteId as string;

		if (!mockNotes.get(workspaceId)?.has(noteId)) {
			return HttpResponse.json({ detail: "Note not found" }, { status: 404 });
		}

		mockNotes.get(workspaceId)?.delete(noteId);
		mockNoteIndex.get(workspaceId)?.delete(noteId);

		return HttpResponse.json({ success: true });
	}),

	// Query notes
	http.post(
		"http://localhost:3000/api/workspaces/:workspaceId/query",
		async ({ params, request }) => {
			const workspaceId = params.workspaceId as string;
			if (!mockWorkspaces.has(workspaceId)) {
				return HttpResponse.json({ detail: "Workspace not found" }, { status: 404 });
			}

			const body = (await request.json()) as { filter: Record<string, unknown> };
			const notes = Array.from(mockNoteIndex.get(workspaceId)?.values() || []);

			// Simple filtering
			const filtered = notes.filter((note) => {
				for (const [key, value] of Object.entries(body.filter)) {
					if (key === "class" && note.class !== value) return false;
					if (note.properties[key] !== value) return false;
				}
				return true;
			});

			return HttpResponse.json(filtered);
		},
	),

	// Search notes
	http.get("http://localhost:3000/api/workspaces/:workspaceId/search", ({ params, request }) => {
		const workspaceId = params.workspaceId as string;
		if (!mockWorkspaces.has(workspaceId)) {
			return HttpResponse.json({ detail: "Workspace not found" }, { status: 404 });
		}
		const url = new URL(request.url);
		const q = url.searchParams.get("q")?.toLowerCase() ?? "";
		const notes = Array.from(mockNotes.get(workspaceId)?.values() || []);
		const index = Array.from(mockNoteIndex.get(workspaceId)?.values() || []);
		const matches = index.filter((record) => {
			const noteContent = notes.find((n) => n.id === record.id)?.content ?? "";
			const haystack =
				`${record.title}\n${JSON.stringify(record.properties)}\n${noteContent}`.toLowerCase();
			return haystack.includes(q);
		});
		return HttpResponse.json(matches);
	}),

	// Upload attachment
	http.post(
		"http://localhost:3000/api/workspaces/:workspaceId/attachments",
		async ({ params, request }) => {
			const workspaceId = params.workspaceId as string;
			if (!mockWorkspaces.has(workspaceId)) {
				return HttpResponse.json({ detail: "Workspace not found" }, { status: 404 });
			}
			let name = "upload.bin";
			try {
				const formData = await request.formData();
				const file = formData.get("file");
				if (file && typeof file === "object" && "name" in file) {
					name = (file as File).name || name;
				}
			} catch {
				// Fallback to default name when form parsing is unavailable
			}
			const id = crypto.randomUUID();
			const attachment: Attachment = { id, name, path: `attachments/${id}_${name}` };
			const store = mockAttachments.get(workspaceId);
			store?.set(id, attachment);
			return HttpResponse.json(attachment, { status: 201 });
		},
	),

	// Delete attachment
	http.delete(
		"http://localhost:3000/api/workspaces/:workspaceId/attachments/:attachmentId",
		({ params }) => {
			const workspaceId = params.workspaceId as string;
			const attachmentId = params.attachmentId as string;
			const store = mockAttachments.get(workspaceId);
			if (!store || !store.has(attachmentId)) {
				return HttpResponse.json({ detail: "Not found" }, { status: 404 });
			}

			// Check references
			const notes = mockNotes.get(workspaceId) || new Map();
			for (const note of notes.values()) {
				if ((note.attachments || []).some((a) => a.id === attachmentId)) {
					return HttpResponse.json(
						{ detail: "Attachment is referenced by a note" },
						{ status: 409 },
					);
				}
			}

			store.delete(attachmentId);
			return HttpResponse.json({ status: "deleted", id: attachmentId });
		},
	),

	// Create link
	http.post(
		"http://localhost:3000/api/workspaces/:workspaceId/links",
		async ({ params, request }) => {
			const workspaceId = params.workspaceId as string;
			const { source, target, kind } = (await request.json()) as WorkspaceLink;
			const notesStore = mockNotes.get(workspaceId);
			if (!notesStore?.has(source) || !notesStore?.has(target)) {
				return HttpResponse.json({ detail: "Note not found" }, { status: 404 });
			}
			const id = crypto.randomUUID();
			const link: WorkspaceLink = { id, source, target, kind };
			mockLinks.get(workspaceId)?.set(id, link);

			const updateLinks = (noteId: string, linkEntry: WorkspaceLink) => {
				const note = notesStore.get(noteId);
				if (note) {
					note.links = [...(note.links || []), linkEntry];
				}
				const record = mockNoteIndex.get(workspaceId)?.get(noteId);
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
		},
	),

	// List links
	http.get("http://localhost:3000/api/workspaces/:workspaceId/links", ({ params }) => {
		const workspaceId = params.workspaceId as string;
		if (!mockWorkspaces.has(workspaceId)) {
			return HttpResponse.json({ detail: "Workspace not found" }, { status: 404 });
		}
		const links = Array.from(mockLinks.get(workspaceId)?.values() || []);
		return HttpResponse.json(links);
	}),

	// Delete link
	http.delete("http://localhost:3000/api/workspaces/:workspaceId/links/:linkId", ({ params }) => {
		const workspaceId = params.workspaceId as string;
		const linkId = params.linkId as string;
		const linksStore = mockLinks.get(workspaceId);
		if (!linksStore?.has(linkId)) {
			return HttpResponse.json({ detail: "Link not found" }, { status: 404 });
		}
		linksStore.delete(linkId);

		const notesStore = mockNotes.get(workspaceId) || new Map();
		for (const note of notesStore.values()) {
			note.links = (note.links || []).filter((l) => l.id !== linkId);
		}
		const indexStore = mockNoteIndex.get(workspaceId) || new Map();
		for (const record of indexStore.values()) {
			record.links = record.links.filter((l) => l.id !== linkId);
		}

		return HttpResponse.json({ status: "deleted", id: linkId });
	}),
];
