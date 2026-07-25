import { A, useNavigate } from "@solidjs/router";
import { createSignal, For, onMount, Show } from "solid-js";
import { authApi, type OidcProvider } from "~/lib/auth-api";
import { GlobalShell } from "~/components/GlobalShell";
import { createResource } from "~/lib/recoverable-resource";

export default function SpaceInvitationJoinRoute() {
  const navigate = useNavigate();
  const hashToken = typeof location === "undefined"
    ? ""
    : new URLSearchParams(location.hash.slice(1)).get("token") ?? "";
  const [token, setToken] = createSignal(hashToken);
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal("");
  const [oidcProviders, setOidcProviders] = createSignal<OidcProvider[]>([]);
  const [session] = createResource(async () =>
    await authApi.getSession().catch(() => ({ authenticated: false }))
  );
  onMount(async () => {
    setOidcProviders(await authApi.listOidcProviders().catch(() => []));
  });
  const submit = async (event: Event) => {
    event.preventDefault();
    setBusy(true);
    setError("");
    try {
      const session = await authApi.getSession();
      if (session.authenticated) {
        await authApi.acceptInvitation(token().trim());
      } else {
        await authApi.registerInvitation(token().trim());
      }
      history.replaceState(null, "", location.pathname);
      navigate("/spaces", { replace: true });
    } catch (cause) {
      setError(
        cause instanceof Error
          ? cause.message
          : "Invitation registration failed.",
      );
    } finally {
      setBusy(false);
    }
  };
  return (
    <GlobalShell
      title="Join a Space"
      active="spaces"
      authenticated={session()?.authenticated ?? false}
    >
      <div class="screenHead">
        <div class="screenTitle">
          <div class="eyebrow">Spaces</div>
          <h1>Join</h1>
        </div>
      </div>
      <section class="settingsMain surface">
        <h2>Join a Space</h2>
        <p class="ui-muted">
          Signed-in accounts can accept this one-use invitation directly. If you
          are not signed in, Ugoite registers a new Passkey first.
        </p>
        <form class="ui-stack-sm" onSubmit={submit}>
          <label>
            <span>Invitation token</span>
            <textarea
              class="mono"
              value={token()}
              onInput={(event) => setToken(event.currentTarget.value)}
              required
            />
          </label>
          <button
            type="submit"
            class="btn primary"
            disabled={busy()}
          >
            {busy() ? "Joining…" : "Accept invitation"}
          </button>
        </form>
        <For each={oidcProviders()}>
          {(provider) => (
            <button
              type="button"
              class="btn"
              disabled={!token().trim()}
              onClick={() =>
                authApi.loginWithOidc(provider.provider_id, token().trim())}
            >
              Join with {provider.issuer}
            </button>
          )}
        </For>
        <Show when={error()}>
          <p class="ui-alert ui-alert-error">{error()}</p>
        </Show>
        <A href="/login" class="btn">
          Already registered? Sign in
        </A>
      </section>
    </GlobalShell>
  );
}
