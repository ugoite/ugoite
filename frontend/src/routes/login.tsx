import { A, useNavigate, useSearchParams } from "@solidjs/router";
import { createResource, createSignal, Show } from "solid-js";
import { authApi, type AuthLoginResponse } from "~/lib/auth-api";
import { setAuthTokenCookie } from "~/lib/auth-session";

const containerQuickStartGuideUrl =
	"https://github.com/ugoite/ugoite/blob/main/docs/guide/container-quickstart.md";
const localDevAuthGuideUrl =
	"https://github.com/ugoite/ugoite/blob/main/docs/guide/local-dev-auth-login.md";

const toMessage = (error: unknown): string => {
	if (error instanceof Error && error.message.trim()) {
		return error.message;
	}
	return "Login failed.";
};

export default function LoginRoute() {
	const navigate = useNavigate();
	const [searchParams] = useSearchParams();
	const [username, setUsername] = createSignal("");
	const [totpCode, setTotpCode] = createSignal("");
	const [authConfigError, setAuthConfigError] = createSignal("");
	const [submitError, setSubmitError] = createSignal("");
	const [isSubmitting, setIsSubmitting] = createSignal(false);
	const redirectTarget = () => searchParams.next || "/spaces";

	const [authConfig] = createResource(async () => {
		setAuthConfigError("");
		try {
			const config = await authApi.getConfig();
			setUsername(config.usernameHint);
			return config;
		} catch (error) {
			setAuthConfigError(toMessage(error));
			return null;
		}
	});

	const completeLogin = async (action: () => Promise<AuthLoginResponse>) => {
		setIsSubmitting(true);
		setSubmitError("");
		try {
			const response = await action();
			setAuthTokenCookie(response.bearerToken, response.expiresAt);
			navigate(redirectTarget(), { replace: true });
		} catch (error) {
			setSubmitError(toMessage(error));
		} finally {
			setIsSubmitting(false);
		}
	};

	const handleSubmit = async (event: Event) => {
		event.preventDefault();
		await completeLogin(() => authApi.loginWithPasskeyTotp(username().trim(), totpCode().trim()));
	};

	const handleMockOAuth = async () => {
		await completeLogin(() => authApi.loginWithMockOauth());
	};

	return (
		<main class="ui-page mx-auto max-w-2xl ui-stack">
			<section class="ui-card ui-stack">
				<div>
					<h1 class="ui-page-title">Login</h1>
					<p class="ui-page-subtitle mt-2">
						Use the same explicit passwordless login flow that browser and CLI sessions share.
					</p>
				</div>

				<Show when={authConfig.loading}>
					<p class="text-sm ui-muted">Loading auth mode...</p>
				</Show>

				<Show when={authConfigError()}>
					<div class="ui-alert ui-alert-error ui-stack-sm text-sm">
						<p>
							Failed to load the current auth mode. Confirm the Ugoite stack you started is still
							running, then refresh this page.
						</p>
						<p>
							Started from source? Re-run <code>mise run dev</code>. Using the published stack?
							Restart your Docker Compose services and reopen <code>/login</code>.
						</p>
						<p>
							Need the browser setup guide? Open{" "}
							<a
								href={containerQuickStartGuideUrl}
								target="_blank"
								rel="noopener"
								class="hover:underline"
							>
								Container Quick Start
							</a>{" "}
							for the published stack or{" "}
							<a href={localDevAuthGuideUrl} target="_blank" rel="noopener" class="hover:underline">
								Local Dev Auth/Login
							</a>{" "}
							for source development.
						</p>
						<p class="ui-muted">Details: {authConfigError()}</p>
					</div>
				</Show>

				<Show when={authConfig()}>
					{(config) => (
						<>
							<Show when={config().mode === "passkey-totp"}>
								<form class="ui-stack-sm" onSubmit={handleSubmit}>
									<section class="ui-card ui-stack-sm" aria-labelledby="login-first-time-heading">
										<div class="ui-stack-sm">
											<h2 id="login-first-time-heading" class="text-base font-semibold">
												First time here?
											</h2>
											<ol class="list-decimal space-y-1 pl-5 text-sm ui-muted">
												<li>
													Start the local stack with <code>mise run dev</code>.
												</li>
												<li>Reuse the username you confirmed in that terminal.</li>
												<li>
													Generate the current 2FA code from your authenticator or the guide&apos;s{" "}
													<code>oathtool</code> example.
												</li>
											</ol>
										</div>
										<p class="text-sm ui-muted">
											Need the full walkthrough? Open{" "}
											<a
												href={localDevAuthGuideUrl}
												target="_blank"
												rel="noopener"
												class="hover:underline"
											>
												Local Dev Auth/Login
											</a>
											.
										</p>
									</section>
									<p class="text-sm ui-muted">
										Use the username you confirmed in the terminal and the current 2FA code from
										your local development authenticator. This browser must also be using the
										passkey-bound local context prepared during startup.
									</p>
									<label class="ui-stack-sm">
										<span class="text-sm font-medium">Username</span>
										<input
											class="ui-input"
											type="text"
											value={username()}
											onInput={(event) => setUsername(event.currentTarget.value)}
											placeholder="dev-local-user"
										/>
									</label>
									<label class="ui-stack-sm">
										<span class="text-sm font-medium">2FA code</span>
										<input
											class="ui-input"
											type="text"
											inputMode="numeric"
											autocomplete="one-time-code"
											value={totpCode()}
											onInput={(event) => setTotpCode(event.currentTarget.value)}
											placeholder="123456"
											maxLength={6}
										/>
									</label>
									<button
										type="submit"
										class="ui-button ui-button-primary"
										disabled={
											!username().trim() || totpCode().trim().length !== 6 || isSubmitting()
										}
									>
										{isSubmitting() ? "Signing in..." : "Sign in with passkey + 2FA"}
									</button>
								</form>
							</Show>

							<Show when={config().mode === "mock-oauth"}>
								<div class="ui-stack-sm">
									<p class="text-sm ui-muted">
										Use the explicit local demo login path to exercise the browser login flow
										without an external OAuth provider or startup auth bypass.
									</p>
									<button
										type="button"
										class="ui-button ui-button-primary"
										onClick={() => void handleMockOAuth()}
										disabled={isSubmitting()}
									>
										{isSubmitting() ? "Redirecting..." : "Continue with Local Demo Login"}
									</button>
								</div>
							</Show>
						</>
					)}
				</Show>

				<Show when={submitError()}>
					<p class="ui-alert ui-alert-error text-sm">{submitError()}</p>
				</Show>

				<div class="flex flex-wrap gap-3">
					<A href="/" class="ui-button ui-button-secondary">
						Back to Home
					</A>
				</div>
			</section>
		</main>
	);
}
