import { A } from "@solidjs/router";

export default function NotFound() {
	return (
		<main class="ui-page mx-auto max-w-4xl ui-stack">
			<section class="ui-card text-center ui-stack">
				<p class="text-sm font-semibold uppercase tracking-[0.2em] ui-muted">404</p>
				<div class="ui-stack-sm">
					<h1 class="ui-page-title">Page not found</h1>
					<p class="ui-page-subtitle mx-auto max-w-2xl">
						This route does not exist, but you are still inside Ugoite. Choose a recovery path to
						get back to your spaces, login flow, or product overview.
					</p>
				</div>
				<div class="flex flex-wrap justify-center gap-3">
					<A href="/spaces" class="ui-button ui-button-primary">
						Open Spaces
					</A>
					<A href="/login" class="ui-button ui-button-secondary">
						Go to Login
					</A>
					<A href="/" class="ui-button ui-button-secondary">
						Back to Home
					</A>
					<A href="/about" class="ui-button ui-button-secondary">
						About Ugoite
					</A>
				</div>
			</section>
			<p class="text-center text-sm ui-muted">
				If you followed an outdated link, return to a known page above and continue with Ugoite's
				local-first knowledge workflows.
			</p>
		</main>
	);
}
