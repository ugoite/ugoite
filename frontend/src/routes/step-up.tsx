import { createSignal, onMount, Show } from "solid-js";
import { useNavigate, useSearchParams } from "@solidjs/router";
import { authApi } from "~/lib/auth-api";
import {
  protocolFetch,
  UgoiteApiError,
} from "~/lib/ugoite-client/protocol";

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

const errorMessage = (cause: unknown): string =>
  cause instanceof Error && cause.message
    ? cause.message
    : "The step-up request could not be completed.";

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
      setError(errorMessage(cause));
    }
  });

  const signIn = () => {
    navigate(
      `/login?next=${encodeURIComponent(`/step-up?challenge=${challengeId()}`)}`,
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
      setError(errorMessage(cause));
    } finally {
      setBusy(false);
    }
  };

  return (
    <main class="publicShell">
      <section class="publicCard ui-stack">
        <h1 class="ui-page-title">Review CLI operation</h1>
        <Show
          when={view() !== "missing"}
          fallback={
            <p class="ui-muted">
              This step-up link is incomplete. Open the full verification link
              from the CLI prompt and try again.
            </p>
          }
        >
          <Show
            when={view() !== "login-required"}
            fallback={
              <>
                <p class="ui-muted">
                  Sign in with a passkey on this device, then return here to
                  approve the pending CLI mutation.
                </p>
                <button
                  type="button"
                  class="ui-button ui-button-primary"
                  onClick={signIn}
                >
                  Sign in with a passkey
                </button>
              </>
            }
          >
            <Show
              when={view() !== "approved"}
              fallback={
                <p class="ui-alert">
                  Step-up approved. Return to the CLI: it retries the identical
                  mutation once automatically.
                </p>
              }
            >
              <Show
                when={view() === "ready" || view() === "loading"}
                fallback={
                  <p class="ui-alert ui-alert-error" role="alert">
                    {view() === "expired" &&
                      "This step-up request has expired. Start the CLI mutation again to open a fresh request."}
                    {view() === "used" &&
                      "This step-up request was already used or is unknown. If you signed in with a different account, switch accounts and reopen the link; otherwise start the CLI mutation again."}
                    {view() === "forbidden" &&
                      (error() ||
                        "This step-up request belongs to a different account or is not approved yet. Sign in with the account that started the CLI mutation.")}
                  </p>
                }
              >
                <form class="ui-stack-sm" onSubmit={approve}>
                  <p>
                    A CLI mutation is waiting for a fresh passkey ceremony.
                    Approving binds one single-use approval to that exact
                    mutation; authorization is re-checked when the CLI retries.
                  </p>
                  <p class="ui-muted">
                    Verify the operation in the terminal that opened this link
                    before approving.
                  </p>
                  <button
                    type="submit"
                    class="ui-button ui-button-primary"
                    disabled={busy() || view() === "loading"}
                  >
                    {busy()
                      ? "Waiting for passkey…"
                      : "Approve with a passkey"}
                  </button>
                </form>
              </Show>
            </Show>
          </Show>
        </Show>
        <Show when={error() && (view() === "ready" || view() === "loading")}>
          <p class="ui-alert ui-alert-error" role="alert">{error()}</p>
        </Show>
      </section>
    </main>
  );
}
