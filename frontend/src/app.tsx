import { Router } from "@solidjs/router";
import { FileRoutes } from "@solidjs/start/router";
import { Show, Suspense, createResource } from "solid-js";
import { isServer } from "solid-js/web";
import Nav from "~/components/Nav";
import { initializePortablePreferences } from "~/lib/preferences-store";
import "./app.css";

export default function App() {
	const ready = isServer
		? () => true
		: createResource(async () => {
				await initializePortablePreferences();
				return true;
			})[0];

	return (
		<Show when={ready()}>
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
		</Show>
	);
}
