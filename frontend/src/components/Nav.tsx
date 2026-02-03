import { useLocation } from "@solidjs/router";

export default function Nav() {
	const location = useLocation();
	const active = (path: string) =>
		path === location.pathname ? "border-sky-600" : "border-transparent hover:border-sky-600";

	const isWorkspaceExplorer =
		location.pathname.includes("/workspaces/") && !location.pathname.endsWith("/workspaces");

	// Hide nav on workspace explorer pages (they have their own navigation)
	if (isWorkspaceExplorer) {
		return null;
	}

	return (
		<nav class="bg-sky-800">
			<ul class="container flex items-center p-3 text-gray-200">
				<li class={`border-b-2 ${active("/")} mx-1.5 sm:mx-6`}>
					<a href="/">Home</a>
				</li>
				<li class={`border-b-2 ${active("/workspaces")} mx-1.5 sm:mx-6`}>
					<a href="/workspaces">Workspaces</a>
				</li>
				<li class={`border-b-2 ${active("/about")} mx-1.5 sm:mx-6`}>
					<a href="/about">About</a>
				</li>
			</ul>
		</nav>
	);
}
