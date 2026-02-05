import { useNavigate, useParams } from "@solidjs/router";
import { onMount } from "solid-js";

export default function SpaceDetailRoute() {
	const params = useParams<{ space_id: string }>();
	const navigate = useNavigate();
	const spaceId = () => params.space_id;

	onMount(() => {
		if (spaceId()) {
			navigate(`/spaces/${spaceId()}/dashboard`, { replace: true });
		}
	});

	return null;
}
