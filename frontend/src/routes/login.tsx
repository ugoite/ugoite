import { useNavigate, useSearchParams } from "@solidjs/router";
import { createSignal, For, onMount, Show } from "solid-js";
import {
  authApi,
  type AuthConfig,
  oidcIssuerLabel,
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
    <main class="loginShell">
      <section class="loginPanel">
        <a class="loginBrand" href="/" aria-label="Ugoite">
          <img class="brandMark" src="/brand/ugoite-mark.svg" alt="" />
          <strong>Ugoite</strong>
        </a>
        <div class="loginCopy">
          <h1>Sign in to your space</h1>
          <p>
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
            class="btn primary"
            autofocus
            ref={(element) => {
              // First-action autofocus: keyboard users land on the primary
              // sign-in action as soon as the config renders it.
              queueMicrotask(() => element.focus());
            }}
            disabled={busy()}
            onClick={() => void login()}
          >
            {busy() ? "Waiting for passkey…" : "Sign in with a passkey"}
          </button>
          <Show when={providers().length > 0}>
            <div class="or" aria-hidden="true">
              <span />
              <b>or</b>
              <span />
            </div>
            <For each={providers()}>
              {(provider) => (
                <button
                  type="button"
                  class="btn tonal"
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
          <a class="loginLink" href={`/recover/account${nextQuery()}`}>
            Lost your Passkey?
          </a>
        </Show>
        <Show when={error()}>
          <p class="ui-alert ui-alert-error text-sm" role="alert">{error()}</p>
        </Show>
      </section>
      <aside class="loginStatement" aria-label="Ugoite principles">
        <span class="statementMark" aria-hidden="true" />
        <p>Knowledge persists.</p>
        <p>Work may disappear.</p>
      </aside>
    </main>
  );
}
