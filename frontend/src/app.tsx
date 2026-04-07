import { Router, useLocation } from "@solidjs/router";
import { FileRoutes } from "@solidjs/start/router";
import { Suspense, createEffect, type JSXElement } from "solid-js";
import Nav from "~/components/Nav";
import {
	initializePortablePreferencesForPath,
	primePortablePreferencesFromLocal,
} from "~/lib/preferences-store";
import "./app.css";

let clientPreferencesPrimed = false;

const primeClientPreferences = () => {
	if (clientPreferencesPrimed || typeof window === "undefined") {
		return;
	}
	primePortablePreferencesFromLocal();
	clientPreferencesPrimed = true;
};

function AppShell(props: { children: JSXElement }) {
	const location = useLocation();

	createEffect(() => {
		void initializePortablePreferencesForPath(location.pathname);
	});

	return (
		<>
			<Nav />
			<Suspense>{props.children}</Suspense>
		</>
	);
}

export default function App() {
	primeClientPreferences();

	return (
		<Router root={(props) => <AppShell>{props.children}</AppShell>}>
			<FileRoutes />
		</Router>
	);
}
