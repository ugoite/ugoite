import { useLocation, useNavigate } from "@solidjs/router";
import { createEffect, createSignal, For, onCleanup, Show } from "solid-js";
import { BackLink } from "~/components/BackLink";
import {
  CompositionDiagnostics,
  CompositionRenderer,
} from "~/components/CompositionRenderer";
import {
  CompositionDesignCanvas,
  designBlockIdForComponent,
} from "~/components/CompositionDesignCanvas";
import { CompositionDisplayList } from "~/components/CompositionDisplayList";
import {
  CompositionDisplayPicker,
  type CompositionDisplaySeed,
} from "~/components/CompositionDisplayPicker";
import { CompositionParameterList } from "~/components/CompositionParameterList";
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
  addMetricDisplay,
  addParameter,
  addSavedSqlSource,
  addTableDisplay,
  canonicalizeDraft,
  type CompositionDraft,
  createEmptyDraft,
  defaultParameterValues,
  type DraftInsertTarget,
  type DraftParameter,
  type DraftSource,
  ensureParametersForVariables,
  moveDisplay,
  moveSource,
  removeDisplay,
  removeParameter,
  removeSource,
  setDisplayLabel,
  setDraftName,
  setDraftTags,
  upsertParameter,
} from "~/lib/composition-draft";
import { compositionApi } from "~/lib/composition-api";
import { createCompositionPreviewHandle } from "~/lib/composition-preview-handle";
import type { CompositionSourcePageState } from "~/lib/composition-query-handle";
import { compositionSaveErrorMessage } from "~/lib/composition-save-error";
import {
  clearPendingCompositionSaveAttempt,
  markPendingCompositionSaveAttemptUncertain,
  type PendingCompositionSaveAttempt,
  stagePendingCompositionSaveAttempt,
} from "~/lib/composition-save-attempt";
import { t } from "~/lib/i18n";
import { spaceCompositionRevisionPath } from "~/lib/space-path";

export type CompositionStudioSaveMode =
  | { kind: "create" }
  | { kind: "update"; compositionId: string; baseRevisionId: string };

export interface CompositionStudioProps {
  spaceId: string;
  initialDraft?: CompositionDraft;
  saveMode: CompositionStudioSaveMode;
  backHref: string;
  backLabel: string;
}

const PREVIEW_DEBOUNCE_MS = 400;

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

