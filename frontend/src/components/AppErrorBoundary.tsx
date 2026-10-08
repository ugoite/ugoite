import { ErrorBoundary, onMount, type JSX } from "solid-js";
import { locale } from "~/lib/i18n";
import { recoverFromRouteChunkFailure } from "~/lib/vite-preload-recovery";

const copy = {
  en: {
    title: "This page could not be displayed",
    body: "An unexpected error occurred. You can retry this page or return to your Spaces.",
    retry: "Try again",
    spaces: "Back to Spaces",
  },
  ja: {
    title: "ページを表示できませんでした",
    body: "予期しないエラーが発生しました。このページを再試行するか、スペース一覧へ戻ってください。",
    retry: "再試行",
    spaces: "スペース一覧へ戻る",
  },
} as const;

type ErrorCopy = typeof copy.en;

function AppErrorFallback(props: {
  error: unknown;
  reset: () => void;
  labels: ErrorCopy;
}) {
  onMount(() => {
    if (typeof window === "undefined") return;
    try {
      recoverFromRouteChunkFailure(
        props.error,
        window.sessionStorage,
        () => window.location.reload(),
        window.location.pathname,
      );
    } catch {
      // Keep the normal fallback when browser storage is unavailable.
    }
  });

  return (
    <main class="content" role="alert">
      <section class="settingsMain surface ui-stack-sm">
        <h1>{props.labels.title}</h1>
        <p class="ui-muted">{props.labels.body}</p>
        <div class="flex flex-wrap gap-3">
          <button class="btn primary" type="button" onClick={props.reset}>
            {props.labels.retry}
          </button>
          <a class="btn" href="/spaces">{props.labels.spaces}</a>
        </div>
      </section>
    </main>
  );
}

export function AppErrorBoundary(props: { children: JSX.Element }) {
  const labels = () => copy[locale() === "ja" ? "ja" : "en"];
  return (
    <ErrorBoundary
      fallback={(error, reset) => (
        <AppErrorFallback error={error} reset={reset} labels={labels()} />
      )}
    >
      {props.children}
    </ErrorBoundary>
  );
}
