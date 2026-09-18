import { createMemo, createSignal, Show } from "solid-js";
import { t } from "~/lib/i18n";
import { summarizeSpaceStorage } from "~/lib/storage-topology";
import type {
  Space,
  SpacePatchPayload,
  StorageConnectionConfig,
} from "~/lib/types";
import { formatUserFacingError } from "~/lib/user-facing-error";

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
  const [uri, setUri] = createSignal(props.space.storage_config?.uri || "");
  const [endpoint, setEndpoint] = createSignal(
    props.space.storage_config?.endpoint || "",
  );
  const [pending, setPending] = createSignal(false);
  const [message, setMessage] = createSignal("");
  const [messageType, setMessageType] = createSignal<"success" | "error">(
    "success",
  );
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
          ? { name: name() }
          : { storage_config: config() },
      );
      setMessageType("success");
      setMessage(t("spaceSettings.saved"));
    } catch (error) {
      setMessageType("error");
      setMessage(formatUserFacingError(error, "spaceSettings.failedSave"));
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
      setMessageType("success");
      setMessage(
        t("spaceSettings.connectionSuccessful", { status: result.status }),
      );
    } catch (error) {
      setMessageType("error");
      setMessage(
        formatUserFacingError(error, "spaceSettings.connectionFailed"),
      );
    } finally {
      setPending(false);
    }
  };
  return (
    <form class="settingsMain surface" onSubmit={save}>
      <Show
        when={section() === "general"}
        fallback={
          <div class="settingsSection ui-stack-sm">
            <h2>{t("spaceSettings.storage")}</h2>
            <p class="ui-muted">{storageSummary().description}</p>
            <p>
              <span class="ui-pill">{storageSummary().label}</span>
            </p>
            <Show when={storageSummary().uri}>
              <p class="ui-muted">
                <code>{storageSummary().uri}</code>
              </p>
            </Show>
            <p class="ui-alert ui-alert-warning">
              {t("spaceSettings.storageWarning")}
            </p>
            <div class="settingsGrid">
              <label>
                {t("spaceSettings.uri")}
                <input
                  id="storage-uri"
                  value={uri()}
                  onInput={(e) => setUri(e.currentTarget.value)}
                  placeholder="s3://ugoite-space/main"
                  required
                />
              </label>
            </div>
            <details class="settingsAdvanced">
              <summary>{t("settings.advancedDetails")}</summary>
              <div class="settingsGrid">
                <label>
                  {t("spaceSettings.endpoint")}
                  <input
                    id="storage-endpoint"
                    value={endpoint()}
                    onInput={(e) => setEndpoint(e.currentTarget.value)}
                    placeholder="https://s3.example.com"
                  />
                </label>
                <label>
                  {t("spaceSettings.status")}
                  <input
                    value={t("spaceSettings.configured")}
                    readOnly
                  />
                </label>
              </div>
            </details>
            <div class="actions">
              <button
                class="btn"
                type="button"
                onClick={() => void test()}
                disabled={pending() || !uri().trim()}
              >
                {t("spaceSettings.testConnection")}
              </button>
              <button class="btn primary" type="submit" disabled={pending()}>
                {t("spaceSettings.save")}
              </button>
            </div>
          </div>
        }
      >
        <div class="settingsSection">
          <h2>{t("spaceSettings.general")}</h2>
          <div class="settingsGrid">
            <label>
              {t("spaceSettings.spaceName")}
              <input
                id="space-name"
                value={name()}
                onInput={(e) => setName(e.currentTarget.value)}
                required
              />
            </label>
          </div>
          <button class="btn primary" type="submit" disabled={pending()}>
            {t("spaceSettings.save")}
          </button>
        </div>
      </Show>
      <Show when={message()}>
        <p
          class="ui-alert"
          classList={{
            "ui-alert-success": messageType() === "success",
            "ui-alert-error": messageType() === "error",
          }}
        >
          {message()}
        </p>
      </Show>
    </form>
  );
}
