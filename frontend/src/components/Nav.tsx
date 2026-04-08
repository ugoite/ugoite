import { useLocation, useNavigate } from "@solidjs/router";
import { createMemo, createResource, createSignal, onCleanup } from "solid-js";
import { authApi } from "~/lib/auth-api";
import { t } from "~/lib/i18n";

const toMessage = (error: unknown, fallback: string): string => {
	if (error instanceof Error && error.message.trim()) {
		return error.message;
	}
	return fallback;
};

export default function Nav() {
	const location = useLocation();
	const navigate = useNavigate();
	const active = (path: string) => path === location.pathname;
	// The browser auth cookie is HttpOnly, so the nav has to ask the server whether
	// the current request still carries a session cookie instead of reading it in JS.
	const [sessionError, setSessionError] = createSignal("");
	const [authSession, { refetch }] = createResource(
		() => location.pathname,
		async (_pathname, info) => {
			try {
				const session = await authApi.getSession();
				setSessionError("");
				return session;
			} catch (error) {
				setSessionError(toMessage(error, "Failed to load auth session."));
				return info.value ?? { authenticated: false };
			}
		},
	);
	const homeLabel = createMemo(() => t("nav.home"), undefined, { equals: false });
	const spacesLabel = createMemo(() => t("nav.spaces"), undefined, { equals: false });
	const loginLabel = createMemo(() => t("nav.login"), undefined, { equals: false });
	const aboutLabel = createMemo(() => t("nav.about"), undefined, { equals: false });
	const authenticated = () => authSession()?.authenticated ?? false;

	if (typeof window !== "undefined") {
		const refreshAuthState = () => {
			void refetch();
		};
		const handleVisibilityChange = () => {
			if (document.visibilityState === "visible") {
				refreshAuthState();
			}
		};
		const intervalId = window.setInterval(refreshAuthState, 30_000);
		window.addEventListener("focus", refreshAuthState);
		document.addEventListener("visibilitychange", handleVisibilityChange);
		onCleanup(() => {
			window.clearInterval(intervalId);
			window.removeEventListener("focus", refreshAuthState);
			document.removeEventListener("visibilitychange", handleVisibilityChange);
		});
	}

	const handleSignOut = async () => {
		setSessionError("");
		try {
			await authApi.clearSession();
			await refetch();
			navigate("/login", { replace: true });
		} catch (error) {
			setSessionError(toMessage(error, "Failed to sign out."));
		}
	};

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
						href="/about"
						class="ui-nav-link"
						classList={{ "ui-nav-link-active": active("/about") }}
					>
						<span>{aboutLabel()}</span>
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
									void handleSignOut();
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
							{loginLabel()}
						</a>
					)}
					{sessionError() ? (
						<p class="ui-alert ui-alert-error text-xs" role="alert">
							{sessionError()}
						</p>
					) : null}
				</li>
			</ul>
		</nav>
	);
	/* v8 ignore start */
}
/* v8 ignore stop */
