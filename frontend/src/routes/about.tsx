import { A } from "@solidjs/router";
import { createMemo } from "solid-js";
import { t } from "~/lib/i18n";

export default function About() {
	const copy = createMemo(() => ({
		title: t("aboutPage.title"),
		subtitle: t("aboutPage.subtitle"),
		openSpaces: t("aboutPage.openSpaces"),
		backHome: t("aboutPage.backHome"),
		whatMakesDifferent: t("aboutPage.section.whatMakesDifferent"),
		localFirstTitle: t("aboutPage.card.localFirst.title"),
		localFirstDescription: t("aboutPage.card.localFirst.description"),
		markdownTitle: t("aboutPage.card.markdown.title"),
		markdownDescription: t("aboutPage.card.markdown.description"),
		aiTitle: t("aboutPage.card.ai.title"),
		aiDescription: t("aboutPage.card.ai.description"),
		howItWorks: t("aboutPage.section.howItWorks"),
		formsTitle: t("aboutPage.how.forms.title"),
		formsDescription: t("aboutPage.how.forms.description"),
		entriesTitle: t("aboutPage.how.entries.title"),
		entriesDescription: t("aboutPage.how.entries.description"),
		storageTitle: t("aboutPage.how.storage.title"),
		storageDescription: t("aboutPage.how.storage.description"),
		automationTitle: t("aboutPage.how.automation.title"),
		automationDescription: t("aboutPage.how.automation.description"),
		stack: t("aboutPage.section.stack"),
		stackFrontendLabel: t("aboutPage.stack.frontend.label"),
		stackFrontendValue: t("aboutPage.stack.frontend.value"),
		stackBackendLabel: t("aboutPage.stack.backend.label"),
		stackBackendValue: t("aboutPage.stack.backend.value"),
		stackCoreLabel: t("aboutPage.stack.core.label"),
		stackCoreValue: t("aboutPage.stack.core.value"),
		stackStorageLabel: t("aboutPage.stack.storage.label"),
		stackStorageValue: t("aboutPage.stack.storage.value"),
		stackAiLabel: t("aboutPage.stack.ai.label"),
		stackAiValue: t("aboutPage.stack.ai.value"),
	}));

	return (
		<main class="ui-page mx-auto">
			<section class="text-center">
				<h1 class="max-w-5xl text-4xl sm:text-6xl font-thin uppercase my-10 sm:my-16 mx-auto">
					{copy().title}
				</h1>
				<p class="text-base sm:text-xl ui-muted max-w-3xl mx-auto">{copy().subtitle}</p>
				<div class="mt-8 flex justify-center gap-3 flex-wrap">
					<A href="/spaces" class="ui-button ui-button-primary">{copy().openSpaces}</A>
					<A href="/" class="ui-button ui-button-secondary">{copy().backHome}</A>
				</div>
			</section>
			<h2 class="sr-only">{copy().whatMakesDifferent}</h2>
			<section class="mt-12 sm:mt-16 grid grid-cols-1 md:grid-cols-3 gap-6 sm:gap-8 max-w-5xl mx-auto">
				<div class="ui-card">
					<h3 class="text-lg font-semibold mb-2">{copy().localFirstTitle}</h3>
					<p class="ui-muted text-sm">{copy().localFirstDescription}</p>
				</div>
				<div class="ui-card">
					<h3 class="text-lg font-semibold mb-2">{copy().markdownTitle}</h3>
					<p class="ui-muted text-sm">{copy().markdownDescription}</p>
				</div>
				<div class="ui-card">
					<h3 class="text-lg font-semibold mb-2">{copy().aiTitle}</h3>
					<p class="ui-muted text-sm">{copy().aiDescription}</p>
				</div>
			</section>
			<section class="mt-12 sm:mt-16 max-w-5xl mx-auto">
				<h2 class="text-2xl font-semibold mb-4">{copy().howItWorks}</h2>
				<div class="grid grid-cols-1 md:grid-cols-2 gap-6">
					<div class="ui-card">
						<h3 class="text-lg font-semibold mb-2">{copy().formsTitle}</h3>
						<p class="ui-muted text-sm">{copy().formsDescription}</p>
					</div>
					<div class="ui-card">
						<h3 class="text-lg font-semibold mb-2">{copy().entriesTitle}</h3>
						<p class="ui-muted text-sm">{copy().entriesDescription}</p>
					</div>
					<div class="ui-card">
						<h3 class="text-lg font-semibold mb-2">{copy().storageTitle}</h3>
						<p class="ui-muted text-sm">{copy().storageDescription}</p>
					</div>
					<div class="ui-card">
						<h3 class="text-lg font-semibold mb-2">{copy().automationTitle}</h3>
						<p class="ui-muted text-sm">{copy().automationDescription}</p>
					</div>
				</div>
			</section>
			<section class="mt-12 sm:mt-16 max-w-5xl mx-auto ui-card">
				<h2 class="text-2xl font-semibold mb-3">{copy().stack}</h2>
				<ul class="ui-muted text-sm space-y-2">
					<li>
						<strong>{copy().stackFrontendLabel}:</strong> {copy().stackFrontendValue}
					</li>
					<li>
						<strong>{copy().stackBackendLabel}:</strong> {copy().stackBackendValue}
					</li>
					<li>
						<strong>{copy().stackCoreLabel}:</strong> {copy().stackCoreValue}
					</li>
					<li>
						<strong>{copy().stackStorageLabel}:</strong> {copy().stackStorageValue}
					</li>
					<li>
						<strong>{copy().stackAiLabel}:</strong> {copy().stackAiValue}
					</li>
				</ul>
			</section>
		</main>
	);
}
