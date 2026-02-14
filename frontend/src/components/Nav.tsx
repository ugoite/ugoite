import { useLocation } from "@solidjs/router";
import { t } from "~/lib/i18n";

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
						{t("nav.home")}
					</a>
				</li>
				<li>
					<a
						href="/spaces"
						class="ui-nav-link"
						classList={{ "ui-nav-link-active": active("/spaces") }}
					>
						{t("nav.spaces")}
					</a>
				</li>
				<li>
					<a
						href="/about"
						class="ui-nav-link"
						classList={{ "ui-nav-link-active": active("/about") }}
					>
						{t("nav.about")}
					</a>
				</li>
			</ul>
		</nav>
	);
}
