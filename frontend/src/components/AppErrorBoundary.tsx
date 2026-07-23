import { ErrorBoundary, type JSX } from "solid-js";
import { locale } from "~/lib/i18n";

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

export function AppErrorBoundary(props: { children: JSX.Element }) {
  const labels = () => copy[locale() === "ja" ? "ja" : "en"];
  return (
    <ErrorBoundary
      fallback={(_error, reset) => (
        <main class="content" role="alert">
          <section class="settingsMain surface ui-stack-sm">
            <h1>{labels().title}</h1>
            <p class="ui-muted">{labels().body}</p>
            <div class="flex flex-wrap gap-3">
              <button class="btn primary" type="button" onClick={reset}>
                {labels().retry}
              </button>
              <a class="btn" href="/spaces">{labels().spaces}</a>
            </div>
          </section>
        </main>
      )}
    >
      {props.children}
    </ErrorBoundary>
  );
}
