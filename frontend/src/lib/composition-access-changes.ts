const channelName = "ugoite:composition-access-changes";

type AccessChangeMessage = {
  kind: "space-access-changed";
  space_id: string;
};

const isAccessChangeMessage = (value: unknown): value is AccessChangeMessage =>
  Boolean(value) && typeof value === "object" &&
  (value as Record<string, unknown>).kind === "space-access-changed" &&
  typeof (value as Record<string, unknown>).space_id === "string" &&
  (value as Record<string, unknown>).space_id !== "";

/** Notify other same-origin tabs that authority for this Space may have changed. */
export function publishCompositionAccessChange(spaceId: string): void {
  if (
    !spaceId || typeof window === "undefined" ||
    typeof BroadcastChannel === "undefined"
  ) return;
  let channel: BroadcastChannel | undefined;
  try {
    channel = new BroadcastChannel(channelName);
    channel.postMessage({ kind: "space-access-changed", space_id: spaceId });
  } catch {
    // This signal is a freshness hint. Server authorization remains authoritative.
  } finally {
    channel?.close();
  }
}

/** Subscribe to same-browser Space authority changes in the current tab. */
export function subscribeToCompositionAccessChanges(
  onChange: (spaceId: string) => void,
): () => void {
  if (
    typeof window === "undefined" || typeof BroadcastChannel === "undefined"
  ) return () => {};
  let channel: BroadcastChannel;
  try {
    channel = new BroadcastChannel(channelName);
  } catch {
    return () => {};
  }
  channel.onmessage = (event: MessageEvent<unknown>) => {
    if (isAccessChangeMessage(event.data)) onChange(event.data.space_id);
  };
  return () => channel.close();
}
