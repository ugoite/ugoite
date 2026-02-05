import { useNavigate, useParams } from "@solidjs/router";
import { EntryDetailPane } from "~/components/EntryDetailPane";
import { SpaceShell } from "~/components/SpaceShell";

export default function SpaceEntryDetailRoute() {
	const navigate = useNavigate();
	const params = useParams<{ space_id: string; entry_id: string }>();
	const spaceId = () => params.space_id || "";
	// SolidJS router already decodes URL parameters
	const entryId = () => params.entry_id ?? "";

	return (
		<SpaceShell spaceId={spaceId()}>
			<div class="mx-auto max-w-6xl h-[calc(100vh-8rem)]">
				<EntryDetailPane
					spaceId={spaceId}
					entryId={entryId}
					onDeleted={() => {
						navigate(`/spaces/${spaceId()}/entries`, { replace: true });
					}}
				/>
			</div>
		</SpaceShell>
	);
}
