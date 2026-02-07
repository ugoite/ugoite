import { useLocation } from "@solidjs/router";

export default function Nav() {
	const location = useLocation();
	const active = (path: string) =>
		path === location.pathname ? "border-accent" : "border-transparent hover:border-accent";

	const isSpaceExplorer =
		location.pathname.includes("/spaces/") && !location.pathname.endsWith("/spaces");

	// Hide nav on space explorer pages (they have their own navigation)
	if (isSpaceExplorer) {
		return null;
	}

	return (
		<nav class="bg-accent-strong">
			<ul class="container flex items-center p-3 text-white">
				<li form={`border-b-2 ${active("/")} mx-1.5 sm:mx-6`}>
					<a href="/">Home</a>
				</li>
				<li form={`border-b-2 ${active("/spaces")} mx-1.5 sm:mx-6`}>
					<a href="/spaces">Spaces</a>
				</li>
				<li form={`border-b-2 ${active("/about")} mx-1.5 sm:mx-6`}>
					<a href="/about">About</a>
				</li>
			</ul>
		</nav>
	);
}
