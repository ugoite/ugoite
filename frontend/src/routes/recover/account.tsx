import { useNavigate, useSearchParams } from "@solidjs/router";
import { createSignal, For, Show } from "solid-js";
import { authApi } from "~/lib/auth-api";
import { getSafeNextPath } from "~/lib/auth-route";

const message = (error: unknown) =>
  error instanceof Error ? error.message : "Authentication failed.";

export default function AccountRecoveryRoute() {
  const navigate = useNavigate();
  const [params] = useSearchParams();
  const [accountId, setAccountId] = createSignal("");
  const [recoveryCode, setRecoveryCode] = createSignal("");
  const [totpCode, setTotpCode] = createSignal("");
  const [recoveryCodes, setRecoveryCodes] = createSignal<string[]>([]);
  const [error, setError] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const nextPath = () => getSafeNextPath(params.next);

  const submit = async (event: Event) => {
    event.preventDefault();
    setBusy(true);
    setError("");
    try {
      const result = await authApi.recoverPasskey(
        accountId().trim(),
        recoveryCode().trim(),
        totpCode().trim(),
      );
      setRecoveryCodes(result.recovery_codes);
    } catch (cause) {
      setError(message(cause));
    } finally {
      setBusy(false);
    }
  };

  return (
    <main class="publicShell">
      <section class="publicCard ui-stack">
        <h1 class="ui-page-title">Recover your account</h1>
        <Show
          when={recoveryCodes().length === 0}
          fallback={
            <section class="ui-stack-sm" aria-label="New recovery codes">
              <h2>Save your new recovery codes</h2>
              <p class="ui-muted">
                These codes are shown only once. Store them offline before
                continuing.
              </p>
              <ul class="font-mono">
                <For each={recoveryCodes()}>{(code) => <li>{code}</li>}</For>
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
          }
        >
          <p class="ui-muted">
            Enter your Account ID, one recovery code, and your recovery
            authenticator code to register a new Passkey.
          </p>
          <form class="ui-stack-sm" onSubmit={submit}>
            <label class="ui-stack-sm">
              <span>Account ID</span>
              <input
                class="ui-input font-mono"
                value={accountId()}
                onInput={(event) => setAccountId(event.currentTarget.value)}
                required
              />
            </label>
            <label class="ui-stack-sm">
              <span>Recovery Code</span>
              <input
                class="ui-input font-mono"
                value={recoveryCode()}
                onInput={(event) => setRecoveryCode(event.currentTarget.value)}
                required
              />
            </label>
            <label class="ui-stack-sm">
              <span>Authenticator code</span>
              <input
                class="ui-input font-mono"
                inputmode="numeric"
                autocomplete="one-time-code"
                pattern="[0-9]{6}"
                value={totpCode()}
                onInput={(event) => setTotpCode(event.currentTarget.value)}
                required
              />
            </label>
            <button
              type="submit"
              class="ui-button ui-button-primary"
              disabled={busy()}
            >
              {busy() ? "Waiting for new Passkey…" : "Register new Passkey"}
            </button>
          </form>
        </Show>
        <Show when={error()}>
          <p class="ui-alert ui-alert-error" role="alert">{error()}</p>
        </Show>
      </section>
    </main>
  );
}
