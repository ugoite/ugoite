import { A } from "@solidjs/router";

export default function About() {
	return (
		<main class="ui-page mx-auto">
			<section class="text-center">
				<h1 class="max-w-5xl text-4xl sm:text-6xl font-thin uppercase my-10 sm:my-16 mx-auto">
					About Ugoite
				</h1>
				<p class="text-base sm:text-xl ui-muted max-w-3xl mx-auto">
					Ugoite is a local-first, AI-native knowledge space designed for flexible structure and
					fast retrieval. Your data stays yours, while forms and SQL make it queryable.
				</p>
				<div class="mt-8 flex justify-center gap-3 flex-wrap">
					<A href="/spaces" class="ui-button ui-button-primary">
						Open Spaces
					</A>
					<A href="/" class="ui-button ui-button-secondary">
						Back to Home
					</A>
				</div>
			</section>
			<section class="mt-12 sm:mt-16 grid grid-cols-1 md:grid-cols-3 gap-6 sm:gap-8 max-w-5xl mx-auto">
				<div class="ui-card">
					<h3 class="text-lg font-semibold mb-2">Local-first ownership</h3>
					<p class="ui-muted text-sm">
						Store knowledge on local or cloud storage without vendor lock-in. You control where data
						lives and how it moves.
					</p>
				</div>
				<div class="ui-card">
					<h3 class="text-lg font-semibold mb-2">Markdown, structured</h3>
					<p class="ui-muted text-sm">
						Write naturally in Markdown while Forms define fields. Entries are stored as Iceberg
						tables for reliable querying.
					</p>
				</div>
				<div class="ui-card">
					<h3 class="text-lg font-semibold mb-2">AI-native workflows</h3>
					<p class="ui-muted text-sm">
						Integrate with agents through MCP for resource-first automation and custom scripts that
						operate on your knowledge.
					</p>
				</div>
			</section>
			<section class="mt-12 sm:mt-16 max-w-5xl mx-auto">
				<h2 class="text-2xl font-semibold mb-4">How it works</h2>
				<div class="grid grid-cols-1 md:grid-cols-2 gap-6">
					<div class="ui-card">
						<h3 class="text-lg font-semibold mb-2">Forms define structure</h3>
						<p class="ui-muted text-sm">
							Create Forms to define field types and defaults. Each Entry inherits a predictable
							schema for reliable search and analytics.
						</p>
					</div>
					<div class="ui-card">
						<h3 class="text-lg font-semibold mb-2">Entries stay queryable</h3>
						<p class="ui-muted text-sm">
							Entries are stored as rows so you can filter, sort, and join using Ugoite SQL without
							losing Markdown readability.
						</p>
					</div>
					<div class="ui-card">
						<h3 class="text-lg font-semibold mb-2">Storage that scales</h3>
						<p class="ui-muted text-sm">
							Iceberg tables plus OpenDAL keep storage portable across local disks or object stores
							with consistent performance.
						</p>
					</div>
					<div class="ui-card">
						<h3 class="text-lg font-semibold mb-2">Built for automation</h3>
						<p class="ui-muted text-sm">
							From MCP to CLI workflows, Ugoite is designed to be scripted and orchestrated by
							agents and power users.
						</p>
					</div>
				</div>
			</section>
			<section class="mt-12 sm:mt-16 max-w-5xl mx-auto ui-card">
				<h2 class="text-2xl font-semibold mb-3">Technology stack</h2>
				<ul class="ui-muted text-sm space-y-2">
					<li>
						<strong>Frontend:</strong> SolidStart + Tailwind CSS
					</li>
					<li>
						<strong>Backend:</strong> FastAPI (Python 3.12+)
					</li>
					<li>
						<strong>Core:</strong> Rust with pyo3 bindings
					</li>
					<li>
						<strong>Storage:</strong> Apache Iceberg + OpenDAL
					</li>
					<li>
						<strong>AI:</strong> MCP resource-first integrations
					</li>
				</ul>
			</section>
		</main>
	);
}