export function CompositionStudio(props: CompositionStudioProps) {
  const location = useLocation();
  const navigate = useNavigate();
  const spaceId = () => props.spaceId;

  const [draft, setDraft] = createSignal<CompositionDraft>(
    props.initialDraft ?? createEmptyDraft(),
  );
  const [pickerOpen, setPickerOpen] = createSignal(false);
  const [displayPickerOpen, setDisplayPickerOpen] = createSignal(false);
  const [expandedId, setExpandedId] = createSignal<string | null>(null);
  // Transient canvas Work: selected block identity for the inspector and
  // the pending palette insertion target for metric/table picks.
  const [selectedId, setSelectedId] = createSignal<string | null>(null);
  const [paletteTarget, setPaletteTarget] = createSignal<
    DraftInsertTarget | null
  >(null);
  const [pendingInsert, setPendingInsert] = createSignal<
    DraftInsertTarget | null
  >(null);
  const [saving, setSaving] = createSignal(false);
  const [saveError, setSaveError] = createSignal<string | null>(null);
  const [saveRetryAvailable, setSaveRetryAvailable] = createSignal(false);

  const previewHandle = createCompositionPreviewHandle();
  onCleanup(previewHandle.dispose);

  let previewTimer: ReturnType<typeof setTimeout> | undefined;

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
    if (previewTimer !== undefined) clearTimeout(previewTimer);
  });

  const schedulePreview = (snapshot: CompositionDraft) => {
    if (previewTimer !== undefined) clearTimeout(previewTimer);
    previewTimer = undefined;
    previewTimer = setTimeout(() => {
      previewTimer = undefined;
      void (async () => {
        try {
          const canonical = await canonicalizeDraft(snapshot);
          await previewHandle.preview(
            spaceId(),
            canonical.canonical_yaml,
            defaultParameterValues(snapshot),
          );
        } catch {
          // canonicalizeDraft rejects only on WASM transport failure;
          // contract diagnostics arrive through the preview response.
        }
      })();
    }, PREVIEW_DEBOUNCE_MS);
  };

  const retryPreview = () => {
    const current = previewHandle.state();
    if (!current.yaml || !current.spaceId) return;
    if (previewTimer !== undefined) clearTimeout(previewTimer);
    previewTimer = undefined;
    void previewHandle.preview(current.spaceId, current.yaml, {
      ...previewHandle.parameters(),
    });
  };

  // Live preview follows the draft with a bounded debounce. Preview needs
  // sources only; the tool name gates saving, never previewing.
  createEffect(() => {
    const snapshot = draft();
    if (snapshot.sources.length === 0) {
      if (previewTimer !== undefined) {
        clearTimeout(previewTimer);
        previewTimer = undefined;
      }
      return;
    }
    schedulePreview(snapshot);
  });

  const addSeed = (seed: CompositionSourceSeed) => {
    const added = seed.kind === "saved_sql"
      ? addSavedSqlSource(draft(), seed.seed)
      : addEntryQuerySource(draft(), seed.seed);
    // Saved SQL variables bind same-named parameters; provision the
    // missing ones from the server-declared types so the draft previews
    // without dangling references.
    const provisioned = seed.kind === "saved_sql" && seed.seed.variableTypes
      ? ensureParametersForVariables(added.draft, seed.seed.variableTypes)
      : added.draft;
    setDraft(provisioned);
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

  const addDisplaySeed = (seed: CompositionDisplaySeed) => {
    // Palette metric/table picks land at the recorded canvas target; the
    // legacy Add display button appends to the last row instead.
    const target = pendingInsert() ?? undefined;
    const added = seed.kind === "table"
      ? addTableDisplay(draft(), seed.sourceDraftId, seed.label, target)
      : addMetricDisplay(
        draft(),
        seed.sourceDraftId,
        seed.valueField,
        seed.label,
        target,
      );
    if (added.ok) {
      setDraft(added.draft);
      if (added.draftId) {
        setSelectedId(designBlockIdForComponent(added.draftId));
      }
    }
    setPendingInsert(null);
    setDisplayPickerOpen(false);
  };

  const moveDraftDisplay = (
    displayDraftId: string,
    direction: "up" | "down",
  ) => {
    const result = moveDisplay(draft(), displayDraftId, direction);
    if (result.ok) setDraft(result.draft);
  };

  const removeDraftDisplay = (displayDraftId: string) => {
    const result = removeDisplay(draft(), displayDraftId);
    if (result.ok) setDraft(result.draft);
  };

  const changeDisplayLabel = (displayDraftId: string, label: string) => {
    const result = setDisplayLabel(draft(), displayDraftId, label);
    if (result.ok) setDraft(result.draft);
  };

  const expandedSource = (): DraftSource | undefined =>
    draft().sources.find((source) => source.draftId === expandedId());

  const addDraftParameter = (parameter: DraftParameter) => {
    const result = addParameter(draft(), parameter);
    if (result.ok) setDraft(result.draft);
  };

  const updateDraftParameter = (parameter: DraftParameter) => {
    const result = upsertParameter(draft(), parameter);
    if (result.ok) setDraft(result.draft);
  };

  const removeDraftParameter = (parameterId: string): string | undefined => {
    const result = removeParameter(draft(), parameterId);
    if (result.ok) {
      setDraft(result.draft);
      return undefined;
    }
    if (result.error === "parameter-referenced") {
      return t("composition.studioParameterReferenced");
    }
    return t("composition.studioCannotRemoveParameter");
  };

  const saveRequest = async (
    attempt: PendingCompositionSaveAttempt,
    isRetry: boolean,
  ) => {
    // In-memory attempt only: never persisted to browser storage or shared
    // across server requests. The idempotency key stays stable across
    // retries of the same attempt.
    stagePendingCompositionSaveAttempt(attempt, undefined);
    try {
      // Create saves keep the original three-argument call; updates carry
      // the exact composition and base revision identity.
      const response = props.saveMode.kind === "update"
        ? await compositionApi.save(
          attempt.spaceId,
          attempt.yaml,
          attempt.idempotencyKey,
          {
            compositionId: props.saveMode.compositionId,
            baseRevisionId: props.saveMode.baseRevisionId,
          },
        )
        : await compositionApi.save(
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

  const previewState = () => previewHandle.state();
  // Preview selectors stay empty while the draft has no sources, so a
  // removed last source never leaves a stale preview on screen.
  const hasSources = () => draft().sources.length > 0;
  const readySources = (): Record<string, CompositionSourcePageState> => {
    const current = previewState();
    return current.preview?.ok && hasSources() ? current.sources : {};
  };
  const readyPlan = () => {
    const current = previewState();
    if (!current.preview?.ok || !hasSources()) return undefined;
    return {
      sources: current.preview.plan?.sources ?? [],
      component_bindings: current.preview.plan?.component_bindings ?? [],
    };
  };
  const previewDiagnostics = () => {
    const current = previewState();
    return !current.preview?.ok && hasSources()
      ? current.preview?.diagnostics
      : undefined;
  };
  // Text blocks render from draft declarations with no source binding, so
  // the canvas fills preview-pending text bindings locally instead of
  // waiting for (or triggering) a second preview path.
  const canvasPlan = () => {
    const plan = readyPlan();
    const bindings = plan?.component_bindings ?? [];
    const known = new Set(
      bindings.map((binding) => binding.component_id),
    );
    const pendingTexts = draft().displays
      .filter((display) =>
        display.kind === "text" && !known.has(display.draftId)
      )
      .map((display) => ({
        component_id: display.draftId,
        kind: "text" as const,
      }));
    return {
      sources: plan?.sources ?? [],
      component_bindings: [...bindings, ...pendingTexts],
    };
  };
  const isPreviewing = () => hasSources() && previewState().previewing;
  const previewFailed = () =>
    hasSources() &&
    previewState().previewError !== undefined &&
    previewState().preview === undefined;

  const dataHeadingId = "studio-data-heading";
  const designHeadingId = "studio-design-heading";
  const displayHeadingId = "studio-display-heading";
  const parametersHeadingId = "studio-parameters-heading";
  const tagsHeadingId = "studio-tags-heading";
  const previewHeadingId = "studio-preview-heading";
  const nameInputId = "studio-name";

  return (
    <div>
      <h1 class="ui-sr-only">
        {draft().name.trim() || t("composition.name")}
      </h1>
      <header class="flex items-center gap-2">
        <BackLink
          href={props.backHref}
          label={props.backLabel}
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

      <section class="section" aria-labelledby={designHeadingId}>
        <h2 id={designHeadingId}>{t("composition.studioDesign")}</h2>
        <CompositionDesignCanvas
          draft={draft()}
          plan={canvasPlan()}
          parameterValues={{
            ...defaultParameterValues(draft()),
            ...previewHandle.parameters(),
          }}
          sources={readySources()}
          selectedId={selectedId()}
          onSelect={setSelectedId}
          onDraftChange={setDraft}
          onRequestDisplayPicker={(target) => {
            setPendingInsert(target);
            setDisplayPickerOpen(true);
          }}
          onParameterChange={(parameterId, value) =>
            previewHandle.setParameter(parameterId, value)}
          onNext={(sourceId) => previewHandle.next(sourceId)}
          onPrevious={(sourceId) => previewHandle.previous(sourceId)}
          onRetry={(sourceId) => previewHandle.retry(sourceId)}
          paletteTarget={paletteTarget()}
          onPaletteTarget={setPaletteTarget}
        />
      </section>

      <section class="section" aria-labelledby={displayHeadingId}>
        <div class="flex flex-wrap items-center justify-between gap-2">
          <h2 id={displayHeadingId}>{t("composition.studioDisplay")}</h2>
          <button
            class="ui-button ui-button-secondary"
            type="button"
            disabled={draft().sources.length === 0}
            onClick={() => {
              setPendingInsert(null);
              setDisplayPickerOpen(true);
            }}
          >
            {t("composition.studioAddDisplay")}
          </button>
        </div>
        <CompositionDisplayList
          sources={draft().sources}
          displays={draft().displays}
          headingId={displayHeadingId}
          onRemove={removeDraftDisplay}
          onMove={moveDraftDisplay}
          onChangeLabel={changeDisplayLabel}
        />
      </section>

      <section class="section" aria-labelledby={parametersHeadingId}>
        <h2 id={parametersHeadingId}>{t("composition.studioParameters")}</h2>
        <CompositionParameterList
          parameters={draft().parameters}
          headingId={parametersHeadingId}
          onAdd={addDraftParameter}
          onUpdate={updateDraftParameter}
          onRemove={removeDraftParameter}
        />
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
        <Show when={isPreviewing()}>
          <LocalBusyIndicator label={t("composition.queryLoading")} />
        </Show>
        <Show when={previewFailed()}>
          <p class="ui-text-danger" role="alert">
            {t("composition.queryFailed")}
          </p>
          <button
            class="ui-button ui-button-secondary"
            type="button"
            aria-label={t("composition.studioRetryPreview")}
            onClick={retryPreview}
          >
            {t("composition.retry")}
          </button>
        </Show>
        <Show when={previewDiagnostics()}>
          {(diagnostics) => (
            <>
              <CompositionDiagnostics diagnostics={diagnostics()} />
              <button
                class="ui-button ui-button-secondary"
                type="button"
                aria-label={t("composition.studioRetryPreview")}
                onClick={retryPreview}
              >
                {t("composition.retry")}
              </button>
            </>
          )}
        </Show>
        <Show when={readyPlan()}>
          {(plan) => (
            <Show
              when={plan().component_bindings.length > 0}
              fallback={<p class="ui-muted">{t("composition.queryEmpty")}</p>}
            >
              <CompositionRenderer
                plan={plan()}
                sources={readySources()}
                onNext={(sourceId) => previewHandle.next(sourceId)}
                onPrevious={(sourceId) => previewHandle.previous(sourceId)}
                onRetry={(sourceId) => previewHandle.retry(sourceId)}
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

      <Show when={displayPickerOpen()}>
        <CompositionDisplayPicker
          sources={draft().sources}
          onAdd={addDisplaySeed}
          onClose={() => {
            setPendingInsert(null);
            setDisplayPickerOpen(false);
          }}
        />
      </Show>
    </div>
  );
}
