import { useNavigate, useParams } from "@solidjs/router";
import { onMount } from "solid-js";

export default function SpaceQueryRoute() {
	const params = useParams<{ space_id: string }>();
	const navigate = useNavigate();

	onMount(() => {
		navigate(`/spaces/${params.space_id}/search`, { replace: true });
	});

	return null;
}
