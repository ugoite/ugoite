import { useNavigate, useSearchParams } from "@solidjs/router";
import { createSignal, onCleanup, onMount, Show } from "solid-js";
import { authApi } from "~/lib/auth-api";
import { getSafeNextPath } from "~/lib/auth-route";

const message = (error: unknown) =>
  error instanceof Error ? error.message : "Setup failed.";

export default function SetupRoute() {
  const navigate = useNavigate();
  const [params] = useSearchParams();
  let cancelled = false;
  onCleanup(() => cancelled = true);
  const nextPath = () => getSafeNextPath(params.next);
  const nextQuery = () =>
    params.next ? `?next=${encodeURIComponent(nextPath())}` : "";
  const initialSecret = typeof location === "undefined"
    ? ""
    : new URLSearchParams(location.hash.slice(1)).get("secret") ?? "";
  const [secret, setSecret] = createSignal(initialSecret);
  const [displayName, setDisplayName] = createSignal("");
  const [recoveryCodes, setRecoveryCodes] = createSignal<string[]>([]);
  const [hasInitialPasskey, setHasInitialPasskey] = createSignal(false);
  const [accountId, setAccountId] = createSignal("");
  const [strengthComplete, setStrengthComplete] = createSignal(false);
  const [error, setError] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const [strengthening, setStrengthening] = createSignal(false);

  onMount(async () => {
    const config = await authApi.getConfig().catch(() => undefined);
    if (cancelled) return;
    if (config?.status === "active") {
      navigate(`/login${nextQuery()}`, { replace: true });
      return;
    }
    const session = await authApi.getSession().catch(() => undefined);
    if (!cancelled && session?.authenticated) {
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
      if (cancelled) return;
      history.replaceState(null, "", `${location.pathname}${nextQuery()}`);
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
                fallback={<p>Resume setup by registering a second Passkey.</p>}
              >
                <p>
                  Save these bootstrap-only recovery codes now. They are not
                  shown again. Account self-recovery becomes available after you
                  explicitly set up a recovery authenticator in Security
                  Settings.
                </p>
                <p>
                  Recovery account ID:{" "}
                  <code data-testid="recovery-account-id">{accountId()}</code>
                </p>
                <pre class="ui-card" data-testid="bootstrap-recovery-codes">
                  {recoveryCodes().join("\n")}
                </pre>
              </Show>
              <Show
                when={strengthComplete()}
                fallback={
                  <div class="ui-stack-sm">
                    <p>Complete setup by registering a second Passkey.</p>
                    <button
                      type="button"
                      class="ui-button ui-button-primary"
                      disabled={strengthening()}
                      onClick={async () => {
                        setStrengthening(true);
                        setError("");
                        try {
                          await authApi.addPasskey();
                          setStrengthComplete(true);
                        } catch (cause) {
                          setError(message(cause));
                        } finally {
                          setStrengthening(false);
                        }
                      }}
                    >
                      {strengthening()
                        ? "Waiting for passkey…"
                        : "Register second Passkey"}
                    </button>
                  </div>
                }
              >
                <button
                  type="button"
                  class="ui-button ui-button-primary"
                  onClick={() => navigate(nextPath(), { replace: true })}
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
