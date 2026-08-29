import { createSignal, For, onMount, Show } from "solid-js";
import { useSearchParams } from "@solidjs/router";
import { spaceApi } from "~/lib/ugoite-client";
import { formatUserFacingError } from "~/lib/user-facing-error";

type PendingMcpAuthorization = {
  device_name: string;
  requested_actions: string[];
  resource: string | null;
  requested_space_uid?: string | null;
};

export default function DeviceApprovalRoute() {
  const [params] = useSearchParams();
  const [code] = createSignal(params.user_code ?? "");
  const [spaceId, setSpaceId] = createSignal("");
  const [spaces, setSpaces] = createSignal<Array<{ id: string; name: string }>>(
    [],
  );
  const [pending, setPending] = createSignal<PendingMcpAuthorization>();
  const [done, setDone] = createSignal(false);
  const [unsupported, setUnsupported] = createSignal(false);
  const [error, setError] = createSignal("");

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
      setSpaceId(
        values.find((space) =>
          space.space_uid === pendingRequest.requested_space_uid
        )?.id ?? values[0]?.id ?? "",
      );
    } catch (cause) {
      setError(
        formatUserFacingError(cause, "spacesPage.failedLoad", "space.list"),
      );
    }
  });

  const approve = async (event: Event) => {
    event.preventDefault();
    const request = pending();
    if (!request || !spaceId()) return;
    setError("");
    if (
      !confirm(
        `Approve ${request.device_name} for actions: ${
          request.requested_actions.join(
            ", ",
          )
        }?`,
      )
    ) return;
    const response = await fetch("/api/oauth/device/approve", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        user_code: code(),
        space_id: spaceId(),
        granted_actions: request.requested_actions,
      }),
    });
    if (!response.ok) {
      const payload = await response.json().catch(() => ({}));
      setError(String(payload.message ?? payload.detail ?? "Approval failed"));
      return;
    }
    setDone(true);
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
            {pending()?.resource ? "Approve MCP access" : "Approve CLI access"}
          </h1>
          <Show
            when={!done()}
            fallback={
              <p class="ui-alert">
                {pending()?.resource
                  ? "MCP access approved. Return to the MCP client."
                  : "CLI access approved. Return to the CLI."}
              </p>
            }
          >
            <Show
              when={pending()}
              fallback={<p class="ui-muted">Loading authorization request…</p>}
            >
              {(request) => (
                <form class="ui-stack-sm" onSubmit={approve}>
                  <p>
                    Approve <strong>{request().device_name}</strong> for{" "}
                    {request().resource ? "MCP" : "CLI"} actions:{" "}
                    {request().requested_actions.join(", ")}.
                  </p>
                  <label class="ui-stack-sm">
                    <span>Space</span>
                    <select
                      class="ui-input"
                      value={spaceId()}
                      onChange={(event) =>
                        setSpaceId(event.currentTarget.value)}
                    >
                      <For each={spaces()}>
                        {(space) => (
                          <option value={space.id}>{space.name}</option>
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
                    disabled={!spaceId()}
                  >
                    {request().resource
                      ? "Approve MCP access"
                      : "Approve CLI access"}
                  </button>
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
