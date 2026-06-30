import { A, useNavigate, useSearchParams } from "@solidjs/router";
import { createSignal, For, onMount, Show } from "solid-js";
import { authApi, type AuthConfig, type OidcProvider } from "~/lib/auth-api";

const message = (error: unknown) =>
  error instanceof Error ? error.message : "Authentication failed.";

export default function LoginRoute() {
  const navigate = useNavigate();
  const [params] = useSearchParams();
  const [config, setConfig] = createSignal<AuthConfig>();
  const [oidcProviders, setOidcProviders] = createSignal<OidcProvider[]>([]);
  const [error, setError] = createSignal("");
  const [busy, setBusy] = createSignal(false);

  onMount(async () => {
    try {
      const current = await authApi.getConfig();
      setConfig(current);
      if (current.oidc) {
        setOidcProviders(await authApi.listOidcProviders());
      }
    } catch (cause) {
      setError(message(cause));
    }
  });

  const login = async () => {
    setBusy(true);
    setError("");
    try {
      await authApi.loginWithPasskey();
      navigate(
        config()?.status === "uninitialized"
          ? "/setup"
          : params.next || "/spaces",
        { replace: true },
      );
    } catch (cause) {
      setError(message(cause));
    } finally {
      setBusy(false);
    }
  };

  return (
    <main class="ui-page mx-auto max-w-xl ui-stack">
      <section class="ui-card ui-stack">
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
          <For each={oidcProviders()}>
            {(provider) => (
              <button
                type="button"
                class="ui-button ui-button-secondary"
                onClick={() => authApi.loginWithOidc(provider.provider_id)}
              >
                Sign in with {provider.issuer}
              </button>
            )}
          </For>
        </Show>
        <Show when={error()}>
          <p class="ui-alert ui-alert-error text-sm">{error()}</p>
        </Show>
        <A href="/recover" class="ui-button ui-button-secondary">
          Recover a Passkey
        </A>
      </section>
    </main>
  );
}
