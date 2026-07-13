import { useNavigate } from "@solidjs/router";
import { createSignal, onMount, Show } from "solid-js";
import { authApi } from "~/lib/auth-api";

const message = (error: unknown) =>
  error instanceof Error ? error.message : "Setup failed.";

export default function SetupRoute() {
  const navigate = useNavigate();
  const initialSecret = typeof location === "undefined"
    ? ""
    : new URLSearchParams(location.hash.slice(1)).get("secret") ?? "";
  const [secret, setSecret] = createSignal(initialSecret);
  const [displayName, setDisplayName] = createSignal("");
  const [recoveryCodes, setRecoveryCodes] = createSignal<string[]>([]);
  const [hasInitialPasskey, setHasInitialPasskey] = createSignal(false);
  const [accountId, setAccountId] = createSignal("");
  const [strengthComplete, setStrengthComplete] = createSignal(false);
  const [totp, setTotp] = createSignal<
    { secret: string; otpauth_uri: string }
  >();
  const [totpCode, setTotpCode] = createSignal("");
  const [error, setError] = createSignal("");
  const [busy, setBusy] = createSignal(false);

  onMount(async () => {
    const config = await authApi.getConfig().catch(() => undefined);
    if (config?.status === "active") navigate("/login", { replace: true });
    const session = await authApi.getSession().catch(() => undefined);
    if (session?.authenticated) {
      setHasInitialPasskey(true);
      setAccountId(session.account?.account_id ?? "");
    }
  });

  const setup = async (event: Event) => {
    event.preventDefault();
    setBusy(true);
    setError("");
    try {
      const result = await authApi.setup(secret(), displayName());
      history.replaceState(null, "", location.pathname);
      setRecoveryCodes(result.recovery_codes);
      setAccountId(result.account.account_id);
      setHasInitialPasskey(true);
    } catch (cause) {
      setError(message(cause));
    } finally {
      setBusy(false);
    }
  };

  return (
    <main class="publicShell">
      <section class="publicCard ui-stack">
        <h1 class="ui-page-title">Initialize this Ugoite node</h1>
        <Show
          when={!hasInitialPasskey()}
          fallback={
            <div class="ui-stack-sm">
              <Show
                when={recoveryCodes().length > 0}
                fallback={
                  <p>
                    Resume setup by adding a second Passkey or confirming TOTP.
                  </p>
                }
              >
                <p>
                  Save these one-time recovery codes now. They are not shown
                  again.
                </p>
                <p>
                  Recovery account ID: <code>{accountId()}</code>
                </p>
                <pre class="ui-card">{recoveryCodes().join("\n")}</pre>
              </Show>
              <Show
                when={strengthComplete()}
                fallback={
                  <div class="ui-stack-sm">
                    <p>
                      Complete setup with a second Passkey, or confirm recovery
                      TOTP while retaining these codes.
                    </p>
                    <button
                      type="button"
                      class="ui-button ui-button-primary"
                      onClick={async () => {
                        await authApi.addPasskey();
                        setStrengthComplete(true);
                      }}
                    >
                      Register second Passkey
                    </button>
                    <button
                      type="button"
                      class="ui-button ui-button-secondary"
                      onClick={async () =>
                        setTotp(await authApi.startTotpEnrollment())}
                    >
                      Configure TOTP instead
                    </button>
                    <Show when={totp()}>
                      {(enrollment) => (
                        <div class="ui-stack-sm">
                          <code class="ui-card break-all">
                            {enrollment().secret}
                          </code>
                          <label class="ui-stack-sm">
                            <span>Current six-digit TOTP</span>
                            <input
                              class="ui-input"
                              inputmode="numeric"
                              autocomplete="one-time-code"
                              value={totpCode()}
                              onInput={(event) =>
                                setTotpCode(event.currentTarget.value)}
                            />
                          </label>
                          <button
                            type="button"
                            class="ui-button ui-button-primary"
                            onClick={async () => {
                              await authApi.finishTotpEnrollment(totpCode());
                              setStrengthComplete(true);
                            }}
                          >
                            Confirm TOTP
                          </button>
                        </div>
                      )}
                    </Show>
                  </div>
                }
              >
                <button
                  type="button"
                  class="ui-button ui-button-primary"
                  onClick={() => navigate("/spaces", { replace: true })}
                >
                  Continue
                </button>
              </Show>
            </div>
          }
        >
          <form class="ui-stack-sm" onSubmit={setup}>
            <label class="ui-stack-sm">
              <span>Setup secret</span>
              <input
                class="ui-input"
                value={secret()}
                onInput={(event) => setSecret(event.currentTarget.value)}
                required
              />
            </label>
            <label class="ui-stack-sm">
              <span>Display name</span>
              <input
                class="ui-input"
                value={displayName()}
                onInput={(event) => setDisplayName(event.currentTarget.value)}
                required
              />
            </label>
            <button
              type="submit"
              class="ui-button ui-button-primary"
              disabled={busy()}
            >
              {busy() ? "Creating passkey…" : "Create administrator passkey"}
            </button>
          </form>
        </Show>
        <Show when={error()}>
          <p class="ui-alert ui-alert-error">{error()}</p>
        </Show>
      </section>
    </main>
  );
}
