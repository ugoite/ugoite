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
  type SelectedContextPreview,
  KonaseWorkFailure,
  KonaseWriteDeniedError,
  type WritePreview,
} from "~/lib/konase/host";
import { authorizeBrowserMcp } from "~/lib/konase/browser-mcp-auth";
import { BrowserMcpHost } from "~/lib/konase/mcp";
import { OpenAiModelHost } from "~/lib/konase/model";
import { entryApi, formApi, spaceApi } from "~/lib/ugoite-client";
import type { EntryPage, EntryQuery, EntryQueryResult } from "~/lib/entry-query";
import type { Form } from "~/lib/types";

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
  const [selectedUris, setSelectedUris] = createSignal<string[]>([]);
  const [forms, setForms] = createSignal<Form[]>([]);
  const [formsLoading, setFormsLoading] = createSignal(false);
  const [entryText, setEntryText] = createSignal("");
  const [entryRows, setEntryRows] = createSignal<EntryQueryResult[]>([]);
  const [entryPage, setEntryPage] = createSignal<EntryPage>();
  const [entrySearchLoading, setEntrySearchLoading] = createSignal(false);
  const [entryCursorPath, setEntryCursorPath] = createSignal<(string | undefined)[]>([
    undefined,
  ]);
  const [contextPreview, setContextPreview] = createSignal<SelectedContextPreview>();
  const [undone, setUndone] = createSignal(false);
  const [error, setError] = createSignal<string>();
  let entrySearchGeneration = 0;
  let entrySearchController: AbortController | undefined;
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
      setSelectedUris([]);
      setForms([]);
      setFormsLoading(false);
      setEntryRows([]);
      setEntryPage(undefined);
      setEntryText("");
      setEntryCursorPath([undefined]);
      setEntrySearchLoading(false);
      setContextPreview(undefined);
      entrySearchController?.abort();
      entrySearchController = undefined;
      entrySearchGeneration += 1;
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
    setContextPreview(undefined);
    entrySearchController?.abort();
    entrySearchGeneration += 1;
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
      void loadFormCandidates(requestedSpaceId, configureLifetime);
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
      if (selectedUris().length > 0) {
        setContextPreview(undefined);
        const selectionSnapshot = [...selectedUris()];
        const preview = await host.previewSelectedContext(value, selectionSnapshot);
        if (
          !isCurrentLifetime(submitLifetime) ||
          prompt().trim() !== value ||
          JSON.stringify(selectedUris()) !== JSON.stringify(selectionSnapshot)
        ) {
          host.invalidateContextPreview();
          return;
        }
        setContextPreview(preview);
        return;
      }
      setContextPreview(undefined);
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

  const loadFormCandidates = async (
    spaceId: string,
    hostLifetime: PanelLifetime,
  ) => {
    setFormsLoading(true);
    try {
      const candidates = await formApi.list(spaceId);
      if (isCurrentLifetime(hostLifetime)) setForms(candidates);
    } catch (cause) {
      if (isCurrentLifetime(hostLifetime)) {
        setError(formatUserFacingError(cause, "konase.candidateError"));
      }
    } finally {
      if (isCurrentLifetime(hostLifetime)) setFormsLoading(false);
    }
  };

  const searchEntries = async (
    after: string | undefined = undefined,
    nextPath?: (string | undefined)[],
  ) => {
    const spaceId = props.spaceId;
    const hostLifetime = captureLifetime();
    if (!activeHost()) return;
    const generation = ++entrySearchGeneration;
    entrySearchController?.abort();
    const controller = new AbortController();
    entrySearchController = controller;
    setEntrySearchLoading(true);
    setError(undefined);
    if (after === undefined) {
      setEntryRows([]);
      setEntryPage(undefined);
    }
    const text = entryText().trim();
    const query: EntryQuery = {
      scope: { kind: "all" },
      ...(text ? { text } : {}),
      filters: [],
      sort: [],
    };
    try {
      const page = await entryApi.query(spaceId, {
        query,
        projection: { kind: "preview" },
        limit: 20,
        ...(after ? { after } : {}),
      }, controller.signal);
      if (
        generation !== entrySearchGeneration || controller.signal.aborted ||
        !isCurrentLifetime(hostLifetime)
      ) return;
      setEntryRows(page.rows);
      setEntryPage(page);
      if (nextPath) setEntryCursorPath(nextPath);
    } catch (cause) {
      if (
        generation === entrySearchGeneration && !controller.signal.aborted &&
        isCurrentLifetime(hostLifetime)
      ) {
        setError(formatUserFacingError(cause, "konase.candidateError"));
      }
    } finally {
      if (generation === entrySearchGeneration) {
        if (entrySearchController === controller) entrySearchController = undefined;
        setEntrySearchLoading(false);
      }
    }
  };

  const toggleResource = (uri: string, checked: boolean) => {
    const current = selectedUris();
    if (checked && current.includes(uri)) return;
    if (checked && current.length >= 4) return;
    activeHost()?.invalidateContextPreview();
    setContextPreview(undefined);
    setSelectedUris((value) => checked
      ? [...value, uri]
      : value.filter((selected) => selected !== uri));
  };

  const editEntryText = (value: string) => {
    setEntryText(value);
    entrySearchGeneration += 1;
    entrySearchController?.abort();
    entrySearchController = undefined;
    setEntrySearchLoading(false);
    setEntryRows([]);
    setEntryPage(undefined);
    setEntryCursorPath([undefined]);
  };

  const editPrompt = (value: string) => {
    if (contextPreview()) {
      activeHost()?.invalidateContextPreview();
      setContextPreview(undefined);
    }
    setPrompt(value);
  };

  const cancelContextPreview = () => {
    activeHost()?.invalidateContextPreview();
    setContextPreview(undefined);
  };

  const sendContextPreview = async () => {
    const host = activeHost();
    const preview = contextPreview();
    if (!host || !preview || running()) return;
    setError(undefined);
    setTurn(undefined);
    setUndone(false);
    setSteps([]);
    setRunning(true);
    const previewLifetime = captureLifetime();
    try {
      const result = await host.sendSelectedContext(preview.id);
      if (!isCurrentLifetime(previewLifetime)) return;
      setTurn(result);
      setPrompt("");
      setSelectedUris([]);
      setContextPreview(undefined);
    } catch (cause) {
      if (isCurrentLifetime(previewLifetime)) {
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
        setContextPreview(undefined);
      }
    } finally {
      if (isCurrentLifetime(previewLifetime)) setRunning(false);
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
        <section class="ui-stack-sm" aria-labelledby="konase-resource-selection-title">
          <h3 id="konase-resource-selection-title">
            {t("konase.resourceSelection")}
          </h3>
          <p class="ui-muted" role="status" aria-live="polite">
            {t("konase.resourceSelectionCount", { count: selectedUris().length })}
          </p>
          <Show when={selectedUris().length > 0}>
            <ul aria-label={t("konase.selectedResources")}>
              <For each={selectedUris()}>
                {(uri) => (
                  <li>
                    <span>{uri}</span>
                    <button
                      class="btn"
                      type="button"
                      disabled={running()}
                      aria-label={t("konase.removeSelectedResource", { uri })}
                      onClick={() => toggleResource(uri, false)}
                    >
                      {t("konase.removeSelectedResourceButton")}
                    </button>
                  </li>
                )}
              </For>
            </ul>
          </Show>
          <fieldset class="ui-stack-sm" disabled={running()}>
            <legend>{t("konase.formCandidates")}</legend>
            <Show when={formsLoading()}>
              <p class="ui-muted" role="status">{t("konase.loadingCandidates")}</p>
            </Show>
            <Show when={!formsLoading() && forms().length === 0}>
              <p class="ui-muted">{t("konase.noFormCandidates")}</p>
            </Show>
            <For each={forms()}>
              {(form) => {
                const uri = form.id ? `ugoite://form/${form.id}` : "";
                return (
                  <label>
                    <input
                      type="checkbox"
                      checked={Boolean(uri && selectedUris().includes(uri))}
                      disabled={!uri || (selectedUris().length >= 4 && !selectedUris().includes(uri))}
                      onChange={(event) => toggleResource(uri, event.currentTarget.checked)}
                    />
                    {form.name} {form.id ? `(${form.id})` : ""}
                  </label>
                );
              }}
            </For>
          </fieldset>
          <form class="ui-inline-actions" onSubmit={(event) => {
            event.preventDefault();
            setEntryCursorPath([undefined]);
            void searchEntries(undefined, [undefined]);
          }}>
            <label>
              {t("konase.entrySearch")}
              <input
              type="search"
              value={entryText()}
              disabled={running()}
              onInput={(event) => editEntryText(event.currentTarget.value)}
            />
          </label>
          <button class="btn" type="submit" disabled={running() || entrySearchLoading()}>
              {t("konase.searchEntries")}
            </button>
          </form>
          <Show when={entrySearchLoading()}>
            <p class="ui-muted" role="status">{t("konase.loadingCandidates")}</p>
          </Show>
          <Show when={entryRows().length > 0}>
            <fieldset class="ui-stack-sm" disabled={running()}>
              <legend>{t("konase.entryCandidates")}</legend>
              <For each={entryRows()}>
                {(entry) => {
                  const uri = `ugoite://entry/${entry.id}`;
                  return (
                    <label>
                      <input
                        type="checkbox"
                        checked={selectedUris().includes(uri)}
                        disabled={selectedUris().length >= 4 && !selectedUris().includes(uri)}
                        onChange={(event) => toggleResource(uri, event.currentTarget.checked)}
                      />
                      {entry.preview || entry.id} ({entry.id})
                    </label>
                  );
                }}
              </For>
            </fieldset>
          </Show>
          <div class="ui-inline-actions">
            <button
              class="btn"
              type="button"
              disabled={running() || entrySearchLoading() || entryCursorPath().length <= 1}
              onClick={() => {
                const path = entryCursorPath();
                const previous = path.slice(0, -1);
                const after = previous[previous.length - 1];
                void searchEntries(after, previous);
              }}
            >
              {t("konase.previousEntries")}
            </button>
            <button
              class="btn"
              type="button"
              disabled={running() || entrySearchLoading() || !entryPage()?.has_more || !entryPage()?.next}
              onClick={() => {
                const cursor = entryPage()?.next;
                if (!cursor) return;
                void searchEntries(cursor, [...entryCursorPath(), cursor]);
              }}
            >
              {t("konase.nextEntries")}
            </button>
          </div>
        </section>
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
            onInput={(event) => editPrompt(event.currentTarget.value)}
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
            {selectedUris().length > 0
              ? t("konase.previewContext")
              : t("konase.submit")}
          </button>
        </form>
        <Show when={contextPreview()}>
          {(preview) => (
            <section
              class="ui-card ui-stack-sm"
              aria-labelledby="konase-context-preview-title"
              aria-describedby="konase-context-preview-description"
            >
              <h3 id="konase-context-preview-title">
                {t("konase.contextPreviewTitle")}
              </h3>
              <p id="konase-context-preview-description" class="ui-muted" role="status" aria-live="polite">
                {t("konase.contextPreviewUntrusted")}
              </p>
              <ul class="ui-stack-sm">
                <For each={preview().admission}>
                  {(admission) => {
                    const resource = preview().resources.find((item) => item.uri === admission.uri);
                    return (
                      <li>
                        <p>
                          <strong>{admission.uri}</strong>: {resourceStatusLabel(admission.status)}
                          {admission.reason
                            ? ` — ${resourceReasonLabel(admission.reason)}`
                            : ""}
                        </p>
                        <Show when={resource}>
                          {(content) => <pre class="ui-scroll-x">{content().content}</pre>}
                        </Show>
                      </li>
                    );
                  }}
                </For>
              </ul>
              <div class="ui-inline-actions">
                <button
                  class="btn primary"
                  type="button"
                  disabled={running()}
                  aria-busy={running() || undefined}
                  onClick={() => void sendContextPreview()}
                >
                  <Show when={running()}><ButtonSpinner /></Show>
                  {t("konase.sendSelectedContext")}
                </button>
                <button class="btn" type="button" disabled={running()} onClick={cancelContextPreview}>
                  {t("konase.cancelContextPreview")}
                </button>
              </div>
            </section>
          )}
        </Show>
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

const resourceStatusLabel = (status: SelectedContextPreview["admission"][number]["status"]): string => {
  switch (status) {
    case "included":
      return t("konase.resourceStatus.included");
    case "truncated":
      return t("konase.resourceStatus.truncated");
    case "omitted":
      return t("konase.resourceStatus.omitted");
  }
};

const resourceReasonLabel = (
  reason: NonNullable<SelectedContextPreview["admission"][number]["reason"]>,
): string => {
  switch (reason) {
    case "projection_compacted":
      return t("konase.resourceReason.projection_compacted");
    case "context_byte_budget":
      return t("konase.resourceReason.context_byte_budget");
    case "model_prompt_character_limit":
      return t("konase.resourceReason.model_prompt_character_limit");
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
