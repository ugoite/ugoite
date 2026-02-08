import { useParams } from "@solidjs/router";

export default function SpaceSqlDetailRoute() {
	const params = useParams();
	return (
		<div class="ui-page">
			<h1 class="text-xl font-semibold">Saved SQL Detail</h1>
			<p class="text-sm ui-muted">Space: {params.space_id}</p>
			<p class="text-sm ui-muted">SQL ID: {params.sql_id}</p>
			<p class="text-sm ui-muted mt-2">Saved SQL detail UI is not yet available.</p>
		</div>
	);
}
