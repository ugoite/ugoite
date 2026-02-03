import { useNavigate, useParams } from "@solidjs/router";
import { onMount } from "solid-js";

export default function WorkspaceDetailRoute() {
	const params = useParams<{ workspace_id: string }>();
	const navigate = useNavigate();
	const workspaceId = () => params.workspace_id;

	onMount(() => {
		if (workspaceId()) {
			navigate(`/workspaces/${workspaceId()}/dashboard`, { replace: true });
		}
	});

	return null;
}
