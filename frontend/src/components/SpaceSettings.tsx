import { createMemo, createSignal, Show } from "solid-js";
import { summarizeSpaceStorage } from "~/lib/storage-topology";
import type {
  Space,
  SpacePatchPayload,
  StorageConnectionConfig,
} from "~/lib/types";

export interface SpaceSettingsProps {
  space: Space;
  section?: "general" | "storage";
  onSave: (payload: SpacePatchPayload) => Promise<void>;
  onTestConnection?: (
    config: StorageConnectionConfig,
  ) => Promise<{ status: string }>;
}

export function SpaceSettings(props: SpaceSettingsProps) {
  const [name, setName] = createSignal(props.space.name);
  const [defaultForm, setDefaultForm] = createSignal(
    typeof props.space.settings?.default_form === "string"
      ? props.space.settings.default_form
      : "Entry",
  );
  const [uri, setUri] = createSignal(props.space.storage_config?.uri || "");
  const [endpoint, setEndpoint] = createSignal(
    props.space.storage_config?.endpoint || "",
  );
  const [pending, setPending] = createSignal(false);
  const [message, setMessage] = createSignal("");
  const section = () => props.section ?? "general";
  const storageSummary = createMemo(() => summarizeSpaceStorage(props.space));
  const config = (): StorageConnectionConfig => {
    const next: StorageConnectionConfig = {
      ...(props.space.storage_config ?? {}),
      uri: uri().trim(),
    };
    const storageUri = uri().trim().toLowerCase();
    const storageEndpoint = endpoint().trim();
    if (storageUri.startsWith("s3://") && storageEndpoint) {
      next.endpoint = storageEndpoint;
    } else {
      delete next.endpoint;
    }
    return next;
  };
  const save = async (event: Event) => {
    event.preventDefault();
    setPending(true);
    setMessage("");
    try {
      await props.onSave(
        section() === "general"
          ? {
            name: name(),
            settings: { default_form: defaultForm().trim() },
          }
          : { storage_config: config() },
      );
      setMessage("Saved");
    } catch (error) {
      setMessage(
        error instanceof Error ? error.message : "Failed to save settings",
      );
    } finally {
      setPending(false);
    }
  };
  const test = async () => {
    if (!props.onTestConnection) return;
    setPending(true);
    setMessage("");
    try {
      const result = await props.onTestConnection(config());
      setMessage(`Connection successful (${result.status})`);
    } catch (error) {
      setMessage(error instanceof Error ? error.message : "Connection failed");
    } finally {
      setPending(false);
    }
  };
  return (
    <form class="settingsMain surface" onSubmit={save}>
      <Show
        when={section() === "general"}
        fallback={
          <div class="settingsSection">
            <h2>Storage</h2>
            <section class="ui-card ui-stack-sm">
              <h3>Storage topology</h3>
              <p class="ui-muted">{storageSummary().description}</p>
              <span class="ui-pill">{storageSummary().label}</span>
              <Show when={storageSummary().uri}>
                <code>{storageSummary().uri}</code>
              </Show>
            </section>
            <p class="ui-alert ui-alert-warning">
              The saved URI below is migration metadata only. Updating it does
              not move existing data or change the backend's current storage
              root.
            </p>
            <div class="settingsGrid">
              <label>
                URI<input
                  id="storage-uri"
                  value={uri()}
                  onInput={(e) => setUri(e.currentTarget.value)}
                  placeholder="s3://ugoite-space/main"
                  required
                />
              </label>
              <label>
                Endpoint<input
                  id="storage-endpoint"
                  value={endpoint()}
                  onInput={(e) => setEndpoint(e.currentTarget.value)}
                  placeholder="https://s3.example.com"
                />
              </label>
              <label>
                Status<input value="configured" readOnly />
              </label>
            </div>
            <div class="actions">
              <button
                class="btn"
                type="button"
                onClick={() => void test()}
                disabled={pending() || !uri().trim()}
              >
                Test Connection
              </button>
              <button class="btn primary" type="submit" disabled={pending()}>
                Save
              </button>
            </div>
          </div>
        }
      >
        <div class="settingsSection">
          <h2>General</h2>
          <div class="settingsGrid">
            <label>
              Space Name<input
                id="space-name"
                value={name()}
                onInput={(e) => setName(e.currentTarget.value)}
                required
              />
            </label>
            <label>
              Default Form<input
                value={defaultForm()}
                onInput={(event) => setDefaultForm(event.currentTarget.value)}
                required
              />
            </label>
          </div>
          <button class="btn primary" type="submit" disabled={pending()}>
            Save
          </button>
        </div>
      </Show>
      <Show when={message()}>
        <p
          class="ui-alert"
          classList={{
            "ui-alert-success": message() === "Saved" ||
              message().startsWith("Connection successful"),
            "ui-alert-error": message() !== "Saved" &&
              !message().startsWith("Connection successful"),
          }}
        >
          {message()}
        </p>
      </Show>
    </form>
  );
}
