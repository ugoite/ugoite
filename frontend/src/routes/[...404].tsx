import { GlobalShell } from "~/components/GlobalShell";
import { getDocsiteHref } from "~/lib/docsite-links";

const docsHref = getDocsiteHref(
  "/docs/get-started",
  "docs/get-started/index.md",
);

export default function NotFound() {
  return (
    <GlobalShell authenticated={false}>
      <div class="screenHead">
        <div class="screenTitle">
          <h1>Page not found</h1>
        </div>
        <div class="actions">
          <a href="/" class="btn primary">Home</a>
          <a href={docsHref} class="btn" target="_blank" rel="noopener">
            Docs
          </a>
        </div>
      </div>
    </GlobalShell>
  );
}
