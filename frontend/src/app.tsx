import { Router } from "@solidjs/router";
import { FileRoutes } from "@solidjs/start/router";
import { Suspense, onMount } from "solid-js";
import Nav from "~/components/Nav";
import { initializeLocale } from "~/lib/i18n";
import { initializeUiTheme } from "~/lib/ui-theme";
import "./app.css";

export default function App() {
	onMount(() => {
		initializeUiTheme();
		initializeLocale();
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
