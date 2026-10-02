import { useNavigate } from "@solidjs/router";
import { createSignal, onCleanup, onMount, Show } from "solid-js";
import { authApi } from "~/lib/ugoite-client";
import { t } from "~/lib/i18n";

export default function IndexRoute() {
  const navigate = useNavigate();
  const [checkingSession, setCheckingSession] = createSignal(true);
  let cancelled = false;

  onCleanup(() => cancelled = true);
  onMount(() => {
    void authApi.getSession().then((session) => {
      if (cancelled) return;
      if (session.authenticated) {
        navigate("/spaces", { replace: true });
      }
    }).catch(() => {
      // Visitors can still continue to the explicit sign-in route.
    }).finally(() => {
      if (!cancelled) setCheckingSession(false);
    });
  });

  return (
    <main class="loginShell">
      <section class="loginPanel">
        <h1 class="loginBrand">
          <img class="brandMark" src="/brand/ugoite-mark.svg" alt="" />
          <strong>Ugoite</strong>
        </h1>
        <Show when={checkingSession()}>
          <p class="loginStatus" role="status" aria-live="polite">
            {t("loginPage.checkingSession")}
          </p>
        </Show>
        <a
          class="btn primary"
          href="/login"
          ref={(element) => {
            queueMicrotask(() => element.focus());
          }}
        >
          {t("nav.login")}
        </a>
      </section>
    </main>
  );
}
