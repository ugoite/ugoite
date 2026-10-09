import { useNavigate, useSearchParams } from "@solidjs/router";
import { createSignal, For, Show } from "solid-js";
import { authApi } from "~/lib/auth-api";
import { getSafeNextPath } from "~/lib/auth-route";
import { t } from "~/lib/i18n";

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
    } catch {
      setError(t("ownerRecovery.failure"));
    } finally {
      setBusy(false);
    }
  };

  const auditStatus = () =>
    result()?.audit_status === "delivered"
      ? t("ownerRecovery.auditDelivered")
      : t("ownerRecovery.auditPending");

  return (
    <main class="publicShell">
      <section class="publicCard ui-stack">
        <h1 id="recovery-title" class="ui-page-title">
          {result()
            ? t("ownerRecovery.saveCodesTitle")
            : t("ownerRecovery.title")}
        </h1>
        <Show when={!result()}>
          <form class="ui-stack-sm" onSubmit={submit}>
            <label class="ui-stack-sm">
              <span>{t("ownerRecovery.tokenLabel")}</span>
              <input
                class="ui-input font-mono"
                type="password"
                autocomplete="off"
                spellcheck={false}
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
              {busy()
                ? t("ownerRecovery.continuing")
                : t("ownerRecovery.continue")}
            </button>
          </form>
        </Show>
        <Show when={result()}>
          {(completed) => (
            <section class="ui-stack-sm" aria-labelledby="recovery-title">
              <p class="ui-muted">{t("ownerRecovery.codesInstruction")}</p>
              <p class="ui-muted" role="status" aria-live="polite">
                {auditStatus()}
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
                {t("ownerRecovery.savedCodes")}
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
