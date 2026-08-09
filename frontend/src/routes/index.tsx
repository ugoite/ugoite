import { useNavigate } from "@solidjs/router";
import { onCleanup, onMount } from "solid-js";
import { authApi } from "~/lib/ugoite-client";

export default function IndexRoute() {
  const navigate = useNavigate();
  let cancelled = false;
  onCleanup(() => cancelled = true);
  onMount(() => {
    void authApi.getSession().then((session) => {
      if (cancelled) return;
      navigate(session.authenticated ? "/spaces" : "/login", { replace: true });
    }).catch(() => {
      if (!cancelled) navigate("/login", { replace: true });
    });
  });
  return (
    <main class="publicShell">
      <section class="publicCard ui-muted">Opening Ugoite…</section>
    </main>
  );
}
