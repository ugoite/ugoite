import { useNavigate } from "@solidjs/router";
import { createSignal, For, Show } from "solid-js";
import { authApi } from "~/lib/auth-api";

export default function RecoverRoute() {
  const navigate = useNavigate();
  const [accountId, setAccountId] = createSignal("");
  const [recoveryCode, setRecoveryCode] = createSignal("");
  const [totpCode, setTotpCode] = createSignal("");
  const [error, setError] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const [replacementCodes, setReplacementCodes] = createSignal<string[]>([]);

  const submit = async (event: Event) => {
    event.preventDefault();
    setBusy(true);
    setError("");
    try {
      const result = await authApi.recoverPasskey(
        accountId(),
        recoveryCode(),
        totpCode(),
      );
      setReplacementCodes(result.recovery_codes);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Recovery failed.");
    } finally {
      setBusy(false);
    }
  };

  return (
    <main class="ui-page mx-auto max-w-xl ui-stack">
      <section class="ui-card ui-stack">
        <h1 class="ui-page-title">Recover a Passkey</h1>
        <p class="ui-muted">
          Recovery requires both an unused recovery code and your recovery TOTP.
          TOTP alone is not accepted.
        </p>
        <Show when={replacementCodes().length === 0}>
          <form class="ui-stack-sm" onSubmit={submit}>
          <label class="ui-stack-sm">
            <span>Account ID</span>
            <input
              class="ui-input"
              value={accountId()}
              onInput={(event) => setAccountId(event.currentTarget.value)}
              required
            />
          </label>
          <label class="ui-stack-sm">
            <span>Recovery code</span>
            <input
              class="ui-input font-mono"
              value={recoveryCode()}
              onInput={(event) => setRecoveryCode(event.currentTarget.value)}
              required
            />
          </label>
          <label class="ui-stack-sm">
            <span>TOTP code</span>
            <input
              class="ui-input"
              inputmode="numeric"
              autocomplete="one-time-code"
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
            {busy() ? "Verifying…" : "Register replacement Passkey"}
          </button>
          </form>
        </Show>
        <Show when={replacementCodes().length > 0}>
          <section class="ui-stack-sm" aria-label="Replacement recovery codes">
            <h2>Save your replacement recovery codes</h2>
            <p class="ui-muted">
              The code used for recovery has been replaced. These values are shown only once.
            </p>
            <ul class="font-mono">
              <For each={replacementCodes()}>{(code) => <li>{code}</li>}</For>
            </ul>
            <button
              type="button"
              class="ui-button ui-button-primary"
              onClick={() => navigate("/spaces", { replace: true })}
            >
              I saved the codes
            </button>
          </section>
        </Show>
        <Show when={error()}>
          <p class="ui-alert ui-alert-error">{error()}</p>
        </Show>
      </section>
    </main>
  );
}
