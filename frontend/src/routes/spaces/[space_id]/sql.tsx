import { useParams } from "@solidjs/router";

export default function SpaceSqlRoute() {
	const params = useParams();
	return (
		<div class="p-6">
			<h1 class="text-xl font-semibold">Saved SQL</h1>
			<p class="text-sm text-gray-600">Space: {params.space_id}</p>
			<p class="text-sm text-gray-500 mt-2">Saved SQL management is not yet in the UI.</p>
		</div>
	);
}
