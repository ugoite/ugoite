import { http, HttpResponse } from "msw";
import type {
	Note,
	NoteCreatePayload,
	NoteRecord,
	NoteUpdatePayload,
	Workspace,
} from "~/lib/types";

// In-memory mock data store
let mockWorkspaces: Map<string, Workspace> = new Map();
let mockNotes: Map<string, Map<string, Note>> = new Map();
let mockNoteIndex: Map<string, Map<string, NoteRecord>> = new Map();
let revisionCounter = 0;

const generateRevisionId = () => `rev-${++revisionCounter}`;

// Reset function for tests
export const resetMockData = () => {
	mockWorkspaces = new Map();
	mockNotes = new Map();
	mockNoteIndex = new Map();
	revisionCounter = 0;
};

// Seed data helpers
export const seedWorkspace = (workspace: Workspace) => {
	mockWorkspaces.set(workspace.id, workspace);
	mockNotes.set(workspace.id, new Map());
	mockNoteIndex.set(workspace.id, new Map());
};

export const seedNote = (workspaceId: string, note: Note, record: NoteRecord) => {
	mockNotes.get(workspaceId)?.set(note.id, note);
	mockNoteIndex.get(workspaceId)?.set(note.id, record);
};

export const handlers = [
	// List workspaces
	http.get("/api/workspaces", () => {
		const workspaces = Array.from(mockWorkspaces.values());
		return HttpResponse.json(workspaces);
	}),

	// Create workspace
	http.post("/api/workspaces", async ({ request }) => {
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

		return HttpResponse.json({ id, name: body.name }, { status: 201 });
	}),

	// Get workspace
	http.get("/api/workspaces/:workspaceId", ({ params }) => {
		const workspace = mockWorkspaces.get(params.workspaceId as string);
		if (!workspace) {
			return HttpResponse.json({ detail: "Workspace not found" }, { status: 404 });
		}
		return HttpResponse.json(workspace);
	}),

	// List notes in workspace
	http.get("/api/workspaces/:workspaceId/notes", ({ params }) => {
		const workspaceId = params.workspaceId as string;
		if (!mockWorkspaces.has(workspaceId)) {
			return HttpResponse.json({ detail: "Workspace not found" }, { status: 404 });
		}
		const notes = Array.from(mockNoteIndex.get(workspaceId)?.values() || []);
		return HttpResponse.json(notes);
	}),

	// Create note
	http.post("/api/workspaces/:workspaceId/notes", async ({ params, request }) => {
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
		let match: RegExpExecArray | null;
		while ((match = h2Regex.exec(body.content)) !== null) {
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
		};

		const record: NoteRecord = {
			id: noteId,
			title,
			updated_at: now,
			properties,
			tags: [],
			links: [],
		};

		mockNotes.get(workspaceId)?.set(noteId, note);
		mockNoteIndex.get(workspaceId)?.set(noteId, record);

		return HttpResponse.json({ id: noteId, revision_id: revisionId }, { status: 201 });
	}),

	// Get note
	http.get("/api/workspaces/:workspaceId/notes/:noteId", ({ params }) => {
		const workspaceId = params.workspaceId as string;
		const noteId = params.noteId as string;

		const note = mockNotes.get(workspaceId)?.get(noteId);
		if (!note) {
			return HttpResponse.json({ detail: "Note not found" }, { status: 404 });
		}
		return HttpResponse.json(note);
	}),

	// Update note
	http.put("/api/workspaces/:workspaceId/notes/:noteId", async ({ params, request }) => {
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
		let match: RegExpExecArray | null;
		while ((match = h2Regex.exec(body.markdown)) !== null) {
			const key = match[1].trim();
			const value = match[2].trim();
			properties[key] = value;
		}

		// Update note
		note.content = body.markdown;
		note.revision_id = newRevisionId;
		note.updated_at = now;

		// Update index
		const record = mockNoteIndex.get(workspaceId)?.get(noteId);
		if (record) {
			record.title = title;
			record.updated_at = now;
			record.properties = properties;
			if (body.canvas_position) {
				record.canvas_position = body.canvas_position;
			}
		}

		return HttpResponse.json({
			id: noteId,
			revision_id: newRevisionId,
		});
	}),

	// Delete note
	http.delete("/api/workspaces/:workspaceId/notes/:noteId", ({ params }) => {
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
	http.post("/api/workspaces/:workspaceId/query", async ({ params, request }) => {
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
	}),
];
