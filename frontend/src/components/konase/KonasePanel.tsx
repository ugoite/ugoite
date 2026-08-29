import { createSignal, For, onCleanup, onMount, Show } from "solid-js";
import { t } from "~/lib/i18n";
import { formatUserFacingError } from "~/lib/user-facing-error";
import {
  KonaseHost,
  type KonaseProgress,
  type KonaseTurn,
} from "~/lib/konase/host";
import { BrowserMcpHost } from "~/lib/konase/mcp";
import { OpenAiModelHost } from "~/lib/konase/model";

type KonasePanelProps = {
  host?: KonaseHost;
};

/** Small browser surface for the same disposable Work used by the CLI. */
export function KonasePanel(props: KonasePanelProps) {
  const [configuredHost, setConfiguredHost] = createSignal<KonaseHost>();
  const activeHost = () => props.host ?? configuredHost();
  const [modelApiKey, setModelApiKey] = createSignal("");
  const [mcpAccessToken, setMcpAccessToken] = createSignal("");
  const [prompt, setPrompt] = createSignal("");
  const [running, setRunning] = createSignal(false);
  const [undoing, setUndoing] = createSignal(false);
  const [steps, setSteps] = createSignal<string[]>([]);
  const [turn, setTurn] = createSignal<KonaseTurn>();
  const [undone, setUndone] = createSignal(false);
  const [error, setError] = createSignal<string>();
  let unsubscribe: (() => void) | undefined;

  const subscribe = (host: KonaseHost) => {
    unsubscribe?.();
    unsubscribe = host.subscribeProgress((progress) => {
      setSteps((current) => [...current, progressLabel(progress)]);
    });
  };

  onMount(() => {
    if (props.host) subscribe(props.host);
    onCleanup(() => unsubscribe?.());
  });

  const configure = (event: SubmitEvent) => {
    event.preventDefault();
    try {
      const host = new KonaseHost({
        model: new OpenAiModelHost({ apiKey: modelApiKey() }),
        mcp: new BrowserMcpHost({ accessToken: mcpAccessToken() }),
      });
      setConfiguredHost(host);
      subscribe(host);
      setError(undefined);
    } catch (cause) {
      setError(formatUserFacingError(cause, "konase.error"));
    }
  };

  const submit = async (event: SubmitEvent) => {
    event.preventDefault();
    const host = activeHost();
    if (!host) {
      setError(t("konase.hostRequired"));
      return;
    }
    const value = prompt().trim();
    if (!value || running()) return;
    setError(undefined);
    setTurn(undefined);
    setUndone(false);
    setSteps([]);
    setRunning(true);
    try {
      setTurn(await host.submit(value));
      setPrompt("");
    } catch (cause) {
      setError(
        formatUserFacingError(cause, "konase.error"),
      );
    } finally {
      setRunning(false);
    }
  };

  const undo = async () => {
    const host = activeHost();
    const current = turn();
    if (!host || !current || !current.undoAvailable || undone()) return;
    setError(undefined);
    setUndoing(true);
    try {
      await host.undo(current.workId);
      setUndone(true);
    } catch (cause) {
      setError(formatUserFacingError(cause, "konase.error"));
    } finally {
      setUndoing(false);
    }
  };

  return (
    <section class="surface ui-stack" aria-labelledby="konase-panel-heading">
      <div class="sectionHead">
        <h2 id="konase-panel-heading">{t("konase.title")}</h2>
      </div>
      <Show
        when={activeHost()}
        fallback={
          <form class="ui-stack-sm" onSubmit={configure}>
            <p class="ui-muted">{t("konase.credentialsHint")}</p>
            <label>
              {t("konase.modelKey")}
              <input
                type="password"
                value={modelApiKey()}
                autocomplete="off"
                onInput={(event) => setModelApiKey(event.currentTarget.value)}
              />
            </label>
            <label>
              {t("konase.mcpToken")}
              <input
                type="password"
                value={mcpAccessToken()}
                autocomplete="off"
                onInput={(event) =>
                  setMcpAccessToken(event.currentTarget.value)}
              />
            </label>
            <button
              class="btn"
              type="submit"
              disabled={!modelApiKey().trim() || !mcpAccessToken().trim()}
            >
              {t("konase.connect")}
            </button>
          </form>
        }
      >
        <form class="ui-stack-sm" onSubmit={submit}>
          <label class="ui-sr-only" for="konase-prompt">
            {t("konase.title")}
          </label>
          <textarea
            id="konase-prompt"
            rows="3"
            value={prompt()}
            placeholder={t("konase.promptPlaceholder")}
            disabled={running()}
            onInput={(event) =>
              setPrompt(event.currentTarget.value)}
          />
          <button
            class="btn primary"
            type="submit"
            disabled={running() || !prompt().trim()}
          >
            {running() ? t("konase.running") : t("konase.submit")}
          </button>
        </form>
      </Show>

      <Show when={steps().length > 0}>
        <ol class="ui-stack-sm" aria-label={t("konase.title")}>
          <For each={steps()}>{(step) => <li>✓ {step}</li>}</For>
        </ol>
      </Show>
      <Show when={error()}>
        <p class="ui-alert ui-alert-error" role="alert">{error()}</p>
      </Show>
      <Show when={turn()}>
        {(current) => (
          <div class="ui-card ui-stack-sm">
            <p>{current().outcome.summary}</p>
            <Show when={current().undoAvailable && !undone()}>
              <button
                class="btn"
                type="button"
                disabled={undoing()}
                onClick={() => void undo()}
              >
                {undoing() ? t("konase.undoing") : t("konase.undo")}
              </button>
            </Show>
            <Show when={undone()}>
              <p class="ui-text-success" role="status">{t("konase.undone")}</p>
            </Show>
          </div>
        )}
      </Show>
    </section>
  );
}

const progressLabel = (progress: KonaseProgress): string => {
  switch (progress.kind) {
    case "model":
      return t("konase.model");
    case "mcp":
      return t("konase.mcp", { operation: progress.operation });
    case "complete":
      return t("konase.complete");
    case "undo":
      return t("konase.undone");
  }
};
