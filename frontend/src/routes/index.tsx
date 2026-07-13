import { useNavigate } from "@solidjs/router";
import { onMount } from "solid-js";
import { authApi } from "~/lib/ugoite-client";

export default function IndexRoute() {
  const navigate = useNavigate();
  onMount(() => {
    void authApi.getSession().then((session) => {
      navigate(session.authenticated ? "/spaces" : "/login", { replace: true });
    }).catch(() => navigate("/login", { replace: true }));
  });
  return <main class="publicShell"><section class="publicCard ui-muted">Opening Ugoite…</section></main>;
}
