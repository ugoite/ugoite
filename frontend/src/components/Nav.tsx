import { useLocation } from "@solidjs/router";
import { createMemo } from "solid-js";
import { t } from "~/lib/i18n";

export default function Nav() {
	const location = useLocation();
	const active = (path: string) => path === location.pathname;
	const homeLabel = createMemo(() => t("nav.home"), undefined, { equals: false });
	const spacesLabel = createMemo(() => t("nav.spaces"), undefined, { equals: false });
	const loginLabel = createMemo(() => t("nav.login"), undefined, { equals: false });
	const aboutLabel = createMemo(() => t("nav.about"), undefined, { equals: false });

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
						<span>{homeLabel()}</span>
					</a>
				</li>
				<li>
					<a
						href="/spaces"
						class="ui-nav-link"
						classList={{ "ui-nav-link-active": active("/spaces") }}
					>
						<span>{spacesLabel()}</span>
					</a>
				</li>
				<li>
					<a
						href="/login"
						class="ui-nav-link"
						classList={{ "ui-nav-link-active": active("/login") }}
					>
						<span>{loginLabel()}</span>
					</a>
				</li>
				<li>
					<a
						href="/about"
						class="ui-nav-link"
						classList={{ "ui-nav-link-active": active("/about") }}
					>
						<span>{aboutLabel()}</span>
					</a>
				</li>
			</ul>
		</nav>
	);
	/* v8 ignore start */
}
/* v8 ignore stop */
