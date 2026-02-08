import { A } from "@solidjs/router";

export default function Home() {
	return (
		<main class="ui-page text-center mx-auto">
			<h1 class="max-6-xs text-6xl font-thin uppercase my-16">IEapp</h1>
			<p class="text-xl mb-8 ui-muted">Your AI-native, programmable knowledge base</p>
			<div class="flex justify-center gap-4 flex-wrap">
				<A href="/spaces" class="ui-button ui-button-primary">
					Open Spaces
				</A>
				<A href="/about" class="ui-button ui-button-secondary">
					Learn More
				</A>
			</div>
			<div class="mt-16 grid grid-cols-1 md:grid-cols-3 gap-8 max-w-4xl mx-auto text-left">
				<div class="ui-card">
					<h3 class="text-lg font-semibold mb-2">📝 Structured Freedom</h3>
					<p class="ui-muted text-sm">
						Write in Markdown, get structured data. Use H2 headers to define properties like dates,
						status, and more.
					</p>
				</div>
				<div class="ui-card">
					<h3 class="text-lg font-semibold mb-2">🤖 AI Native</h3>
					<p class="ui-muted text-sm">
						Built for AI agents. Execute scripts, query your knowledge base, and automate complex
						workflows.
					</p>
				</div>
				<div class="ui-card">
					<h3 class="text-lg font-semibold mb-2">🎨 Infinite Canvas</h3>
					<p class="ui-muted text-sm">
						Organize entries visually on a 2D canvas. Create connections and see the big picture.
					</p>
				</div>
			</div>
		</main>
	);
}
