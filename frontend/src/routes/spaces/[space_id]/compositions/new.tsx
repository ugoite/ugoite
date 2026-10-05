import { useLocation, useNavigate, useParams } from "@solidjs/router";
import { createEffect, createSignal, For, onCleanup, Show } from "solid-js";
import { BackLink } from "~/components/BackLink";
import {
  CompositionDiagnostics,
  CompositionRenderer,
} from "~/components/CompositionRenderer";
import {
  CompositionSourcePicker,
  type CompositionSourceSeed,
} from "~/components/CompositionSourcePicker";
import { IconButton } from "~/components/IconButton";
import { LocalBusyIndicator } from "~/components/LocalBusyIndicator";
import { RowList, RowListButton, RowListItem } from "~/components/RowList";
import { UiIcon } from "~/components/UiIcon";
import {
  addEntryQuerySource,
  addSavedSqlSource,
  canonicalizeDraft,
  type CompositionDraft,
  createEmptyDraft,
  type DraftSource,
  moveSource,
  removeSource,
  setDraftName,
  setDraftTags,
} from "~/lib/composition-draft";
import {
  compositionApi,
  type CompositionPreviewPlan,
  type CompositionResolveDiagnostic,
  type CompositionResolvedSource,
} from "~/lib/composition-api";
import { compositionSaveErrorMessage } from "~/lib/composition-save-error";
import {
  clearPendingCompositionSaveAttempt,
  markPendingCompositionSaveAttemptUncertain,
  type PendingCompositionSaveAttempt,
  stagePendingCompositionSaveAttempt,
} from "~/lib/composition-save-attempt";
import type { CompositionSourcePageState } from "~/lib/composition-query-handle";
import { t } from "~/lib/i18n";
import {
  spaceCompositionRevisionPath,
  spaceCompositionsPath,
} from "~/lib/space-path";
import { spaceRoute } from "~/lib/space-shell-route";

export const route = spaceRoute({ navigation: "home" });

type StudioPreview =
  | { status: "idle" }
  | { status: "previewing" }
  | {
    status: "ready";
    plan: Pick<CompositionPreviewPlan, "sources" | "component_bindings">;
    sources: Record<string, CompositionSourcePageState>;
  }
  | { status: "diagnostics"; diagnostics: CompositionResolveDiagnostic[] }
  | { status: "error" };

const PREVIEW_DEBOUNCE_MS = 400;

const isAbort = (error: unknown): boolean =>
  !!error && typeof error === "object" &&
  (error as { name?: unknown }).name === "AbortError";

const parseTags = (value: string): string[] => {
  const seen = new Set<string>();
  for (const tag of value.split(/[,\s]+/)) {
    const trimmed = tag.trim();
    if (trimmed && !seen.has(trimmed)) seen.add(trimmed);
  }
  return [...seen];
};

const sourceKindIcon = (source: DraftSource): "sql" | "forms" =>
  source.kind === "saved_sql" ? "sql" : "forms";

const sourceKindLabel = (source: DraftSource): string =>
  source.kind === "saved_sql"
    ? t("spaceShell.title.savedSql")
    : t("common.form");

