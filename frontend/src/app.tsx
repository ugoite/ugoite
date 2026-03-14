import { Router, useLocation } from "@solidjs/router";
import { FileRoutes } from "@solidjs/start/router";
import { Suspense, createEffect, type JSXElement } from "solid-js";
import Nav from "~/components/Nav";
import { initializePortablePreferencesForPath } from "~/lib/preferences-store";
import "./app.css";

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
	return (
		<Router root={(props) => <AppShell>{props.children}</AppShell>}>
			<FileRoutes />
		</Router>
	);
}
