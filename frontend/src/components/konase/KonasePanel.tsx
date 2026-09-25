import { createEffect, createSignal, For, onCleanup, Show } from "solid-js";
import { ButtonSpinner } from "~/components/ButtonSpinner";
import { t } from "~/lib/i18n";
import { UgoiteApiError } from "~/lib/ugoite-client/protocol";
import { formatUserFacingError } from "~/lib/user-facing-error";
import {
  KonaseHost,
  KonaseMutationUnconfirmedError,
  type KonaseProgress,
  type KonaseTurn,
  KonaseWorkFailure,
  KonaseWriteDeniedError,
  type WritePreview,
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
  const [confirmation, setConfirmation] = createSignal<WritePreview>();
  const [confirmationSubmitting, setConfirmationSubmitting] = createSignal(
    false,
  );
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
      configuredHost()?.cancelPending();
      configuredHost()?.dispose();
      setConfirmation(undefined);
      setConfirmationSubmitting(false);
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
      if (
        progress.kind === "mcp" &&
        confirmation()?.operation === progress.operation &&
        confirmationSubmitting()
      ) {
        setConfirmation(undefined);
        setConfirmationSubmitting(false);
      }
      setSteps((current) => [...current, progressLabel(progress)]);
    });
  };
  onCleanup(() => {
    configuredHost()?.cancelPending();
    configuredHost()?.dispose();
    setConfirmation(undefined);
    unsubscribe?.();
  });

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
        throw new UgoiteApiError({
          kind: "invalid_arguments",
          code: "INVALID_INPUT",
          operation: "space.get",
          message: "Current Space metadata did not include a Space UID",
          detail: { kind: "space_identity", space_id: requestedSpaceId },
        });
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
        spaceId: requestedSpaceId,
        onConfirmationRequired: (preview) => {
          if (!isCurrentLifetime(configureLifetime)) {
            host.resolveConfirmation(preview.requestId, false);
            return;
          }
          setConfirmationSubmitting(false);
          setConfirmation(preview);
        },
        onConfirmationCancelled: (requestId) => {
          if (
            isCurrentLifetime(configureLifetime) &&
            confirmation()?.requestId === requestId
          ) {
            setConfirmation(undefined);
            setConfirmationSubmitting(false);
          }
        },
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
        if (cause instanceof KonaseWorkFailure) {
          setTurn({
            outcome: {
              job_id: cause.partial.jobId,
              summary: t("konase.partialWork"),
              meaningful: false,
            },
            workId: cause.partial.workId,
            undoAvailable: cause.partial.undoAvailable,
            knowledge: cause.partial.knowledge,
          });
        }
        setError(konaseErrorMessage(cause));
      }
    } finally {
      if (isCurrentLifetime(submitLifetime)) setRunning(false);
    }
  };

  const undo = async () => {
    const host = activeHost();
    const current = turn();
    if (!host || !current || !current.undoAvailable || undone() || undoing()) {
      return;
    }
    setError(undefined);
    setUndoing(true);
    const undoLifetime = captureLifetime();
    try {
      await host.undo(current.workId);
      if (isCurrentLifetime(undoLifetime)) {
        setUndone(true);
        setTurn((turn) =>
          turn
            ? { ...turn, undoAvailable: false, knowledge: "unchanged" }
            : turn
        );
      }
    } catch (cause) {
      if (isCurrentLifetime(undoLifetime)) {
        setError(konaseErrorMessage(cause));
      }
    } finally {
      if (isCurrentLifetime(undoLifetime)) setUndoing(false);
    }
  };

  const approve = () => {
    const pending = confirmation();
    const host = activeHost();
    if (!pending || !host || confirmationSubmitting()) return;
    setConfirmationSubmitting(true);
    if (!host.resolveConfirmation(pending.requestId, true)) {
      setConfirmation(undefined);
      setConfirmationSubmitting(false);
    }
  };

  const deny = () => {
    const pending = confirmation();
    const host = activeHost();
    if (!pending || !host || confirmationSubmitting()) return;
    setConfirmation(undefined);
    setConfirmationSubmitting(false);
    host.resolveConfirmation(pending.requestId, false);
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
              aria-busy={connecting() || undefined}
            >
              <Show when={connecting()}>
                <ButtonSpinner />
              </Show>
              {t("konase.connect")}
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
            aria-busy={running() || undefined}
          >
            <Show when={running()}>
              <ButtonSpinner />
            </Show>
            {t("konase.submit")}
          </button>
        </form>
      </Show>

      <Show when={steps().length > 0}>
        <ol class="ui-stack-sm" aria-label={t("konase.title")}>
          <For each={steps()}>{(step) => <li>{step}</li>}</For>
        </ol>
      </Show>
      <Show when={confirmation()}>
        {(preview) => (
          <section
            class="ui-card ui-stack-sm"
            aria-labelledby="konase-write-confirmation-title"
            aria-describedby="konase-write-confirmation-summary"
            role="alertdialog"
            aria-modal="true"
          >
            <h3 id="konase-write-confirmation-title">
              {t("konase.writeApprovalTitle")}
            </h3>
            <p>
              {preview().action === "undo"
                ? t("konase.undo")
                : t(`konase.${preview().action}`)}
            </p>
            <dl class="ui-stack-sm">
              <div>
                <dt>{t("konase.approvalTarget")}</dt>
                <dd>
                  {preview().spaceId}
                  {preview().form ? ` / ${preview().form}` : ""}
                  {preview().entryId ? ` / ${preview().entryId}` : ""}
                </dd>
              </div>
              <div>
                <dt>{t("konase.approvalDetails")}</dt>
                <dd id="konase-write-confirmation-summary">
                  {preview().summary}
                </dd>
              </div>
            </dl>
            <Show when={confirmationSubmitting()}>
              <p role="status">{t("konase.saving")}</p>
            </Show>
            <div class="ui-inline-actions">
              <button
                class="btn primary"
                type="button"
                aria-label={t("konase.approveWrite")}
                disabled={confirmationSubmitting()}
                aria-busy={confirmationSubmitting() || undefined}
                onClick={approve}
              >
                <Show when={confirmationSubmitting()}>
                  <ButtonSpinner />
                </Show>
                {t("konase.approveWrite")}
              </button>
              <button
                class="btn"
                type="button"
                aria-label={t("konase.denyWrite")}
                disabled={confirmationSubmitting()}
                onClick={deny}
              >
                {t("konase.denyWrite")}
              </button>
            </div>
          </section>
        )}
      </Show>
      <Show when={error()}>
        <p class="ui-alert ui-alert-error" role="alert">{error()}</p>
      </Show>
      <Show when={turn()}>
        {(current) => (
          <div class="ui-card ui-stack-sm">
            <p>{current().outcome.summary}</p>
            <p class="ui-muted" role="status">
              {t("konase.knowledge", {
                outcome: knowledgeLabel(current().knowledge),
              })}
            </p>
            <Show when={current().undoAvailable && !undone()}>
              <button
                class="btn"
                type="button"
                disabled={undoing()}
                aria-busy={undoing() || undefined}
                onClick={() => void undo()}
              >
                <Show when={undoing()}>
                  <ButtonSpinner />
                </Show>
                {t("konase.undo")}
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
      return t("konase.mcpStarted", { operation: progress.operation });
    case "complete":
      return t("konase.complete");
    case "knowledge":
      return t("konase.knowledge", {
        outcome: knowledgeLabel(progress.outcome),
      });
    case "undo":
      return t("konase.undone");
  }
};

const knowledgeLabel = (outcome: KonaseTurn["knowledge"]): string => {
  switch (outcome) {
    case "unchanged":
      return t("konase.knowledgeUnchanged");
    case "saved":
      return t("konase.knowledgeSaved");
    case "write_failed":
      return t("konase.knowledgeWriteFailed");
  }
};

const konaseErrorMessage = (cause: unknown): string => {
  if (cause instanceof KonaseWorkFailure) {
    return konaseErrorMessage(cause.reason);
  }
  if (cause instanceof KonaseWriteDeniedError) return t("konase.writeDenied");
  if (cause instanceof KonaseMutationUnconfirmedError) {
    return t("konase.unconfirmed");
  }
  return formatUserFacingError(cause, "konase.error");
};
