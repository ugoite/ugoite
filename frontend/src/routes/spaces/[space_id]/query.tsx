import { A, useParams } from "@solidjs/router";
import { SpaceShell } from "~/components/SpaceShell";

export default function SpaceQueryRoute() {
	const params = useParams<{ space_id: string }>();
	const spaceId = () => params.space_id;

	return (
		<SpaceShell spaceId={spaceId()} activeTopTab="search">
			<div class="mx-auto max-w-4xl ui-stack">
				<section class="ui-card ui-stack">
					<div class="ui-stack-sm">
						<p class="text-sm ui-muted">Space: {spaceId()}</p>
						<h1 class="ui-page-title">Query moved to Search</h1>
						<p class="ui-page-subtitle max-w-2xl">
							This legacy <code>/query</code> URL is kept only so older bookmarks and stale links do
							not fail. Ugoite&apos;s supported query and search flow now lives under{" "}
							<code>/search</code>, where keyword search, advanced filters, and query history are
							available together.
						</p>
					</div>

					<div class="ui-stack-sm">
						<h2 class="text-lg font-semibold">What to do next</h2>
						<ul class="list-disc space-y-2 pl-5 text-sm ui-muted">
							<li>Open Search to continue with the supported query experience.</li>
							<li>
								Update old bookmarks or shared links that still point to <code>/query</code>.
							</li>
							<li>Return to the dashboard if you were only trying to get back into this space.</li>
						</ul>
					</div>

					<div class="flex flex-wrap gap-3">
						<A href={`/spaces/${spaceId()}/search`} class="ui-button ui-button-primary">
							Open Search
						</A>
						<A href={`/spaces/${spaceId()}/dashboard`} class="ui-button ui-button-secondary">
							Back to Dashboard
						</A>
					</div>
				</section>
			</div>
		</SpaceShell>
	);
}
