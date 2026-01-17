import { useNavigate, useParams } from "@solidjs/router";
import { useContext } from "solid-js";
import { NoteDetailPane } from "~/components/NoteDetailPane";
import { NotesRouteContext } from "~/lib/notes-route-context";
import { createNoteStore } from "~/lib/note-store";

export default function WorkspaceNoteDetailRoute() {
	const navigate = useNavigate();
	const params = useParams<{ workspace_id: string; note_id: string }>();
	const fallbackWorkspaceId = () => params.workspace_id || "";
	const ctx = useContext(NotesRouteContext);
	const workspaceId = ctx?.workspaceId ?? fallbackWorkspaceId;
	const noteStore = ctx?.noteStore ?? createNoteStore(fallbackWorkspaceId);
	const noteId = () => {
		const raw = params.note_id ?? "";
		if (!raw) return "";
		try {
			return decodeURIComponent(raw);
		} catch {
			return raw;
		}
	};

	return (
		<NoteDetailPane
			workspaceId={workspaceId}
			noteId={noteId}
			onAfterSave={() => noteStore.loadNotes()}
			onDeleted={() => {
				noteStore.loadNotes();
				navigate(`/workspaces/${workspaceId()}/notes`, { replace: true });
			}}
		/>
	);
}
