import { A } from "@solidjs/router";
import { createMemo } from "solid-js";
import { t } from "~/lib/i18n";

export default function Home() {
	const copy = createMemo(() => ({
		subtitle: t("homePage.subtitle"),
		login: t("homePage.login"),
		openSpaces: t("homePage.openSpaces"),
		learnMore: t("homePage.learnMore"),
		markdownTitle: t("homePage.card.markdown.title"),
		markdownDescription: t("homePage.card.markdown.description"),
		aiTitle: t("homePage.card.ai.title"),
		aiDescription: t("homePage.card.ai.description"),
		localFirstTitle: t("homePage.card.localFirst.title"),
		localFirstDescription: t("homePage.card.localFirst.description"),
	}));

	return (
		<main class="ui-page text-center mx-auto">
			<h1 class="max-w-6xl text-4xl sm:text-6xl font-thin uppercase my-10 sm:my-16">Ugoite</h1>
			<p class="text-base sm:text-xl mb-6 sm:mb-8 ui-muted">{copy().subtitle}</p>
			<div class="flex justify-center gap-3 sm:gap-4 flex-wrap">
				<A href="/login" class="ui-button ui-button-secondary">{copy().login}</A>
				<A href="/spaces" class="ui-button ui-button-primary">{copy().openSpaces}</A>
				<A href="/about" class="ui-button ui-button-secondary">{copy().learnMore}</A>
			</div>
			<div class="mt-12 sm:mt-16 grid grid-cols-1 md:grid-cols-3 gap-6 sm:gap-8 max-w-4xl mx-auto text-left">
				<div class="ui-card">
					<h3 class="text-lg font-semibold mb-2">{copy().markdownTitle}</h3>
					<p class="ui-muted text-sm">{copy().markdownDescription}</p>
				</div>
				<div class="ui-card">
					<h3 class="text-lg font-semibold mb-2">{copy().aiTitle}</h3>
					<p class="ui-muted text-sm">{copy().aiDescription}</p>
				</div>
				<div class="ui-card">
					<h3 class="text-lg font-semibold mb-2">{copy().localFirstTitle}</h3>
					<p class="ui-muted text-sm">{copy().localFirstDescription}</p>
				</div>
			</div>
		</main>
	);
}
