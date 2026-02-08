import { useParams } from "@solidjs/router";

export default function SpaceSqlRoute() {
	const params = useParams();
	return (
		<div class="ui-page">
			<h1 class="text-xl font-semibold">Saved SQL</h1>
			<p class="text-sm ui-muted">Space: {params.space_id}</p>
			<p class="text-sm ui-muted mt-2">Saved SQL management is not yet in the UI.</p>
		</div>
	);
}
