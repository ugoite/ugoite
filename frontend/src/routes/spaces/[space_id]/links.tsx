import { A, useParams } from "@solidjs/router";

export default function SpaceLinksRoute() {
	const params = useParams<{ space_id: string }>();
	const spaceId = () => params.space_id;
	const notice = "Links are no longer managed via API. Use row_reference fields instead.";
	return (
		<main class="ui-shell ui-page">
			<div class="max-w-4xl mx-auto p-6">
				<div class="flex items-center justify-between mb-6">
					<h1 class="ui-page-title">Links</h1>
					<A href={`/spaces/${spaceId()}/entries`} class="text-sm">
						Back to Entries
					</A>
				</div>

				<section class="ui-card">
					<h2 class="text-lg font-semibold mb-2">Links API removed</h2>
					<p class="text-sm ui-muted">{notice}</p>
				</section>
			</div>
		</main>
	);
}
