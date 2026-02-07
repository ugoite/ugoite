import { Router } from "@solidjs/router";
import { FileRoutes } from "@solidjs/start/router";
import { Suspense, onMount } from "solid-js";
import Nav from "~/components/Nav";
import { applyTheme, resolveTheme } from "~/lib/theme";
import "./app.css";

export default function App() {
	onMount(() => {
		const { theme, tone } = resolveTheme();
		applyTheme(theme, tone);
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
