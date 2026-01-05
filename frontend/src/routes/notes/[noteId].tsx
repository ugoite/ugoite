import { useParams } from "@solidjs/router";
import NotesLayout from "./NotesLayout";

export default function NoteDetailPage() {
	const params = useParams<{ noteId: string }>();
	return <NotesLayout noteId={params.noteId} />;
}
