import { A } from "@solidjs/router";

const learnMoreHref =
	process.env.NODE_ENV === "development"
		? "http://localhost:4321/getting-started"
		: "https://ugoite.github.io/ugoite/getting-started";

export default function Home() {
	return (
		<main class="ui-page text-center mx-auto">
			<h1 class="max-w-6xl text-4xl sm:text-6xl font-thin uppercase my-10 sm:my-16">Ugoite</h1>
			<p class="text-base sm:text-xl mb-6 sm:mb-8 ui-muted">
				Local-first knowledge, structured for search and automation
			</p>
			<div class="flex justify-center gap-3 sm:gap-4 flex-wrap">
				<A href="/login" class="ui-button ui-button-secondary">
					Log in
				</A>
				<A href="/spaces" class="ui-button ui-button-primary">
					Open Spaces
				</A>
				<a href={learnMoreHref} class="ui-button ui-button-secondary">
					Learn More
				</a>
			</div>
			<div class="mt-12 sm:mt-16 grid grid-cols-1 md:grid-cols-3 gap-6 sm:gap-8 max-w-4xl mx-auto text-left">
				<div class="ui-card">
					<h3 class="text-lg font-semibold mb-2">📝 Markdown, structured</h3>
					<p class="ui-muted text-sm">
						Write naturally while Forms define the schema. Entries stay readable and queryable.
					</p>
				</div>
				<div class="ui-card">
					<h3 class="text-lg font-semibold mb-2">🤖 AI-native workflows</h3>
					<p class="ui-muted text-sm">
						Automate with MCP and the CLI. Agents can query, summarize, and orchestrate tasks.
					</p>
				</div>
				<div class="ui-card">
					<h3 class="text-lg font-semibold mb-2">🔒 Local-first control</h3>
					<p class="ui-muted text-sm">
						Store knowledge on local or cloud storage without lock-in. You own the data.
					</p>
				</div>
			</div>
		</main>
	);
}
