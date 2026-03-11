import { Router } from "@solidjs/router";
import { FileRoutes } from "@solidjs/start/router";
import { Suspense, onMount } from "solid-js";
import Nav from "~/components/Nav";
import { initializePortablePreferences } from "~/lib/preferences-store";
import "./app.css";

export default function App() {
	onMount(() => {
		void initializePortablePreferences();
	});

	return (
		<Router
			root={(props) => (
				<>
					<Nav />
					<Suspense>{props.children}</Suspense>
				</>
			)}
		>
			<FileRoutes />
		</Router>
	);
}
