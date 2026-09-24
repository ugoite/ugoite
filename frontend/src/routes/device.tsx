import { createSignal, For, onMount, Show } from "solid-js";
import { useSearchParams } from "@solidjs/router";
import { ConfirmDestructiveAction } from "~/components/ConfirmDestructiveAction";
import { spaceApi } from "~/lib/ugoite-client";
import { formatUserFacingError } from "~/lib/user-facing-error";
import { spaceUid } from "~/lib/space-list";
import type { Space } from "~/lib/types";

type PendingMcpAuthorization = {
  device_name: string;
  requested_actions: string[];
  resource: string | null;
  requested_space_uid?: string | null;
};

export default function DeviceApprovalRoute() {
  const [params] = useSearchParams();
  const [code] = createSignal(params.user_code ?? "");
  const [spaceUidValue, setSpaceUidValue] = createSignal("");
  const [spaces, setSpaces] = createSignal<Space[]>([]);
  const [pending, setPending] = createSignal<PendingMcpAuthorization>();
  const [done, setDone] = createSignal(false);
  const [unsupported, setUnsupported] = createSignal(false);
  const [error, setError] = createSignal("");
  const [approveConfirmOpen, setApproveConfirmOpen] = createSignal(false);
  const [approving, setApproving] = createSignal(false);

  onMount(async () => {
    if (!code().trim()) {
      setUnsupported(true);
      return;
    }
    try {
      const response = await fetch(
        `/api/oauth/device/pending?user_code=${encodeURIComponent(code())}`,
      );
      const payload = await response.json().catch(() => ({}));
      if (!response.ok) {
        setError(
          String(payload.message ?? payload.detail ?? "Code is invalid"),
        );
        return;
      }
      if (
        payload.resource !== null &&
        payload.resource !== `${location.origin}/mcp`
      ) {
        setUnsupported(true);
        return;
      }
      const pendingRequest: PendingMcpAuthorization = {
        device_name: String(payload.device_name),
        requested_actions: Array.isArray(payload.requested_actions)
          ? payload.requested_actions.map(String)
          : [],
        resource: payload.resource === null ? null : String(payload.resource),
        requested_space_uid: payload.requested_space_uid == null
          ? null
          : String(payload.requested_space_uid),
      };
      setPending(pendingRequest);
      const values = await spaceApi.list();
      setSpaces(values);
      const requestedSpace = values.find((space) =>
        spaceUid(space) === pendingRequest.requested_space_uid
      );
      setSpaceUidValue(
        requestedSpace
          ? spaceUid(requestedSpace)
          : values[0]
          ? spaceUid(values[0])
          : "",
      );
    } catch (cause) {
      setError(
        formatUserFacingError(cause, "spacesPage.failedLoad", "space.list"),
      );
    }
  });

  const requestApproval = (event: Event) => {
    event.preventDefault();
    const request = pending();
    if (!request || !spaceUidValue() || approving()) return;
    setError("");
    setApproveConfirmOpen(true);
  };

  const closeApproveConfirm = () => {
    if (approving()) return;
    setApproveConfirmOpen(false);
  };

  const approve = async () => {
    const request = pending();
    if (!request || !spaceUidValue() || approving()) return;
    setError("");
    setApproving(true);
    try {
      const response = await fetch("/api/oauth/device/approve", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          user_code: code(),
          space_id: spaceUidValue(),
          granted_actions: request.requested_actions,
        }),
      });
      if (!response.ok) {
        const payload = await response.json().catch(() => ({}));
        setError(
          String(payload.message ?? payload.detail ?? "Approval failed"),
        );
        return;
      }
      setApproveConfirmOpen(false);
      setDone(true);
    } catch (cause) {
      setError(formatUserFacingError(cause, "errors.operation.settings"));
    } finally {
      setApproving(false);
    }
  };

  const approveSummary = () => {
    const request = pending();
    if (!request) return "";
    return `Approve ${request.device_name} for actions: ${
      request.requested_actions.join(
        ", ",
      )
    }?`;
  };

  return (
    <main class="publicShell">
      <section class="publicCard ui-stack">
        <Show
          when={!unsupported()}
          fallback={
            <>
              <h1 class="ui-page-title">
                Unsupported device authorization
              </h1>
              <p class="ui-muted">
                This approval request is for an unsupported resource. REST CLI
                requests omit the resource; MCP requests use this Node's
                <code>/mcp</code> resource.
              </p>
            </>
          }
        >
          <h1 class="ui-page-title">
            {done()
              ? `${pending()?.resource ? "MCP" : "CLI"} access approved`
              : pending()
              ? `Approve ${pending()?.resource ? "MCP" : "CLI"} access`
              : "Device authorization"}
          </h1>
          <Show
            when={!done()}
            fallback={
              <p class="ui-alert">
                {pending()?.resource
                  ? "Return to the MCP client."
                  : "Return to the CLI."}
              </p>
            }
          >
            <Show
              when={pending()}
              fallback={<p class="ui-muted">Loading authorization request…</p>}
            >
              {(request) => (
                <form class="ui-stack-sm" onSubmit={requestApproval}>
                  <p>
                    <strong>{request().device_name}</strong> requested{" "}
                    {request().resource ? "MCP" : "CLI"} actions:{" "}
                    {request().requested_actions.join(", ")}.
                  </p>
                  <label class="ui-stack-sm">
                    <span>Space</span>
                    <select
                      class="ui-input"
                      value={spaceUidValue()}
                      onChange={(event) =>
                        setSpaceUidValue(event.currentTarget.value)}
                    >
                      <For each={spaces()}>
                        {(space) => (
                          <option value={spaceUid(space)}>{space.name}</option>
                        )}
                      </For>
                    </select>
                  </label>
                  <p class="ui-muted">
                    Verify the client name, Space, and exact requested actions
                    before approving.
                  </p>
                  <button
                    type="submit"
                    class="ui-button ui-button-primary"
                    aria-label={`Review ${
                      request().resource ? "MCP" : "CLI"
                    } access request`}
                    disabled={!spaceUidValue()}
                  >
                    Review request
                  </button>
                  <ConfirmDestructiveAction
                    open={approveConfirmOpen()}
                    title={request().resource
                      ? "Approve MCP access?"
                      : "Approve CLI access?"}
                    body={approveSummary()}
                    confirmLabel={request().resource
                      ? "Approve MCP access"
                      : "Approve CLI access"}
                    busy={approving()}
                    error={error() || null}
                    onConfirm={() =>
                      void approve()}
                    onClose={closeApproveConfirm}
                  />
                </form>
              )}
            </Show>
          </Show>
        </Show>
        <Show when={error()}>
          <p class="ui-alert ui-alert-error" role="alert">{error()}</p>
        </Show>
      </section>
    </main>
  );
}
