import { createEffect, createSignal, For, onCleanup, Show } from "solid-js";
import { t } from "~/lib/i18n";
import { formatUserFacingError } from "~/lib/user-facing-error";
import {
  KonaseHost,
  type KonaseProgress,
  type KonaseTurn,
} from "~/lib/konase/host";
import { authorizeBrowserMcp } from "~/lib/konase/browser-mcp-auth";
import { BrowserMcpHost } from "~/lib/konase/mcp";
import { OpenAiModelHost } from "~/lib/konase/model";
import { spaceApi } from "~/lib/ugoite-client";

type KonasePanelProps = {
  spaceId: string;
};

type PanelLifetime = {
  generation: number;
  spaceId: string;
};

/** Small browser surface for the same disposable Work used by the CLI. */
export function KonasePanel(props: KonasePanelProps) {
  const [configuredHost, setConfiguredHost] = createSignal<KonaseHost>();
  const [configuredSpaceId, setConfiguredSpaceId] = createSignal<string>();
  const activeHost = () =>
    configuredSpaceId() === props.spaceId ? configuredHost() : undefined;
  const [modelApiKey, setModelApiKey] = createSignal("");
  const [approvalUrl, setApprovalUrl] = createSignal<string>();
  const [connecting, setConnecting] = createSignal(false);
  const [prompt, setPrompt] = createSignal("");
  const [running, setRunning] = createSignal(false);
  const [undoing, setUndoing] = createSignal(false);
  const [steps, setSteps] = createSignal<string[]>([]);
  const [turn, setTurn] = createSignal<KonaseTurn>();
  const [undone, setUndone] = createSignal(false);
  const [error, setError] = createSignal<string>();
  let unsubscribe: (() => void) | undefined;
  let pendingSpaceId: string | undefined;
  let lifetime: PanelLifetime = { generation: 0, spaceId: props.spaceId };

  const captureLifetime = (): PanelLifetime => ({ ...lifetime });
  const isCurrentLifetime = (candidate: PanelLifetime) =>
    candidate.generation === lifetime.generation &&
    candidate.spaceId === lifetime.spaceId &&
    candidate.spaceId === props.spaceId;

  createEffect(() => {
    const currentSpaceId = props.spaceId;
    if (lifetime.spaceId !== currentSpaceId) {
      lifetime = {
        generation: lifetime.generation + 1,
        spaceId: currentSpaceId,
      };
      if (pendingSpaceId && pendingSpaceId !== currentSpaceId) {
        pendingSpaceId = undefined;
      }
      unsubscribe?.();
      unsubscribe = undefined;
      setConfiguredHost(undefined);
      setConfiguredSpaceId(undefined);
      setApprovalUrl(undefined);
      setConnecting(false);
      setPrompt("");
      setRunning(false);
      setUndoing(false);
      setTurn(undefined);
      setUndone(false);
      setSteps([]);
      setError(undefined);
    }
  });

  const subscribe = (host: KonaseHost, hostLifetime: PanelLifetime) => {
    unsubscribe?.();
    unsubscribe = host.subscribeProgress((progress) => {
      if (!isCurrentLifetime(hostLifetime)) return;
      setSteps((current) => [...current, progressLabel(progress)]);
    });
  };
  onCleanup(() => unsubscribe?.());

  const configure = async (event: SubmitEvent) => {
    event.preventDefault();
    if (connecting()) return;
    const configureLifetime = captureLifetime();
    const requestedSpaceId = configureLifetime.spaceId;
    pendingSpaceId = requestedSpaceId;
    setConnecting(true);
    setApprovalUrl(undefined);
    setError(undefined);
    try {
      const space = await spaceApi.get(requestedSpaceId);
      const spaceUid = space.space_uid?.trim();
      if (!spaceUid) {
        throw new Error("Current Space metadata did not include a Space UID");
      }
      const credential = await authorizeBrowserMcp({
        spaceUid,
        deviceName: `Ugoite Browser Konase (${requestedSpaceId})`,
        onApprovalRequired: ({ verificationUriComplete }) => {
          if (isCurrentLifetime(configureLifetime)) {
            setApprovalUrl(verificationUriComplete);
          }
        },
      });
      if (!isCurrentLifetime(configureLifetime)) return;
      const host = new KonaseHost({
        model: new OpenAiModelHost({ apiKey: modelApiKey() }),
        mcp: new BrowserMcpHost(credential),
      });
      setConfiguredHost(host);
      setConfiguredSpaceId(requestedSpaceId);
      subscribe(host, configureLifetime);
    } catch (cause) {
      if (isCurrentLifetime(configureLifetime)) {
        setError(formatUserFacingError(cause, "konase.error"));
      }
    } finally {
      if (isCurrentLifetime(configureLifetime)) {
        if (pendingSpaceId === requestedSpaceId) pendingSpaceId = undefined;
        setConnecting(false);
      }
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
    const submitLifetime = captureLifetime();
    try {
      const result = await host.submit(value);
      if (!isCurrentLifetime(submitLifetime)) return;
      setTurn(result);
      setPrompt("");
    } catch (cause) {
      if (isCurrentLifetime(submitLifetime)) {
        setError(
          formatUserFacingError(cause, "konase.error"),
        );
      }
    } finally {
      if (isCurrentLifetime(submitLifetime)) setRunning(false);
    }
  };

  const undo = async () => {
    const host = activeHost();
    const current = turn();
    if (!host || !current || !current.undoAvailable || undone()) return;
    setError(undefined);
    setUndoing(true);
    const undoLifetime = captureLifetime();
    try {
      await host.undo(current.workId);
      if (isCurrentLifetime(undoLifetime)) setUndone(true);
    } catch (cause) {
      if (isCurrentLifetime(undoLifetime)) {
        setError(formatUserFacingError(cause, "konase.error"));
      }
    } finally {
      if (isCurrentLifetime(undoLifetime)) setUndoing(false);
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
            <button
              class="btn"
              type="submit"
              disabled={connecting() || !modelApiKey().trim()}
            >
              {connecting() ? t("konase.connecting") : t("konase.connect")}
            </button>
            <Show when={approvalUrl()}>
              {(url) => (
                <p class="ui-muted" role="status">
                  {t("konase.approvalRequired")}{"  "}
                  <a href={url()} target="_blank" rel="noopener noreferrer">
                    {t("konase.openApproval")}
                  </a>
                </p>
              )}
            </Show>
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
