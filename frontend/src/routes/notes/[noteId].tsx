import { useNavigate, useParams } from "@solidjs/router";
import { NoteDetailPane } from "~/components/NoteDetailPane";
import { useNotesRouteContext } from "~/lib/notes-route-context";

export default function NoteDetailRoute() {
	const navigate = useNavigate();
	const params = useParams<{ noteId: string }>();
	const { workspaceId, noteStore } = useNotesRouteContext();

	return (
		<NoteDetailPane
			workspaceId={workspaceId}
			noteId={() => params.noteId}
			onAfterSave={() => noteStore.loadNotes()}
			onDeleted={() => {
				noteStore.loadNotes();
				navigate("/notes", { replace: true });
			}}
		/>
	);
}
