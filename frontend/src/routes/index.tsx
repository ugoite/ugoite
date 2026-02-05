import { A } from "@solidjs/router";

export default function Home() {
	return (
		<main class="text-center mx-auto text-gray-700 p-4">
			<h1 class="max-6-xs text-6xl text-sky-700 font-thin uppercase my-16">IEapp</h1>
			<p class="text-xl mb-8 text-gray-600">Your AI-native, programmable knowledge base</p>
			<div class="flex justify-center gap-4">
				<A
					href="/spaces"
					class="px-6 py-3 bg-sky-600 text-white rounded-lg hover:bg-sky-700 transition-colors"
				>
					Open Spaces
				</A>
				<A
					href="/about"
					class="px-6 py-3 border border-sky-600 text-sky-600 rounded-lg hover:bg-sky-50 transition-colors"
				>
					Learn More
				</A>
			</div>
			<div class="mt-16 grid grid-cols-1 md:grid-cols-3 gap-8 max-w-4xl mx-auto text-left">
				<div class="p-6 bg-white rounded-lg shadow">
					<h3 class="text-lg font-semibold mb-2">📝 Structured Freedom</h3>
					<p class="text-gray-600 text-sm">
						Write in Markdown, get structured data. Use H2 headers to define properties like dates,
						status, and more.
					</p>
				</div>
				<div class="p-6 bg-white rounded-lg shadow">
					<h3 class="text-lg font-semibold mb-2">🤖 AI Native</h3>
					<p class="text-gray-600 text-sm">
						Built for AI agents. Execute scripts, query your knowledge base, and automate complex
						workflows.
					</p>
				</div>
				<div class="p-6 bg-white rounded-lg shadow">
					<h3 class="text-lg font-semibold mb-2">🎨 Infinite Canvas</h3>
					<p class="text-gray-600 text-sm">
						Organize entries visually on a 2D canvas. Create connections and see the big picture.
					</p>
				</div>
			</div>
		</main>
	);
}