export default function CompositionNewRoute() {
  const params = useParams<{ space_id: string }>();
  const location = useLocation();
  const navigate = useNavigate();
  const spaceId = () => params.space_id;

  const [draft, setDraft] = createSignal<CompositionDraft>(createEmptyDraft());
  const [pickerOpen, setPickerOpen] = createSignal(false);
  const [expandedId, setExpandedId] = createSignal<string | null>(null);
  const [preview, setPreview] = createSignal<StudioPreview>({ status: "idle" });
  const [saving, setSaving] = createSignal(false);
  const [saveError, setSaveError] = createSignal<string | null>(null);
  const [saveRetryAvailable, setSaveRetryAvailable] = createSignal(false);

  let previewGeneration = 0;
  let previewController: AbortController | undefined;
  let previewTimer: ReturnType<typeof setTimeout> | undefined;
  const resolvedSources = new Map<string, CompositionResolvedSource>();
  const sourceGenerations = new Map<string, number>();
  const sourceControllers = new Map<string, AbortController>();

  let pendingSave: PendingCompositionSaveAttempt | undefined;

  const saveRoute = () => ({
    spaceId: spaceId(),
    routePath: location.pathname,
  });
  const isCurrentSaveRoute = (attempt: PendingCompositionSaveAttempt) =>
    attempt.spaceId === spaceId() && attempt.routePath === location.pathname;
  const canSave = () =>
    draft().name.trim().length > 0 && draft().sources.length > 0 &&
    !saving();

  onCleanup(() => {
    previewGeneration += 1;
    previewController?.abort();
    if (previewTimer !== undefined) clearTimeout(previewTimer);
    for (const controller of sourceControllers.values()) controller.abort();
    sourceControllers.clear();
  });

  const loadSourcePage = (
    requestGeneration: number,
    source: CompositionResolvedSource,
    cursor: string | undefined,
    cursorStack: (string | undefined)[],
  ) => {
    const sourceId = source.source_id;
    sourceControllers.get(sourceId)?.abort();
    const sourceGeneration = (sourceGenerations.get(sourceId) ?? 0) + 1;
    sourceGenerations.set(sourceId, sourceGeneration);
    resolvedSources.set(sourceId, source);
    const controller = new AbortController();
    sourceControllers.set(sourceId, controller);
    setPreview((current) => {
      if (current.status !== "ready") return current;
      return {
        ...current,
        sources: {
          ...current.sources,
          [sourceId]: { status: "loading", cursorStack },
        },
      };
    });
    void compositionApi.querySource(
      spaceId(),
      source,
      cursor,
      controller.signal,
    ).then(
      (page) => {
        if (
          requestGeneration !== previewGeneration ||
          controller.signal.aborted ||
          sourceGenerations.get(sourceId) !== sourceGeneration
        ) return;
        setPreview((current) => {
          if (current.status !== "ready") return current;
          return {
            ...current,
            sources: {
              ...current.sources,
              [sourceId]: { status: "ready", cursorStack, cursor, page },
            },
          };
        });
      },
      (error: unknown) => {
        if (
          requestGeneration !== previewGeneration ||
          controller.signal.aborted || isAbort(error) ||
          sourceGenerations.get(sourceId) !== sourceGeneration
        ) return;
        setPreview((current) => {
          if (current.status !== "ready") return current;
          return {
            ...current,
            sources: {
              ...current.sources,
              [sourceId]: { status: "error", cursorStack, cursor, error },
            },
          };
        });
      },
    ).finally(() => {
      if (sourceControllers.get(sourceId) === controller) {
        sourceControllers.delete(sourceId);
      }
    });
  };

  const runPreview = async (snapshot: CompositionDraft) => {
    const requestGeneration = ++previewGeneration;
    previewController?.abort();
    const controller = new AbortController();
    previewController = controller;
    resolvedSources.clear();
    sourceGenerations.clear();
    setPreview({ status: "previewing" });
    try {
      const canonical = await canonicalizeDraft(snapshot);
      if (
        requestGeneration !== previewGeneration || controller.signal.aborted
      ) return;
      const response = await compositionApi.preview(
        spaceId(),
        canonical.canonical_yaml,
        {},
        controller.signal,
      );
      if (
        requestGeneration !== previewGeneration || controller.signal.aborted
      ) return;
      if (!response.ok) {
        setPreview({
          status: "diagnostics",
          diagnostics: response.diagnostics,
        });
        return;
      }
      const plan: Pick<
        CompositionPreviewPlan,
        "sources" | "component_bindings"
      > = {
        sources: response.plan.sources,
        component_bindings: response.plan.component_bindings ?? [],
      };
      setPreview({ status: "ready", plan, sources: {} });
      for (const source of plan.sources) {
        loadSourcePage(requestGeneration, source, undefined, [undefined]);
      }
    } catch (error) {
      if (
        requestGeneration !== previewGeneration || controller.signal.aborted ||
        isAbort(error)
      ) return;
      setPreview({ status: "error" });
    }
  };

  const retryPreview = () => {
    if (previewTimer !== undefined) clearTimeout(previewTimer);
    void runPreview(draft());
  };

  createEffect(() => {
    const snapshot = draft();
    if (previewTimer !== undefined) clearTimeout(previewTimer);
    previewTimer = undefined;
    if (
      snapshot.name.trim().length === 0 || snapshot.sources.length === 0
    ) {
      previewGeneration += 1;
      previewController?.abort();
      previewController = undefined;
      setPreview({ status: "idle" });
      return;
    }
    setPreview((current) =>
      current.status === "ready" || current.status === "previewing"
        ? current
        : { status: "previewing" }
    );
    previewTimer = setTimeout(() => {
      previewTimer = undefined;
      void runPreview(snapshot);
    }, PREVIEW_DEBOUNCE_MS);
  });

  const addSeed = (seed: CompositionSourceSeed) => {
    const added = seed.kind === "saved_sql"
      ? addSavedSqlSource(draft(), seed.seed)
      : addEntryQuerySource(draft(), seed.seed);
    setDraft(added.draft);
    setExpandedId(added.draftId);
    setPickerOpen(false);
  };

  const toggleExpanded = (sourceDraftId: string) => {
    setExpandedId((current) =>
      current === sourceDraftId ? null : sourceDraftId
    );
  };

  const moveDraftSource = (sourceDraftId: string, direction: "up" | "down") => {
    const result = moveSource(draft(), sourceDraftId, direction);
    if (result.ok) setDraft(result.draft);
  };

  const removeDraftSource = (sourceDraftId: string) => {
    const result = removeSource(draft(), sourceDraftId);
    if (result.ok) {
      setDraft(result.draft);
      setExpandedId((current) => current === sourceDraftId ? null : current);
    }
  };

  const expandedSource = (): DraftSource | undefined =>
    draft().sources.find((source) => source.draftId === expandedId());

  const saveRequest = async (
    attempt: PendingCompositionSaveAttempt,
    isRetry: boolean,
  ) => {
    // In-memory attempt only: never persisted to browser storage or shared
    // across server requests. The idempotency key stays stable across
    // retries of the same attempt.
    stagePendingCompositionSaveAttempt(attempt, undefined);
    try {
      const response = await compositionApi.save(
        attempt.spaceId,
        attempt.yaml,
        attempt.idempotencyKey,
      );
      if (!isCurrentSaveRoute(attempt)) {
        markPendingCompositionSaveAttemptUncertain(attempt, undefined);
        return;
      }
      clearPendingCompositionSaveAttempt(attempt, "completed", undefined);
      pendingSave = undefined;
      navigate(
        spaceCompositionRevisionPath(
          spaceId(),
          response.composition_id,
          response.revision_id,
        ),
      );
    } catch (error) {
      const outcome = error && typeof error === "object" &&
          "mutationOutcome" in error
        ? (error as { mutationOutcome?: unknown }).mutationOutcome
        : "unknown";
      const status = error && typeof error === "object" && "status" in error
        ? (error as { status?: unknown }).status
        : undefined;
      const retryWasDenied = isRetry && (status === 401 || status === 403);
      if (outcome === "rejected" && !retryWasDenied) {
        clearPendingCompositionSaveAttempt(attempt, "rejected", undefined);
        setSaveError(compositionSaveErrorMessage(error));
      } else {
        markPendingCompositionSaveAttemptUncertain(attempt, undefined);
        setSaveError(t("composition.saveFailed"));
        setSaveRetryAvailable(true);
      }
    }
  };

  const handleSave = async () => {
    if (!canSave()) return;
    const snapshot = draft();
    pendingSave = undefined;
    setSaveError(null);
    setSaveRetryAvailable(false);
    setSaving(true);
    try {
      const canonical = await canonicalizeDraft(snapshot);
      const attempt: PendingCompositionSaveAttempt = {
        ...saveRoute(),
        name: snapshot.name.trim(),
        yaml: canonical.canonical_yaml,
        idempotencyKey: crypto.randomUUID(),
      };
      pendingSave = attempt;
      await saveRequest(attempt, false);
    } catch {
      setSaveError(t("composition.saveFailed"));
    } finally {
      setSaving(false);
    }
  };

  const handleRetrySave = async () => {
    if (saving() || !saveRetryAvailable() || !pendingSave) return;
    setSaveError(null);
    setSaving(true);
    try {
      await saveRequest(pendingSave, true);
    } finally {
      setSaving(false);
    }
  };

  const readyPreview = () => {
    const current = preview();
    return current.status === "ready" ? current : undefined;
  };

  const diagnosticsPreview = () => {
    const current = preview();
    return current.status === "diagnostics" ? current.diagnostics : undefined;
  };

  const dataHeadingId = "studio-data-heading";
  const tagsHeadingId = "studio-tags-heading";
  const previewHeadingId = "studio-preview-heading";
  const nameInputId = "studio-name";

  return (
    <div class="studioNew">
      <h1 class="ui-sr-only">
        {draft().name.trim() || t("composition.name")}
      </h1>
      <header class="flex items-center gap-2">
        <BackLink
          href={spaceCompositionsPath(spaceId())}
          label={t("composition.listHeading")}
        />
        <input
          id={nameInputId}
          class="ui-input"
          aria-label={t("composition.name")}
          value={draft().name}
          onInput={(event) =>
            setDraft(setDraftName(draft(), event.currentTarget.value))}
        />
        <IconButton
          icon="save"
          label={t("composition.save")}
          disabled={!canSave()}
          onClick={() => void handleSave()}
        />
      </header>
      <Show when={saveError()}>
        <p class="ui-text-danger" role="alert">{saveError()}</p>
      </Show>
      <Show when={saveRetryAvailable() && !saving()}>
        <button
          class="ui-button ui-button-secondary"
          type="button"
          onClick={() => void handleRetrySave()}
        >
          {t("composition.retry")}
        </button>
      </Show>

      <section class="section" aria-labelledby={dataHeadingId}>
        <div class="flex flex-wrap items-center justify-between gap-2">
          <h2 id={dataHeadingId}>{t("composition.studioData")}</h2>
          <button
            class="ui-button ui-button-secondary"
            type="button"
            onClick={() => setPickerOpen(true)}
          >
            {t("composition.studioAddData")}
          </button>
        </div>
        <Show
          when={draft().sources.length > 0}
          fallback={<p class="ui-muted">{t("composition.studioEmptyData")}</p>}
        >
          <RowList
            label={t("composition.studioData")}
            labelledBy={dataHeadingId}
          >
            <For each={draft().sources}>
              {(source, index) => (
                <RowListItem
                  main={
                    <RowListButton
                      ariaLabel={source.name}
                      primary={
                        <span class="rowListName">
                          <UiIcon name={sourceKindIcon(source)} />
                          <span>{source.name}</span>
                        </span>
                      }
                      secondary={sourceKindLabel(source)}
                      onActivate={() => toggleExpanded(source.draftId)}
                    />
                  }
                  actions={
                    <>
                      <button
                        type="button"
                        class="pill iconpill icononly"
                        disabled={index() === 0}
                        aria-label={t("composition.studioMoveUp", {
                          name: source.name,
                        })}
                        onClick={() => moveDraftSource(source.draftId, "up")}
                      >
                        <span aria-hidden="true">↑</span>
                      </button>
                      <button
                        type="button"
                        class="pill iconpill icononly"
                        disabled={index() === draft().sources.length - 1}
                        aria-label={t("composition.studioMoveDown", {
                          name: source.name,
                        })}
                        onClick={() => moveDraftSource(source.draftId, "down")}
                      >
                        <span aria-hidden="true">↓</span>
                      </button>
                      <IconButton
                        icon="trash"
                        label={t("composition.studioRemoveSource", {
                          name: source.name,
                        })}
                        onClick={() => removeDraftSource(source.draftId)}
                      />
                    </>
                  }
                />
              )}
            </For>
          </RowList>
        </Show>
        <Show when={expandedSource()}>
          {(source) => (
            <details class="ui-stack-sm" open>
              <summary>{source().name}</summary>
              <div class="ui-muted">
                {source().kind === "saved_sql"
                  ? t("composition.studioRevisionDetail", {
                    revision: (source() as Extract<
                      DraftSource,
                      { kind: "saved_sql" }
                    >).revisionId,
                  })
                  : t("composition.studioFormDetail", {
                    form: (source() as Extract<
                      DraftSource,
                      { kind: "entry_query" }
                    >).formId,
                  })}
              </div>
            </details>
          )}
        </Show>
      </section>

      <section class="section" aria-labelledby={tagsHeadingId}>
        <h2 id={tagsHeadingId}>{t("composition.studioTags")}</h2>
        <input
          class="ui-input"
          aria-label={t("composition.studioTags")}
          value={draft().tags.join(", ")}
          onInput={(event) =>
            setDraft(
              setDraftTags(draft(), parseTags(event.currentTarget.value)),
            )}
        />
      </section>

      <section class="section" aria-labelledby={previewHeadingId}>
        <h2 id={previewHeadingId}>{t("composition.studioPreview")}</h2>
        <Show when={preview().status === "previewing"}>
          <LocalBusyIndicator label={t("composition.queryLoading")} />
        </Show>
        <Show when={preview().status === "error"}>
          <p class="ui-text-danger" role="alert">
            {t("composition.queryFailed")}
          </p>
          <button
            class="ui-button ui-button-secondary"
            type="button"
            onClick={retryPreview}
          >
            {t("composition.retry")}
          </button>
        </Show>
        <Show when={diagnosticsPreview()}>
          {(diagnostics) => (
            <>
              <CompositionDiagnostics diagnostics={diagnostics()} />
              <button
                class="ui-button ui-button-secondary"
                type="button"
                onClick={retryPreview}
              >
                {t("composition.retry")}
              </button>
            </>
          )}
        </Show>
        <Show when={readyPreview()}>
          {(ready) => (
            <Show
              when={ready().plan.component_bindings.length > 0}
              fallback={<p class="ui-muted">{t("composition.queryEmpty")}</p>}
            >
              <CompositionRenderer
                plan={ready().plan}
                sources={ready().sources}
                onNext={(sourceId) => {
                  const source = resolvedSources.get(sourceId);
                  const state = ready().sources[sourceId];
                  const cursor = state?.page?.page.next;
                  if (!source || !cursor || state?.status === "loading") return;
                  void loadSourcePage(
                    previewGeneration,
                    source,
                    cursor,
                    [...state.cursorStack, cursor],
                  );
                }}
                onPrevious={(sourceId) => {
                  const source = resolvedSources.get(sourceId);
                  const state = ready().sources[sourceId];
                  if (
                    !source || !state || state.cursorStack.length <= 1
                  ) return;
                  const cursorStack = state.cursorStack.slice(0, -1);
                  void loadSourcePage(
                    previewGeneration,
                    source,
                    cursorStack.at(-1),
                    cursorStack,
                  );
                }}
                onRetry={(sourceId) => {
                  const source = resolvedSources.get(sourceId);
                  const state = ready().sources[sourceId];
                  if (!source || !state || state.status === "loading") return;
                  void loadSourcePage(
                    previewGeneration,
                    source,
                    state.cursor,
                    state.cursorStack,
                  );
                }}
              />
            </Show>
          )}
        </Show>
      </section>

      <Show when={pickerOpen()}>
        <CompositionSourcePicker
          spaceId={spaceId()}
          onSelect={addSeed}
          onClose={() => setPickerOpen(false)}
        />
      </Show>
    </div>
  );
}
