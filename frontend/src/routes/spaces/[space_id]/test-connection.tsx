import { A, useParams } from "@solidjs/router";
import { createSignal, Show } from "solid-js";
import { spaceApi } from "~/lib/ugoite-client";
import type { StorageConnectionConfig } from "~/lib/types";
import { createResource } from "~/lib/recoverable-resource";
import { spaceRoute } from "~/lib/space-shell-route";

export const route = spaceRoute({ navigation: "settings", title: "settingsStorage" });

export default function SpaceTestConnectionRoute() {
  const params = useParams<{ space_id: string }>();
  const spaceId = () => params.space_id;
  const [uri, setUri] = createSignal("");
  const [endpoint, setEndpoint] = createSignal("");
  const [status, setStatus] = createSignal<string | null>(null);
  const [error, setError] = createSignal<string | null>(null);
  const [isTesting, setIsTesting] = createSignal(false);

  const [space] = createResource(async () => {
    const ws = await spaceApi.get(spaceId());
    setUri(ws.storage_config?.uri || "");
    setEndpoint(ws.storage_config?.endpoint || "");
    return ws;
  });

  const buildStorageConfig = (): StorageConnectionConfig => {
    const trimmedUri = uri().trim();
    const storage_config: StorageConnectionConfig = { uri: trimmedUri };
    const trimmedEndpoint = endpoint().trim();
    if (trimmedUri.toLowerCase().startsWith("s3://") && trimmedEndpoint) {
      storage_config.endpoint = trimmedEndpoint;
    }
    return storage_config;
  };

  const handleTest = async () => {
    setError(null);
    setStatus(null);
    setIsTesting(true);
    try {
      const result = await spaceApi.testConnection(spaceId(), {
        storage_config: buildStorageConfig(),
      });
      setStatus(result.status);
    } catch (err) {
      setError(
        err instanceof Error ? err.message : "Failed to test connection",
      );
    } finally {
      setIsTesting(false);
    }
  };

  return (
    <>
      <div class="screenHead">
        <div class="screenTitle">
          <div class="eyebrow">Settings / Storage</div>
          <h1>Test Connection</h1>
        </div>
        <A href={`/spaces/${spaceId()}/settings?section=storage`} class="btn">
          Back to Settings
        </A>
      </div>

      <Show when={space.loading}>
        <p class="text-sm ui-muted">Loading space...</p>
      </Show>
      <Show when={space.error}>
        <p class="ui-alert ui-alert-error text-sm">Failed to load space.</p>
      </Show>

      <div class="settingsMain surface">
        <label for="storage-uri">
          Storage URI
          <input
            id="storage-uri"
            type="text"
            value={uri()}
            onInput={(event) => setUri(event.currentTarget.value)}
            placeholder="file:///local/path or s3://bucket/path"
          />
        </label>
        <label for="storage-endpoint">
          Storage Endpoint (optional)
          <input
            id="storage-endpoint"
            type="url"
            value={endpoint()}
            onInput={(event) => setEndpoint(event.currentTarget.value)}
            placeholder="https://s3.example.com"
          />
        </label>
        <p class="ui-muted">
          Use this for remote storage services that need an explicit HTTP or
          HTTPS endpoint.
        </p>
        <button
          type="button"
          class="btn primary"
          onClick={handleTest}
          disabled={isTesting() || !uri().trim()}
        >
          {isTesting() ? "Testing..." : "Test Connection"}
        </button>
        <Show when={status()}>
          <p class="ui-alert ui-alert-success">
            Connection successful ({status()})
          </p>
        </Show>
        <Show when={error()}>
          <p class="ui-alert ui-alert-error">{error()}</p>
        </Show>
      </div>
    </>
  );
}
