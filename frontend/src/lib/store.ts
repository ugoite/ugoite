import { createSignal, createResource } from "solid-js";
import type { Note, NoteRecord, NoteUpdatePayload } from "./types";
import { noteApi, RevisionConflictError } from "./client";

export interface NoteStoreState {
	notes: NoteRecord[];
	selectedNoteId: string | null;
	selectedNote: Note | null;
	loading: boolean;
	error: string | null;
	// Optimistic state
	pendingUpdates: Map<string, NoteRecord>;
}

/**
 * Creates a reactive note store for a workspace.
 * Implements optimistic updates with server reconciliation.
 */
export function createNoteStore(workspaceId: () => string) {
	// Core state
	const [notes, setNotes] = createSignal<NoteRecord[]>([]);
	const [selectedNoteId, setSelectedNoteId] = createSignal<string | null>(null);
	const [loading, setLoading] = createSignal(false);
	const [error, setError] = createSignal<string | null>(null);

	// Track pending optimistic updates
	const pendingUpdates = new Map<string, { original: NoteRecord; optimistic: NoteRecord }>();

	// Fetch selected note content
	const [selectedNote, { refetch: refetchSelectedNote }] = createResource(
		() => {
			const noteId = selectedNoteId();
			const wsId = workspaceId();
			return noteId && wsId ? { wsId, noteId } : null;
		},
		async (params) => {
			if (!params) return null;
			try {
				return await noteApi.get(params.wsId, params.noteId);
			} catch {
				return null;
			}
		},
	);

	/** Load all notes from server */
	async function loadNotes() {
		setLoading(true);
		setError(null);
		try {
			const fetchedNotes = await noteApi.list(workspaceId());
			setNotes(fetchedNotes);
		} catch (e) {
			setError(e instanceof Error ? e.message : "Failed to load notes");
		} finally {
			setLoading(false);
		}
	}

	/** Create a new note */
	async function createNote(content: string, id?: string) {
		setError(null);
		try {
			const result = await noteApi.create(workspaceId(), { content, id });
			// Reload to get the indexed version
			await loadNotes();
			return result;
		} catch (e) {
			setError(e instanceof Error ? e.message : "Failed to create note");
			throw e;
		}
	}

	/** Update a note with optimistic updates */
	async function updateNote(noteId: string, payload: NoteUpdatePayload) {
		setError(null);
		const currentNotes = notes();
		const noteIndex = currentNotes.findIndex((n) => n.id === noteId);

		if (noteIndex === -1) {
			throw new Error("Note not found in local state");
		}

		const originalNote = currentNotes[noteIndex];

		// Extract title from markdown for optimistic update
		const titleMatch = payload.markdown.match(/^#\s+(.+)$/m);
		const title = titleMatch ? titleMatch[1] : originalNote.title;

		// Create optimistic record
		const optimisticNote: NoteRecord = {
			...originalNote,
			title,
			updated_at: new Date().toISOString(),
			canvas_position: payload.canvas_position || originalNote.canvas_position,
		};

		// Store for potential rollback
		pendingUpdates.set(noteId, { original: originalNote, optimistic: optimisticNote });

		// Apply optimistic update
		setNotes((prev) =>
			prev.map((n) => (n.id === noteId ? optimisticNote : n)),
		);

		try {
			const result = await noteApi.update(workspaceId(), noteId, payload);

			// Clear pending update on success
			pendingUpdates.delete(noteId);

			// Refetch the note to ensure we have latest server state
			if (selectedNoteId() === noteId) {
				refetchSelectedNote();
			}

			return result;
		} catch (e) {
			// Rollback on failure
			const pending = pendingUpdates.get(noteId);
			if (pending) {
				setNotes((prev) =>
					prev.map((n) => (n.id === noteId ? pending.original : n)),
				);
				pendingUpdates.delete(noteId);
			}

			if (e instanceof RevisionConflictError) {
				// Reload to get server state
				await loadNotes();
				if (selectedNoteId() === noteId) {
					refetchSelectedNote();
				}
			}

			setError(e instanceof Error ? e.message : "Failed to update note");
			throw e;
		}
	}

	/** Delete a note */
	async function deleteNote(noteId: string) {
		setError(null);

		// Optimistic removal
		const currentNotes = notes();
		const noteToDelete = currentNotes.find((n) => n.id === noteId);
		setNotes((prev) => prev.filter((n) => n.id !== noteId));

		// Clear selection if deleted
		if (selectedNoteId() === noteId) {
			setSelectedNoteId(null);
		}

		try {
			await noteApi.delete(workspaceId(), noteId);
		} catch (e) {
			// Rollback on failure
			if (noteToDelete) {
				setNotes((prev) => [...prev, noteToDelete]);
			}
			setError(e instanceof Error ? e.message : "Failed to delete note");
			throw e;
		}
	}

	/** Select a note for editing */
	function selectNote(noteId: string | null) {
		setSelectedNoteId(noteId);
	}

	return {
		// Reactive getters
		notes,
		selectedNoteId,
		selectedNote,
		loading,
		error,

		// Actions
		loadNotes,
		createNote,
		updateNote,
		deleteNote,
		selectNote,
		refetchSelectedNote,
	};
}

export type NoteStore = ReturnType<typeof createNoteStore>;
