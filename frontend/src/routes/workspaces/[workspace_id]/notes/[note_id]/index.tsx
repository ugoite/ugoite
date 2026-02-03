import { useNavigate, useParams } from "@solidjs/router";
import { NoteDetailPane } from "~/components/NoteDetailPane";
import { WorkspaceShell } from "~/components/WorkspaceShell";

export default function WorkspaceNoteDetailRoute() {
	const navigate = useNavigate();
	const params = useParams<{ workspace_id: string; note_id: string }>();
	const workspaceId = () => params.workspace_id || "";
	// SolidJS router already decodes URL parameters
	const noteId = () => params.note_id ?? "";

	return (
		<WorkspaceShell workspaceId={workspaceId()}>
			<div class="mx-auto max-w-6xl h-[calc(100vh-8rem)]">
				<NoteDetailPane
					workspaceId={workspaceId}
					noteId={noteId}
					onDeleted={() => {
						navigate(`/workspaces/${workspaceId()}/notes`, { replace: true });
					}}
				/>
			</div>
		</WorkspaceShell>
	);
}
