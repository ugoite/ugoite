import { A, useParams } from "@solidjs/router";

export default function SpaceLinkDetailRoute() {
	const params = useParams<{ space_id: string; link_id: string }>();
	const spaceId = () => params.space_id;
	const notice = "Links API has been removed. Use row_reference fields instead.";
	return (
		<main class="ui-shell">
			<div class="ui-page max-w-3xl mx-auto">
				<div class="flex items-center justify-between mb-6">
					<h1 class="ui-page-title">Link</h1>
					<A href={`/spaces/${spaceId()}/links`} class="text-sm ui-link">
						Back to Links
					</A>
				</div>

				<div class="ui-card">
					<p class="text-sm ui-muted">{notice}</p>
				</div>
			</div>
		</main>
	);
}
