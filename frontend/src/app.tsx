import { Router, useLocation } from "@solidjs/router";
import { FileRoutes } from "@solidjs/start/router";
import { type JSXElement, Suspense } from "solid-js";
import { AppErrorBoundary } from "~/components/AppErrorBoundary";
import { AuthGate } from "~/components/AuthGate";
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

function AppRoot(props: { children: JSXElement }) {
  const location = useLocation();

  return (
    <>
      <Nav />
      <AuthGate>
        <AppErrorBoundary pathname={location.pathname}>
          {props.children}
        </AppErrorBoundary>
      </AuthGate>
    </>
  );
}

export default function App() {
  primeClientPreferences();

  return (
    <Router root={(props) => <AppRoot>{props.children}</AppRoot>}>
      <Suspense>
        <FileRoutes />
      </Suspense>
    </Router>
  );
}
