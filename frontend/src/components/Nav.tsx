import { useLocation } from "@solidjs/router";

export default function Nav() {
	const location = useLocation();
	const active = (path: string) => path === location.pathname;

	const isSpaceExplorer =
		location.pathname.includes("/spaces/") && !location.pathname.endsWith("/spaces");

	// Hide nav on space explorer pages (they have their own navigation)
	if (isSpaceExplorer) {
		return null;
	}

	return (
		<nav class="ui-nav">
			<ul class="ui-nav-list">
				<li>
					<a href="/" class="ui-nav-link" classList={{ "ui-nav-link-active": active("/") }}>
						Home
					</a>
				</li>
				<li>
					<a
						href="/spaces"
						class="ui-nav-link"
						classList={{ "ui-nav-link-active": active("/spaces") }}
					>
						Spaces
					</a>
				</li>
				<li>
					<a
						href="/about"
						class="ui-nav-link"
						classList={{ "ui-nav-link-active": active("/about") }}
					>
						About
					</a>
				</li>
			</ul>
		</nav>
	);
}
