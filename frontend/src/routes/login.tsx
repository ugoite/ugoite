import { useNavigate, useSearchParams } from "@solidjs/router";
import { createSignal, For, onMount, Show } from "solid-js";
import {
  authApi,
  type AuthConfig,
  oidcIssuerLabel,
  type OidcProvider,
} from "~/lib/auth-api";
import { clearPendingLoginPath, getSafeNextPath } from "~/lib/auth-route";
import { t } from "~/lib/i18n";

type LoginState =
  | { kind: "loading" }
  | { kind: "unauthenticated" }
  | { kind: "authenticating"; method: "passkey" | "oidc" }
  | {
    kind: "authentication-failed";
    operation: "configuration" | "passkey" | "oidc";
    detail: string;
    providerId?: string;
  };

const errorDetail = (error: unknown): string =>
  error instanceof Error ? error.message : String(error ?? "");

const isPasskeyCancellation = (error: unknown): boolean => {
  if (!error || typeof error !== "object") return false;
  return ("code" in error && error.code === "PASSKEY_CANCELLED") ||
    ("name" in error && error.name === "NotAllowedError");
};

export default function LoginRoute() {
  const navigate = useNavigate();
  const [params] = useSearchParams();
  const [config, setConfig] = createSignal<AuthConfig>();
  const [providers, setProviders] = createSignal<OidcProvider[]>([]);
  const [providerLoading, setProviderLoading] = createSignal(false);
  const [providerError, setProviderError] = createSignal("");
  const [providerErrorDetail, setProviderErrorDetail] = createSignal("");
  const [state, setState] = createSignal<LoginState>({ kind: "loading" });

  const nextPath = () => getSafeNextPath(params.next);
  const nextQuery = () =>
    params.next ? `?next=${encodeURIComponent(nextPath())}` : "";
  const isBusy = () => state().kind === "authenticating";
  const configurationFailed = () =>
    state().kind === "authentication-failed" &&
    state().operation === "configuration";
  const failureMessage = () =>
    configurationFailed()
      ? t("loginPage.optionsUnavailable")
      : t("loginPage.authenticationFailed");

  const loadProviders = async () => {
    setProviderError("");
    setProviderErrorDetail("");
    setProviderLoading(true);
    try {
      setProviders(await authApi.listOidcProviders());
    } catch (cause) {
      setProviderError(t("loginPage.providersUnavailable"));
      setProviderErrorDetail(errorDetail(cause));
    } finally {
      setProviderLoading(false);
    }
  };

  const loadAuthentication = async () => {
    setState({ kind: "loading" });
    setConfig(undefined);
    setProviders([]);
    setProviderError("");
    setProviderErrorDetail("");
    try {
      const current = await authApi.getConfig();
      setConfig(current);
      setState({ kind: "unauthenticated" });
      if (current.oidc) await loadProviders();
    } catch (cause) {
      setState({
        kind: "authentication-failed",
        operation: "configuration",
        detail: errorDetail(cause),
      });
    }
  };

  onMount(() => void loadAuthentication());

  const signInWithPasskey = async () => {
    if (isBusy()) return;
    if (configurationFailed()) {
      await loadAuthentication();
      return;
    }

    setProviderError("");
    setProviderErrorDetail("");
    setState({ kind: "authenticating", method: "passkey" });
    try {
      clearPendingLoginPath();
      await authApi.loginWithPasskey();
      navigate(
        config()?.status === "uninitialized"
          ? `/setup${nextQuery()}`
          : nextPath(),
        { replace: true },
      );
    } catch (cause) {
      if (isPasskeyCancellation(cause)) {
        setState({ kind: "unauthenticated" });
        return;
      }
      setState({
        kind: "authentication-failed",
        operation: "passkey",
        detail: errorDetail(cause),
      });
    }
  };

  const signInWithOidc = (provider: OidcProvider) => {
    if (isBusy()) return;
    setProviderError("");
    setProviderErrorDetail("");
    setState({ kind: "authenticating", method: "oidc" });
    try {
      authApi.loginWithOidc(
        provider.provider_id,
        undefined,
        nextPath(),
      );
    } catch (cause) {
      setState({
        kind: "authentication-failed",
        operation: "oidc",
        providerId: provider.provider_id,
        detail: errorDetail(cause),
      });
    }
  };

  return (
    <main class="loginShell">
      <section class="loginPanel">
        <h1 class="loginBrand">
          <img class="brandMark" src="/brand/ugoite-mark.svg" alt="" />
          <strong>Ugoite</strong>
        </h1>

        <Show
          when={state().kind === "loading" || isBusy() || providerLoading()}
        >
          <p class="loginStatus" role="status" aria-live="polite">
            {state().kind === "loading"
              ? t("loginPage.loading")
              : isBusy()
              ? t("loginPage.authenticating")
              : t("loginPage.loadingOptions")}
          </p>
        </Show>

        <Show when={state().kind === "authentication-failed"}>
          <div class="loginError">
            <p role="alert">{failureMessage()}</p>
            <details>
              <summary>{t("loginPage.details")}</summary>
              <pre>
                {state().kind === "authentication-failed"
                  ? state().detail
                  : ""}
              </pre>
            </details>
          </div>
        </Show>

        <Show when={providerError()}>
          <div class="loginError">
            <p role="alert">{providerError()}</p>
            <details>
              <summary>{t("loginPage.details")}</summary>
              <pre>{providerErrorDetail()}</pre>
            </details>
            <button
              type="button"
              class="btn tonal"
              aria-disabled={providerLoading() || isBusy() ? "true" : undefined}
              onClick={() => {
                if (!providerLoading() && !isBusy()) void loadProviders();
              }}
            >
              {t("loginPage.retry")}
            </button>
          </div>
        </Show>

        <Show when={config() || configurationFailed()}>
          <button
            type="button"
            class="btn primary"
            aria-disabled={isBusy() ? "true" : undefined}
            ref={(element) => {
              // Focus the first available authentication action for keyboard users.
              queueMicrotask(() => element.focus());
            }}
            onClick={() => void signInWithPasskey()}
          >
            {configurationFailed()
              ? t("loginPage.retry")
              : state().kind === "authentication-failed" &&
                  state().operation === "passkey"
              ? t("loginPage.tryAgain")
              : t("loginPage.signInWithPasskey")}
          </button>

          <Show when={providers().length > 0}>
            <For each={providers()}>
              {(provider) => (
                <button
                  type="button"
                  class="btn tonal"
                  aria-disabled={isBusy() ? "true" : undefined}
                  onClick={() => signInWithOidc(provider)}
                >
                  {state().kind === "authentication-failed" &&
                      state().operation === "oidc" &&
                      state().providerId === provider.provider_id
                    ? t("loginPage.tryAgain")
                    : t("loginPage.continueWithProvider", {
                      provider: oidcIssuerLabel(provider.issuer),
                    })}
                </button>
              )}
            </For>
          </Show>
        </Show>

        <a class="loginLink" href={`/recover/account${nextQuery()}`}>
          {t("loginPage.lostPasskey")}
        </a>
      </section>
    </main>
  );
}
