import { useLocation, useNavigate } from "@solidjs/router";
import { createEffect, createSignal, onCleanup } from "solid-js";
import {
	authSessionChangedEventName,
	clearAuthTokenCookie,
	hasAuthTokenCookie,
} from "~/lib/auth-session";
import { t } from "~/lib/i18n";

export default function Nav() {
	const location = useLocation();
	const navigate = useNavigate();
	const active = (path: string) => path === location.pathname;
	const [authenticated, setAuthenticated] = createSignal(false);
	const syncAuthState = () => setAuthenticated(hasAuthTokenCookie());

	createEffect(() => {
		location.pathname;
		syncAuthState();
	});

	if (typeof window !== "undefined") {
		window.addEventListener(authSessionChangedEventName, syncAuthState);
		onCleanup(() => {
			window.removeEventListener(authSessionChangedEventName, syncAuthState);
		});
	}

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
				<li class="ui-nav-session">
					{authenticated() ? (
						<>
							<span class="ui-pill" aria-live="polite">
								{t("nav.signedIn")}
							</span>
							<button
								type="button"
								class="ui-button ui-button-secondary ui-button-sm"
								onClick={() => {
									clearAuthTokenCookie();
									setAuthenticated(false);
									void navigate("/login", { replace: true });
								}}
							>
								{t("nav.signOut")}
							</button>
						</>
					) : (
						<a
							href="/login"
							class="ui-nav-link"
							classList={{ "ui-nav-link-active": active("/login") }}
						>
							{t("nav.login")}
						</a>
					)}
				</li>
			</ul>
		</nav>
	);
	/* v8 ignore start */
}
/* v8 ignore stop */
