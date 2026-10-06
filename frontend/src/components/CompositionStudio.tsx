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
import { CompositionDataWorkspace } from "~/components/CompositionDataWorkspace";
import {
  CompositionInspector,
  type CompositionInspectorDataJump,
} from "~/components/CompositionInspector";
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
  ensureParametersForVariables,
  moveDisplay,
  moveSource,
  removeDisplay,
  removeParameter,
  removeSource,
  type SavedSqlRevisionUpdate,
  setDisplayLabel,
  setDraftName,
  setDraftTags,
  setEntryQueryFilters,
  setEntryQueryProjection,
  setEntryQuerySort,
  setSavedSqlRevision,
  upsertParameter,
} from "~/lib/composition-draft";
import {
  compositionApi,
  type CompositionParameterType,
} from "~/lib/composition-api";
import type {
  EntryQueryCompositionFilter,
  EntryQueryCompositionProjection,
  EntryQueryCompositionSort,
} from "~/lib/entry-query-composition";
import { createCompositionPreviewHandle } from "~/lib/composition-preview-handle";
import {
  blockIdsUsingSource,
  sourceDraftIdForBlock,
  STUDIO_MODES,
  type StudioMode,
  visibleComponentSourceIds,
} from "~/lib/composition-studio-sync";
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
  // Transient workspace Work: Design | Data | Split arrangement only. Draft,
  // preview, and selection state stay shared and continuous across modes;
  // Split pairs the canvas with the Data pane and is never persisted.
  const [mode, setMode] = createSignal<StudioMode>("design");
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
          // Scope resolves against the latest draft at fire time so block,
          // source, and mode changes during an in-flight preview never
          // resurrect a stale fetch set; the generation guard drops the
          // stale response itself.
          await previewHandle.preview(
            spaceId(),
            canonical.canonical_yaml,
            defaultParameterValues(snapshot),
            {
              visibleSourceIds: visibleComponentSourceIds(draft()),
              selectedSourceId: expandedId(),
            },
          );
        } catch {
          // canonicalizeDraft rejects only on WASM transport failure;
          // contract diagnostics arrive through the preview response.
        }
      })();
    }, PREVIEW_DEBOUNCE_MS);
  };

  const previewScope = () => ({
    visibleSourceIds: visibleComponentSourceIds(draft()),
    selectedSourceId: expandedId(),
  });

  const retryPreview = () => {
    const current = previewHandle.state();
    if (!current.yaml || !current.spaceId) return;
    if (previewTimer !== undefined) clearTimeout(previewTimer);
    previewTimer = undefined;
    void previewHandle.preview(current.spaceId, current.yaml, {
      ...previewHandle.parameters(),
    }, previewScope());
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
    const selecting = expandedId() !== sourceDraftId;
    setExpandedId(selecting ? sourceDraftId : null);
    // Selecting a source outside the scoped fetch set pages it on demand
    // through the existing path; deselecting never refetches.
    if (selecting) previewHandle.ensureSource(sourceDraftId);
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
    if (result.ok) {
      setDraft(result.draft);
      // Clearing keeps the inspector from pointing at a removed block;
      // with no resolvable selection it renders nothing.
      if (selectedId() === designBlockIdForComponent(displayDraftId)) {
        setSelectedId(null);
      }
    }
  };

  const changeDisplayLabel = (displayDraftId: string, label: string) => {
    const result = setDisplayLabel(draft(), displayDraftId, label);
    if (result.ok) setDraft(result.draft);
  };

  // Data workspace edits flow through narrow draft updaters into the shared
  // debounced preview. Each returns whether the edit was accepted so the
  // editor can surface a rejection without inventing query grammar.
  const updateEntryQueryFilters = (
    sourceDraftId: string,
    filters: EntryQueryCompositionFilter[],
  ): boolean => {
    const result = setEntryQueryFilters(draft(), sourceDraftId, filters);
    if (result.ok) setDraft(result.draft);
    return result.ok;
  };

  const updateEntryQuerySort = (
    sourceDraftId: string,
    sort: EntryQueryCompositionSort[],
  ): boolean => {
    const result = setEntryQuerySort(draft(), sourceDraftId, sort);
    if (result.ok) setDraft(result.draft);
    return result.ok;
  };

  const updateEntryQueryProjection = (
    sourceDraftId: string,
    projection: EntryQueryCompositionProjection,
  ): boolean => {
    const result = setEntryQueryProjection(draft(), sourceDraftId, projection);
    if (result.ok) setDraft(result.draft);
    return result.ok;
  };

  // Exact-revision update after the Saved SQL editor publishes a new
  // revision. Missing parameters are provisioned from the server-declared
  // variable types, mirroring the add-source seed path; the composition
  // save itself stays a separate explicit action.
  const updateSavedSqlRevision = (
    sourceDraftId: string,
    revision: SavedSqlRevisionUpdate,
    variableTypes: Record<string, CompositionParameterType>,
  ): boolean => {
    const result = setSavedSqlRevision(draft(), sourceDraftId, revision);
    if (!result.ok) return false;
    setDraft(ensureParametersForVariables(result.draft, variableTypes));
    return true;
  };

  const savedSqlEditHref = (entryId: string) =>
    `/spaces/${encodeURIComponent(spaceId())}/sql/${
      encodeURIComponent(entryId)
    }`;

  // Inspector data jump: select the block's source in the Data workspace
  // navigator, scroll it into view, and focus its activation control. The
  // navigator row stays the jump target behind the workspace editors; RA7
  // split sync reuses the same typed jump payload.
  const sourceRowEls = new Map<string, HTMLDivElement>();
  const registerSourceRow = (
    sourceDraftId: string,
    el: HTMLDivElement | null,
  ) => {
    if (el) sourceRowEls.set(sourceDraftId, el);
    else sourceRowEls.delete(sourceDraftId);
  };
  const focusSourceRow = (sourceDraftId: string) => {
    const row = sourceRowEls.get(sourceDraftId);
    if (!row || !row.isConnected) return;
    if (typeof row.scrollIntoView === "function") {
      try {
        row.scrollIntoView({ block: "nearest" });
      } catch {
        // Keep the expanded selection when scrolling is unavailable.
      }
    }
    row.querySelector("button")?.focus();
  };
  const jumpToSource = (jump: CompositionInspectorDataJump) => {
    setExpandedId(jump.sourceDraftId);
    previewHandle.ensureSource(jump.sourceDraftId);
    focusSourceRow(jump.sourceDraftId);
  };

  // Design block selection owns the inspector. Metric/table blocks also
  // auto-select their source in the Data workspace, reusing the RA5 jump
  // handoff (expand + scroll + focus only where the target row is
  // rendered; in Split the right pane shows it directly). Text blocks and
  // parameter controls carry no source, so the Data selection is kept and
  // never invented. Selection changes never refetch the preview.
  const handleSelectBlock = (blockId: string | null) => {
    setSelectedId(blockId);
    const sourceDraftId = sourceDraftIdForBlock(draft(), blockId);
    if (!sourceDraftId) return;
    setExpandedId(sourceDraftId);
    previewHandle.ensureSource(sourceDraftId);
    focusSourceRow(sourceDraftId);
  };

  // Data source selection soft-highlights the Design blocks that use it.
  // Highlight is visual only; the selection owner stays selectedId.
  const highlightedBlockIds = () =>
    new Set(blockIdsUsingSource(draft(), expandedId()));

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

  const modeIcon = (entry: StudioMode): "canvas-table" | "sql" | "columns" =>
    entry === "design" ? "canvas-table" : entry === "data" ? "sql" : "columns";
  const modeLabel = (entry: StudioMode): string =>
    entry === "design"
      ? t("composition.studioDesign")
      : entry === "data"
      ? t("composition.studioData")
      : t("composition.studioSplit");

  // Shared workspace fragments: modes switch the arrangement only, so the
  // Data workspace and the Design canvas render from the same draft,
  // preview, and selection state in every mode without refetching.
  const renderDataWorkspace = () => (
    <CompositionDataWorkspace
      spaceId={spaceId()}
      draft={draft()}
      headingId={dataHeadingId}
      selectedSourceId={expandedId()}
      onSelectSource={toggleExpanded}
      onMoveSource={moveDraftSource}
      onRemoveSource={removeDraftSource}
      onEntryQueryFilters={updateEntryQueryFilters}
      onEntryQuerySort={updateEntryQuerySort}
      onEntryQueryProjection={updateEntryQueryProjection}
      onSavedSqlRevision={updateSavedSqlRevision}
      savedSqlEditHref={savedSqlEditHref}
      planSources={readyPlan()?.sources ?? []}
      sourceStates={readySources()}
      diagnostics={previewDiagnostics() ?? []}
      onNext={(sourceId) => previewHandle.next(sourceId)}
      onPrevious={(sourceId) => previewHandle.previous(sourceId)}
      onRetry={(sourceId) => previewHandle.retry(sourceId)}
      registerSourceRow={registerSourceRow}
    />
  );
  const renderDesignWorkspace = () => (
    <div class="studioDesign">
      <CompositionDesignCanvas
        draft={draft()}
        plan={canvasPlan()}
        parameterValues={{
          ...defaultParameterValues(draft()),
          ...previewHandle.parameters(),
        }}
        sources={readySources()}
        selectedId={selectedId()}
        highlightedIds={highlightedBlockIds()}
        onSelect={handleSelectBlock}
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
      <CompositionInspector
        draft={draft()}
        selectedId={selectedId()}
        onDraftChange={setDraft}
        onDataJump={jumpToSource}
      />
    </div>
  );

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
      <div
        class="studioMode"
        role="radiogroup"
        aria-label={t("composition.studioMode")}
      >
        <For each={STUDIO_MODES}>
          {(entry) => (
            <button
              type="button"
              role="radio"
              aria-checked={mode() === entry}
              class="studioModeOption"
              classList={{
                "studioModeOption--active": mode() === entry,
                studioModeSplit: entry === "split",
              }}
              onClick={() => setMode(entry)}
            >
              <UiIcon name={modeIcon(entry)} />
              <span>{modeLabel(entry)}</span>
            </button>
          )}
        </For>
      </div>
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

      <Show when={mode() === "data"}>
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
          {renderDataWorkspace()}
        </section>
      </Show>

      <Show when={mode() === "design"}>
        <section class="section" aria-labelledby={designHeadingId}>
          <h2 id={designHeadingId}>{t("composition.studioDesign")}</h2>
          {renderDesignWorkspace()}
        </section>
      </Show>

      <Show when={mode() === "split"}>
        <section class="section" aria-label={t("composition.studioMode")}>
          <div class="studioSplit">
            <div aria-labelledby={designHeadingId}>
              <h2 id={designHeadingId}>{t("composition.studioDesign")}</h2>
              {renderDesignWorkspace()}
            </div>
            <div class="studioSplitPane" aria-labelledby={dataHeadingId}>
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
              {renderDataWorkspace()}
            </div>
          </div>
        </section>
      </Show>

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
