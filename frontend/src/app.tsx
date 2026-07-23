import { Router } from "@solidjs/router";
import { FileRoutes } from "@solidjs/start/router";
import { type JSXElement, Suspense } from "solid-js";
import { AppErrorBoundary } from "~/components/AppErrorBoundary";
import Nav from "~/components/Nav";
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

export default function App() {
  primeClientPreferences();

  return (
    <Router
      root={(props) => (
        <>
          <Nav />
          <AppErrorBoundary>{props.children}</AppErrorBoundary>
        </>
      )}
    >
      <Suspense>
        <FileRoutes />
      </Suspense>
    </Router>
  );
}
