import { useNavigate, useParams } from "@solidjs/router";
import { NoteDetailPane } from "~/components/NoteDetailPane";
import { useNotesRouteContext } from "~/lib/notes-route-context";

export default function WorkspaceNoteDetailRoute() {
	const navigate = useNavigate();
	const params = useParams<{ workspace_id: string; note_id: string }>();
	const { workspaceId, noteStore } = useNotesRouteContext();

	return (
		<NoteDetailPane
			workspaceId={workspaceId}
			noteId={() => params.note_id}
			onAfterSave={() => noteStore.loadNotes()}
			onDeleted={() => {
				noteStore.loadNotes();
				navigate(`/workspaces/${workspaceId()}/notes`, { replace: true });
			}}
		/>
	);
}
