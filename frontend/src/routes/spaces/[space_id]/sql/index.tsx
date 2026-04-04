import { A, useParams } from "@solidjs/router";
import { SpaceShell } from "~/components/SpaceShell";

export default function SpaceSqlIndexRoute() {
	const params = useParams();
	const spaceId = () => params.space_id;
	return (
		<SpaceShell spaceId={spaceId()} activeTopTab="search">
			<div class="mx-auto max-w-4xl ui-stack">
				<section class="ui-card ui-stack">
					<div class="ui-stack-sm">
						<p class="text-sm ui-muted">Space: {spaceId()}</p>
						<h1 class="ui-page-title">Saved SQL</h1>
						<p class="ui-page-subtitle max-w-2xl">
							This route is reserved for named-query management, but that dedicated UI is not
							shipped yet. You can still run and revisit queries from Search today, then return here
							once saved-query management lands.
						</p>
					</div>

					<div class="ui-stack-sm">
						<h2 class="text-lg font-semibold">What works today</h2>
						<ul class="list-disc space-y-2 pl-5 text-sm ui-muted">
							<li>Run keyword and advanced searches from the Search tab.</li>
							<li>
								Reuse the current space dashboard when you need to switch back to entries or forms.
							</li>
							<li>
								Keep working in this space without guessing which route is actually supported.
							</li>
						</ul>
					</div>

					<div class="flex flex-wrap gap-3">
						<A href={`/spaces/${spaceId()}/search`} class="ui-button ui-button-primary">
							Open Search
						</A>
						<A href={`/spaces/${spaceId()}/dashboard`} class="ui-button ui-button-secondary">
							Back to Dashboard
						</A>
						<A href={`/spaces/${spaceId()}/entries`} class="ui-button ui-button-secondary">
							Browse Entries
						</A>
					</div>
				</section>
			</div>
		</SpaceShell>
	);
}
