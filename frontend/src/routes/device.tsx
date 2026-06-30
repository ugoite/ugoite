import { createSignal, For, onMount, Show } from "solid-js";
import { useSearchParams } from "@solidjs/router";
import { spaceApi } from "~/lib/ugoite-client";

export default function DeviceApprovalRoute() {
  const [params] = useSearchParams();
  const [code, setCode] = createSignal(params.user_code ?? "");
  const [spaceId, setSpaceId] = createSignal("");
  const [spaces, setSpaces] = createSignal<Array<{ id: string; name: string }>>(
    [],
  );
  const [done, setDone] = createSignal(false);
  const [error, setError] = createSignal("");
  onMount(async () => {
    try {
      const values = await spaceApi.list();
      setSpaces(values);
      setSpaceId(values[0]?.id ?? "");
    } catch {
      location.href = `/login?next=${
        encodeURIComponent(location.pathname + location.search)
      }`;
    }
  });
  const approve = async (event: Event) => {
    event.preventDefault();
    setError("");
    const pendingResponse = await fetch(
      `/api/oauth/device/pending?user_code=${encodeURIComponent(code())}`,
    );
    const pending = await pendingResponse.json().catch(() => ({}));
    if (!pendingResponse.ok) {
      setError(String(pending.message ?? pending.detail ?? "Code is invalid"));
      return;
    }
    if (
      !confirm(
        `Approve ${String(pending.device_name)} for actions: ${
          (pending.requested_actions ?? []).join(", ")
        }?`,
      )
    ) return;
    const response = await fetch("/api/oauth/device/approve", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        user_code: code(),
        space_id: spaceId(),
        granted_actions: [],
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
    <main class="ui-page mx-auto max-w-xl ui-stack">
      <section class="ui-card ui-stack">
        <h1 class="ui-page-title">Approve a device</h1>
        <Show
          when={!done()}
          fallback={
            <p class="ui-alert">
              Device approved. Return to the CLI or MCP client.
            </p>
          }
        >
          <form class="ui-stack-sm" onSubmit={approve}>
            <label class="ui-stack-sm">
              <span>Code shown by the device</span>
              <input
                class="ui-input"
                value={code()}
                onInput={(event) =>
                  setCode(event.currentTarget.value.toUpperCase())}
                required
              />
            </label>
            <label class="ui-stack-sm">
              <span>Space</span>
              <select
                class="ui-input"
                value={spaceId()}
                onChange={(event) => setSpaceId(event.currentTarget.value)}
              >
                <For each={spaces()}>
                  {(space) => <option value={space.id}>{space.name}</option>}
                </For>
              </select>
            </label>
            <p class="ui-muted">
              The next confirmation shows the device name and exact requested
              actions. Verify both before approving.
            </p>
            <button type="submit" class="ui-button ui-button-primary">
              Approve device
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
