import { GlobalShell } from "~/components/GlobalShell";

export default function NotFound() {
  return (
    <GlobalShell title="404">
      <div class="screenHead">
        <div class="screenTitle">
          <div class="eyebrow">404</div>
          <h1>Page not found</h1>
          <p class="ui-page-subtitle mx-auto max-w-2xl">
            This route does not exist, but you are still inside Ugoite. Choose a
            recovery path to get back to your spaces, login flow, or product
            overview.
          </p>
        </div>
        <div class="actions">
          <a href="/spaces" class="btn primary">
            Open Spaces
          </a>
          <a href="/login" class="btn">
            Go to Login
          </a>
          <a href="/" class="btn">
            Back to Home
          </a>
          <a href="/about" class="btn">
            About Ugoite
          </a>
        </div>
      </div>
      <p class="ui-muted">
        If you followed an outdated link, return to a known page above and
        continue with Ugoite's local-first knowledge workflows.
      </p>
    </GlobalShell>
  );
}
