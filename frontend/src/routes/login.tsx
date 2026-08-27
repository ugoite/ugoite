import { useNavigate, useSearchParams } from "@solidjs/router";
import { createSignal, For, onMount, Show } from "solid-js";
import {
  authApi,
  oidcIssuerLabel,
  type AuthConfig,
  type OidcProvider,
} from "~/lib/auth-api";
import { clearPendingLoginPath, getSafeNextPath } from "~/lib/auth-route";

const message = (error: unknown) =>
  error instanceof Error ? error.message : "Authentication failed.";

export default function LoginRoute() {
  const navigate = useNavigate();
  const [params] = useSearchParams();
  const [config, setConfig] = createSignal<AuthConfig>();
  const [providers, setProviders] = createSignal<OidcProvider[]>([]);
  const [error, setError] = createSignal("");
  const [busy, setBusy] = createSignal(false);

  const nextPath = () => getSafeNextPath(params.next);
  const nextQuery = () =>
    params.next ? `?next=${encodeURIComponent(nextPath())}` : "";

  onMount(async () => {
    try {
      const current = await authApi.getConfig();
      setConfig(current);
      if (current.oidc) {
        setProviders(await authApi.listOidcProviders());
      }
    } catch (cause) {
      setError(message(cause));
    }
  });

  const login = async () => {
    setBusy(true);
    setError("");
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
      setError(message(cause));
    } finally {
      setBusy(false);
    }
  };

  return (
    <main class="publicShell">
      <section class="publicCard ui-stack">
        <div>
          <h1 class="ui-page-title">Sign in to Ugoite</h1>
          <p class="ui-page-subtitle mt-2">
            Use a passkey registered with this Ugoite node.
          </p>
        </div>
        <Show
          when={config()}
          fallback={
            <p class="ui-muted">Loading authentication configuration…</p>
          }
        >
          <button
            type="button"
            class="ui-button ui-button-primary"
            disabled={busy()}
            onClick={() => void login()}
          >
            {busy() ? "Waiting for passkey…" : "Sign in with a passkey"}
          </button>
          <Show when={providers().length > 0}>
            <div class="ui-divider" aria-hidden="true">or</div>
            <For each={providers()}>
              {(provider) => (
                <button
                  type="button"
                  class="ui-button ui-button-secondary"
                  disabled={busy()}
                  onClick={() =>
                    authApi.loginWithOidc(
                      provider.provider_id,
                      undefined,
                      nextPath(),
                    )}
                >
                  Continue with {oidcIssuerLabel(provider.issuer)}
                </button>
              )}
            </For>
          </Show>
          <a class="ui-link text-sm" href={`/recover/account${nextQuery()}`}>
            Lost your Passkey?
          </a>
        </Show>
        <Show when={error()}>
          <p class="ui-alert ui-alert-error text-sm">{error()}</p>
        </Show>
      </section>
    </main>
  );
}
