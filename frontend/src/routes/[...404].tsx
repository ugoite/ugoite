import { A } from "@solidjs/router";

export default function NotFound() {
	return (
		<main class="ui-page text-center mx-auto">
			<h1 class="max-6-xs text-6xl font-thin uppercase my-16">Not Found</h1>
			<p class="mt-8">
				Visit{" "}
				<a
					href="https://solidjs.com"
					target="_blank"
					class="ui-muted hover:underline"
					rel="noopener"
				>
					solidjs.com
				</a>{" "}
				to learn how to build Solid apps.
			</p>
			<p class="my-4">
				<A href="/" class="ui-muted hover:underline">
					Home
				</A>
				{" - "}
				<A href="/about" class="ui-muted hover:underline">
					About Page
				</A>
			</p>
		</main>
	);
}
