import { useNavigate, useSearchParams } from "@solidjs/router";
import { createSignal, For, Show } from "solid-js";
import { authApi } from "~/lib/auth-api";
import { getSafeNextPath } from "~/lib/auth-route";

export default function RecoverRoute() {
  const navigate = useNavigate();
  const [params] = useSearchParams();
  const [token, setToken] = createSignal(
    params.owner_approval_token ?? params.token ?? "",
  );
  const [error, setError] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const [result, setResult] = createSignal<
    Awaited<
      ReturnType<typeof authApi.recoverSpaceAccess>
    > | null
  >(null);
  const nextPath = () => getSafeNextPath(params.next);

  const submit = async (event: Event) => {
    event.preventDefault();
    setBusy(true);
    setError("");
    try {
      setResult(await authApi.recoverSpaceAccess(token().trim()));
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Recovery failed.");
    } finally {
      setBusy(false);
    }
  };

  return (
    <main class="publicShell">
      <section class="publicCard ui-stack">
        <h1 class="ui-page-title">Recover Space access</h1>
        <Show when={!result()}>
          <p class="ui-muted">
            Paste the one-time recovery token provided by the Space Owner. You
            will register a new Passkey for this Space Principal.
          </p>
          <form class="ui-stack-sm" onSubmit={submit}>
            <label class="ui-stack-sm">
              <span>Owner recovery token</span>
              <input
                class="ui-input font-mono"
                value={token()}
                onInput={(event) => setToken(event.currentTarget.value)}
                required
              />
            </label>
            <button
              type="submit"
              class="ui-button ui-button-primary"
              disabled={busy()}
            >
              {busy() ? "Preparing Passkey registration…" : "Continue"}
            </button>
          </form>
        </Show>
        <Show when={result()}>
          {(completed) => (
            <section class="ui-stack-sm" aria-label="New recovery codes">
              <h2>Save your new recovery codes</h2>
              <p class="ui-muted">
                These codes belong to the newly created HumanAccount and are
                shown only once. Audit delivery is {completed().audit_status}.
              </p>
              <ul class="font-mono">
                <For each={completed().recovery_codes}>
                  {(code) => <li>{code}</li>}
                </For>
              </ul>
              <button
                type="button"
                class="ui-button ui-button-primary"
                onClick={() =>
                  navigate(nextPath(), { replace: true })}
              >
                I saved the codes
              </button>
            </section>
          )}
        </Show>
        <Show when={error()}>
          <p class="ui-alert ui-alert-error" role="alert">{error()}</p>
        </Show>
      </section>
    </main>
  );
}
