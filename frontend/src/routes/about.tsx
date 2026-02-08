import { A } from "@solidjs/router";
import Counter from "~/components/Counter";

export default function About() {
	return (
		<main class="ui-page text-center mx-auto">
			<h1 class="max-6-xs text-6xl font-thin uppercase my-16">About Page</h1>
			<Counter />
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
				<span>About Page</span>
			</p>
		</main>
	);
}
