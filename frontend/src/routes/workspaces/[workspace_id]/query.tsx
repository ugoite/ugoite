import { useNavigate, useParams } from "@solidjs/router";
import { onMount } from "solid-js";

export default function WorkspaceQueryRoute() {
	const params = useParams<{ workspace_id: string }>();
	const navigate = useNavigate();

	onMount(() => {
		navigate(`/workspaces/${params.workspace_id}/search`, { replace: true });
	});

	return null;
}
