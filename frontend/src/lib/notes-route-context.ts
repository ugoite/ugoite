import { createContext, useContext } from "solid-js";
import type { Accessor } from "solid-js";
import type { NoteStore } from "~/lib/store";
import type { Schema } from "~/lib/types";
import type { WorkspaceStore } from "~/lib/workspace-store";

export interface NotesRouteContextValue {
	workspaceStore: WorkspaceStore;
	workspaceId: Accessor<string>;
	noteStore: NoteStore;
	schemas: Accessor<Schema[]>;
	refetchSchemas: () => void;
}

export const NotesRouteContext = createContext<NotesRouteContextValue>();

export function useNotesRouteContext(): NotesRouteContextValue {
	const ctx = useContext(NotesRouteContext);
	if (!ctx) {
		throw new Error("NotesRouteContext is missing. Ensure it is provided by the /notes route.");
	}
	return ctx;
}
