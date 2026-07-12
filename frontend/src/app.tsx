import { Router } from "@solidjs/router";
import { FileRoutes } from "@solidjs/start/router";
import { type JSXElement, Suspense } from "solid-js";
import { primePortablePreferencesFromLocal } from "~/lib/preferences-store";
import "./app.css";

let clientPreferencesPrimed = false;

const primeClientPreferences = () => {
  if (clientPreferencesPrimed || typeof window === "undefined") {
    return;
  }
  primePortablePreferencesFromLocal();
  clientPreferencesPrimed = true;
};

const conceptScreenForPath = (path: string) => {
  if (path === "/login") return "login";
  if (path === "/recover") return "recover";
  if (path === "/device") return "device";
  if (path === "/setup") return "setup";
  if (path === "/spaces" || path === "/spaces/") return "spaces";
  if (path.endsWith("/join")) return "space-join";
  if (path.includes("/dashboard")) return "home";
  if (path.includes("/settings/security")) return "settings-credentials";
  if (path.includes("/settings")) return "settings-general";
  if (path.includes("/search")) return "search";
  if (path.includes("/queries/") && path.includes("/variables")) return "query-vars";
  if (path.includes("/queries/new")) return "sql-detail";
  if (path.includes("/sql/") && !path.endsWith("/sql")) return "sql-detail";
  if (path.endsWith("/sql") || path.includes("/query")) return "sql-list";
  if (path.includes("/assets/")) return "form-assets";
  if (path.endsWith("/assets")) return "form-assets";
  if (path.includes("/history/")) return "revision";
  if (path.endsWith("/history")) return "entry-history";
  if (path.endsWith("/restore")) return "restore";
  if (path.includes("/entries/") && !path.endsWith("/entries")) return "entry-detail";
  if (path.includes("/forms/types")) return "form-fields";
  if (path.includes("/forms/") && !path.endsWith("/forms")) return "form-entries";
  if (path.includes("/forms") || path.endsWith("/entries")) return "forms";
  if (path === "/about") return "about";
  return "not-found";
};

export default function App() {
  primeClientPreferences();

  return (
    <Router
      root={(props) => (
        <ConceptFrame>{props.children}</ConceptFrame>
      )}
    >
      <Suspense>
        <FileRoutes />
      </Suspense>
    </Router>
  );
}

function ConceptFrame(props: { children: JSXElement }) {
  const path = typeof window === "undefined" ? "/" : window.location.pathname;
  const screen = conceptScreenForPath(path);
  return (
    <div class="ui-concept-host">
      <iframe
        title="Ugoite"
        src={`/ui-concept-v5.html#${screen}`}
        class="ui-concept-frame"
      />
      <div class="ui-concept-routes" aria-hidden="true">{props.children}</div>
    </div>
  );
}
