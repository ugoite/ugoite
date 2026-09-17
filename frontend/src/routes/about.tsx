import { onMount } from "solid-js";
import { getDocsiteHref } from "~/lib/docsite-links";

export const aboutDocsHref = getDocsiteHref(
  "/docs/guide/start",
  "docs/guide/start/index.md",
);

// The in-app About page was removed: documentation is the single authority
// for product overview copy. Bookmarks and deep links to /about land here
// and continue to Docs; there is no About entry in any navigation shell.
export default function AboutRedirectRoute() {
  onMount(() => {
    window.location.replace(aboutDocsHref);
  });

  return (
    <main class="ui-page text-center mx-auto">
      <h1 class="max-w-6xl text-4xl sm:text-6xl font-thin uppercase my-10 sm:my-16">
        Ugoite
      </h1>
      <p class="text-base sm:text-xl mb-6 sm:mb-8 ui-muted">
        <a href={aboutDocsHref} class="ui-button ui-button-primary">
          Docs
        </a>
      </p>
    </main>
  );
}
