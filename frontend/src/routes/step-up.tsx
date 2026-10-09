import { createSignal, onMount, Show } from "solid-js";
import { useNavigate, useSearchParams } from "@solidjs/router";
import { authApi } from "~/lib/auth-api";
import { t } from "~/lib/i18n";
import { protocolFetch, UgoiteApiError } from "~/lib/ugoite-client/protocol";

type StepUpView =
  | "loading"
  | "missing"
  | "login-required"
  | "ready"
  | "approved"
  | "expired"
  | "used"
  | "forbidden";

const errorCode = (cause: unknown): string | undefined =>
  cause instanceof UgoiteApiError ? cause.code : undefined;

const failureMessage = (cause: unknown): string => {
  switch (errorCode(cause)) {
    case "STEP_UP_INVALID":
      return t("stepUpPage.unavailable");
    case "STEP_UP_NOT_APPROVED":
      return t("stepUpPage.notApproved");
    case "STEP_UP_ACCOUNT_INACTIVE":
      return t("stepUpPage.accountInactive");
    case "PASSKEY_CANCELLED":
      return t("securityPage.passkeyCancelled");
    default:
      return t("stepUpPage.approvalFailed");
  }
};

const viewForFailure = (cause: unknown): StepUpView => {
  switch (errorCode(cause)) {
    // Unknown, expired, consumed, and mismatched challenges fail closed
    // with one code (403 STEP_UP_INVALID, no 404 branch). All of them mean
    // the same thing here: start the CLI mutation again for a fresh request.
    case "STEP_UP_INVALID":
      return "used";
    case "STEP_UP_NOT_APPROVED":
    case "STEP_UP_ACCOUNT_INACTIVE":
      return "forbidden";
    default:
      return "forbidden";
  }
};

export default function StepUpApprovalRoute() {
  const [params] = useSearchParams();
  const navigate = useNavigate();
  const challengeId = () => String(params.challenge ?? "").trim();
  const [view, setView] = createSignal<StepUpView>("loading");
  const [error, setError] = createSignal("");
  const [busy, setBusy] = createSignal(false);

  onMount(async () => {
    if (!challengeId()) {
      setView("missing");
      return;
    }
    try {
      const session = await authApi.getSession();
      if (!session.authenticated) {
        setView("login-required");
        return;
      }
      const status = await protocolFetch<{ status: string }>(
        "auth.step_up.status",
        { challenge_id: challengeId() },
      );
      switch (status.status) {
        case "approved":
          setView("approved");
          break;
        case "pending":
          setView("ready");
          break;
        case "expired":
          setView("expired");
          break;
        default:
          setView("used");
          break;
      }
    } catch (cause) {
      setView(viewForFailure(cause));
      setError(failureMessage(cause));
    }
  });

  const signIn = () => {
    navigate(
      `/login?next=${
        encodeURIComponent(`/step-up?challenge=${challengeId()}`)
      }`,
      { replace: true },
    );
  };

  const approve = async (event: Event) => {
    event.preventDefault();
    if (!challengeId() || busy()) return;
    setBusy(true);
    setError("");
    try {
      // Approval requires a fresh phishing-resistant ceremony; the Passkey
      // prompt itself is the human-presence proof. The challenge is consumed
      // once by the CLI mutation afterwards, never here.
      await authApi.loginWithPasskey();
      await protocolFetch("auth.step_up.approve", {}, {
        challenge_id: challengeId(),
      });
      setView("approved");
    } catch (cause) {
      setView(viewForFailure(cause));
      setError(failureMessage(cause));
    } finally {
      setBusy(false);
    }
  };

  return (
    <main class="publicShell">
      <section class="publicCard ui-stack">
        <h1 class="ui-page-title">{t("stepUpPage.title")}</h1>
        <Show
          when={view() !== "missing"}
          fallback={<p class="ui-muted">{t("stepUpPage.missing")}</p>}
        >
          <Show
            when={view() !== "login-required"}
            fallback={
              <button
                type="button"
                class="ui-button ui-button-primary"
                onClick={signIn}
              >
                {t("stepUpPage.signIn")}
              </button>
            }
          >
            <Show
              when={view() !== "approved"}
              fallback={<p class="ui-alert">{t("stepUpPage.approved")}</p>}
            >
              <Show
                when={view() === "ready"}
                fallback={
                  <Show when={view() !== "loading"}>
                    <p class="ui-alert ui-alert-error" role="alert">
                      {view() === "expired" && t("stepUpPage.expired")}
                      {view() === "used" && t("stepUpPage.unavailable")}
                      {view() === "forbidden" &&
                        (error() || t("stepUpPage.approvalFailed"))}
                    </p>
                  </Show>
                }
              >
                <form class="ui-stack-sm" onSubmit={approve}>
                  <p class="ui-muted">{t("stepUpPage.reviewPrompt")}</p>
                  <button
                    type="submit"
                    class="ui-button ui-button-primary"
                    disabled={busy()}
                  >
                    {busy()
                      ? t("stepUpPage.approving")
                      : t("stepUpPage.approve")}
                  </button>
                </form>
              </Show>
            </Show>
          </Show>
        </Show>
        <Show when={view() === "loading"}>
          <p class="ui-muted" role="status">{t("stepUpPage.loading")}</p>
        </Show>
      </section>
    </main>
  );
}
