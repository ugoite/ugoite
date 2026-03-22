import { A, useNavigate, useSearchParams } from "@solidjs/router";
import { createResource, createSignal, onMount, Show } from "solid-js";
import { authApi } from "~/lib/auth-api";
import { clearAuthTokenCookie, setAuthTokenCookie } from "~/lib/auth-session";

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
	const [submitError, setSubmitError] = createSignal("");
	const [isSubmitting, setIsSubmitting] = createSignal(false);
	const redirectTarget = () => searchParams.next || "/spaces";

	const [authConfig] = createResource(async () => {
		const config = await authApi.getConfig();
		setUsername(config.usernameHint);
		return config;
	});

	onMount(() => {
		clearAuthTokenCookie();
	});

	const completeLogin = async (action: () => Promise<{ bearerToken: string }>) => {
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
		await completeLogin(async () => {
			return await authApi.loginWithTotp(username().trim(), totpCode().trim());
		});
	};

	const handleMockOAuth = async () => {
		await completeLogin(async () => {
			return await authApi.loginWithMockOauth();
		});
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

				<Show when={authConfig.error}>
					<p class="ui-alert ui-alert-error text-sm">
						Failed to load the current auth mode. Re-run <code>mise run dev</code> and try again.
					</p>
				</Show>

				<Show when={authConfig()}>
					{(config) => (
						<>
							<Show when={config().mode === "manual-totp"}>
								<form class="ui-stack-sm" onSubmit={handleSubmit}>
									<p class="text-sm ui-muted">
										Use the username you confirmed in the terminal and the current 2FA code from
										your local development authenticator.
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
										{isSubmitting() ? "Signing in..." : "Sign in with 2FA"}
									</button>
								</form>
							</Show>

							<Show when={config().mode === "mock-oauth"}>
								<div class="ui-stack-sm">
									<p class="text-sm ui-muted">
										Use the explicit mock OAuth path to exercise the browser login flow without
										bypassing authentication at startup.
									</p>
									<button
										type="button"
										class="ui-button ui-button-primary"
										onClick={() => void handleMockOAuth()}
										disabled={isSubmitting()}
									>
										{isSubmitting() ? "Redirecting..." : "Continue with Mock OAuth"}
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
					<A href="/spaces" class="ui-button ui-button-secondary">
						Go to Spaces
					</A>
				</div>
			</section>
		</main>
	);
}
