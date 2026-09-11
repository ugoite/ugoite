import { A, useNavigate } from "@solidjs/router";
import { createSignal, For, Show } from "solid-js";
import { authApi, oidcIssuerLabel, type OidcProvider } from "~/lib/auth-api";
import { GlobalShell } from "~/components/GlobalShell";
import { createResource } from "~/lib/recoverable-resource";
import { t } from "~/lib/i18n";
import { formatUserFacingError } from "~/lib/user-facing-error";
import { UgoiteApiError } from "~/lib/ugoite-client/protocol";

type JoinFailure = {
  message: string;
  resume: string;
  showSpaces: boolean;
};

const failureFor = (cause: unknown): JoinFailure => {
  const code = cause instanceof UgoiteApiError ? cause.code : undefined;
  switch (code) {
    case "INVITATION_EXPIRED":
      return {
        message: formatUserFacingError(cause, "joinPage.failedAccept"),
        resume: t("joinPage.expiredResume"),
        showSpaces: false,
      };
    case "INVITATION_NOT_PENDING":
      return {
        message: formatUserFacingError(cause, "joinPage.failedAccept"),
        resume: t("joinPage.usedResume"),
        showSpaces: true,
      };
    case "INVITATION_NOT_FOUND":
      return {
        message: formatUserFacingError(cause, "joinPage.failedAccept"),
        resume: t("joinPage.invalidResume"),
        showSpaces: false,
      };
    default:
      return {
        message: formatUserFacingError(cause, "joinPage.failedAccept"),
        resume: t("joinPage.invalidResume"),
        showSpaces: false,
      };
  }
};

export default function SpaceInvitationJoinRoute() {
  const navigate = useNavigate();
  const hashToken = typeof location === "undefined"
    ? ""
    : new URLSearchParams(location.hash.slice(1)).get("token") ?? "";
  const [token, setToken] = createSignal(hashToken);
  const [busy, setBusy] = createSignal(false);
  const [failure, setFailure] = createSignal<JoinFailure | null>(null);
  const [providers] = createResource<OidcProvider[]>(async () =>
    await authApi.listOidcProviders().catch(() => [])
  );
  const [session] = createResource(async () =>
    await authApi.getSession().catch(() => ({ authenticated: false }))
  );
  const submit = async (event: Event) => {
    event.preventDefault();
    if (busy()) return;
    setBusy(true);
    setFailure(null);
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
      setFailure(failureFor(cause));
    } finally {
      setBusy(false);
    }
  };
  return (
    <GlobalShell
      title="Join a Space"
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
        <Show when={providers()?.length}>
          <div class="ui-divider" aria-hidden="true">or</div>
          <For each={providers()}>
            {(provider) => (
              <button
                type="button"
                class="btn"
                disabled={busy() || !token().trim()}
                onClick={() =>
                  authApi.loginWithOidc(provider.provider_id, token().trim())}
              >
                Continue with {oidcIssuerLabel(provider.issuer)}
              </button>
            )}
          </For>
        </Show>
        <Show when={failure()}>
          {(failed) => (
            <div class="ui-alert ui-alert-error" role="alert">
              <p>{failed().message}</p>
              <p class="ui-muted">{failed().resume}</p>
              <Show when={failed().showSpaces}>
                <A href="/spaces" class="btn">{t("joinPage.goToSpaces")}</A>
              </Show>
            </div>
          )}
        </Show>
        <A href="/login" class="btn">
          Already registered? Sign in
        </A>
      </section>
    </GlobalShell>
  );
}
