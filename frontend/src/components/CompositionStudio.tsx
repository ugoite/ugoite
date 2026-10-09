import { useLocation, useNavigate } from "@solidjs/router";
import {
  createEffect,
  createResource,
  createSignal,
  For,
  onCleanup,
  onMount,
  Show,
} from "solid-js";
import { BackLink } from "~/components/BackLink";
import {
  CompositionDiagnostics,
  type CompositionFieldNames,
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
import { CompositionInspectorSheet } from "~/components/CompositionInspectorSheet";
import {
  CompositionDisplayPicker,
  type CompositionDisplaySeed,
} from "~/components/CompositionDisplayPicker";
import { CompositionParameterList } from "~/components/CompositionParameterList";
import {
  CompositionSourcePicker,
  type CompositionSourceSeed,
} from "~/components/CompositionSourcePicker";
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
  type DraftSaveBlockedReason,
  draftSaveReadiness,
  ensureParametersForVariables,
  moveSource,
  removeParameter,
  removeSource,
  type SavedSqlRevisionUpdate,
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
  STUDIO_SHEET_GATE_MEDIA,
  STUDIO_SPLIT_GATE_MEDIA,
  type StudioMode,
  visibleComponentSourceIds,
} from "~/lib/composition-studio-sync";
import type { CompositionSourcePageState } from "~/lib/composition-query-handle";
import {
  compositionCanonicalizeErrorMessage,
  compositionSaveErrorMessage,
} from "~/lib/composition-save-error";
import {
  clearPendingCompositionSaveAttempt,
  markPendingCompositionSaveAttemptUncertain,
  type PendingCompositionSaveAttempt,
  stagePendingCompositionSaveAttempt,
} from "~/lib/composition-save-attempt";
import { t } from "~/lib/i18n";
import { compositionFormFieldName } from "~/lib/composition-field-name";
import { spaceCompositionRevisionPath } from "~/lib/space-path";
import { formApi } from "~/lib/ugoite-client";

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
  const [displayPickerKind, setDisplayPickerKind] = createSignal<
    "table" | "metric"
  >("table");
  const [displayPickerSourceId, setDisplayPickerSourceId] = createSignal<
    string | null
  >(null);
  const [
    displayPickerAutoAddSingleCandidate,
    setDisplayPickerAutoAddSingleCandidate,
  ] = createSignal(false);
  const [sourcePickerKind, setSourcePickerKind] = createSignal<
    "table" | "metric" | null
  >(null);
  const [dataPanel, setDataPanel] = createSignal<
    "sources" | "parameters" | "tags"
  >("sources");
  const [expandedId, setExpandedId] = createSignal<string | null>(null);
  // Transient canvas Work: selected block identity for the inspector and
  // the pending insertion target for the display picker.
  const [selectedId, setSelectedId] = createSignal<string | null>(null);
  // Transient workspace Work: Design | Data | Split arrangement only. Draft,
  // preview, and selection state stay shared and continuous across modes;
  // Split pairs the canvas with the Data pane and is never persisted.
  const [mode, setMode] = createSignal<StudioMode>("design");
  // Transient viewport Work: narrow flags mirror the Studio CSS gates so no
  // JS path can leave a narrow viewport stuck in Split or rendering the
  // wrong inspector container. `sheetOpen` tracks the bottom-sheet dismissal
  // only; the selection owner stays `selectedId`.
  const [belowSplitGate, setBelowSplitGate] = createSignal(false);
  const [sheetViewport, setSheetViewport] = createSignal(false);
  const [sheetOpen, setSheetOpen] = createSignal(false);

  // RA9 mobile: the Split option hides below the Split gate in CSS, and the
  // same gate is enforced here so resizing into a narrow viewport while in
  // Split returns to Design instead of keeping a stacked Split. The sheet
  // gate mirrors the inspector stacking breakpoint: below it the Design
  // inspector renders as a bottom sheet. Both flags stay viewport-derived
  // transient Work and never persist.
  onMount(() => {
    if (
      typeof window === "undefined" ||
      typeof window.matchMedia !== "function"
    ) {
      return;
    }
    const splitQuery = window.matchMedia(STUDIO_SPLIT_GATE_MEDIA);
    const sheetQuery = window.matchMedia(STUDIO_SHEET_GATE_MEDIA);
    const update = () => {
      setBelowSplitGate(splitQuery.matches);
      setSheetViewport(sheetQuery.matches);
    };
    update();
    if (typeof splitQuery.addEventListener === "function") {
      splitQuery.addEventListener("change", update);
      sheetQuery.addEventListener("change", update);
      onCleanup(() => {
        splitQuery.removeEventListener("change", update);
        sheetQuery.removeEventListener("change", update);
      });
    } else {
      splitQuery.addListener(update);
      sheetQuery.addListener(update);
      onCleanup(() => {
        splitQuery.removeListener(update);
        sheetQuery.removeListener(update);
      });
    }
  });

  createEffect(() => {
    if (belowSplitGate() && mode() === "split") setMode("design");
  });

  // A dismissed or viewport-orphaned sheet returns focus to the invoking
  // block's select control. Block identities are compared literally so
  // parameter identities containing a colon need no selector escaping.
  const focusBlockSelect = (blockId: string | null): boolean => {
    if (!blockId || typeof document === "undefined") return false;
    const blocks = document.querySelectorAll("[data-block-id]");
    for (const block of blocks) {
      if (block.getAttribute("data-block-id") !== blockId) continue;
      const control = block.querySelector("button");
      if (control instanceof HTMLElement) {
        control.focus();
        return true;
      }
    }
    return false;
  };

  const closeSheetKeepSelection = () => {
    if (!sheetOpen()) return;
    const invoking = selectedId();
    setSheetOpen(false);
    // Focus after disposal: the sheet lifts `inert` in its own cleanup, so
    // returning focus in a microtask never races the inert removal.
    queueMicrotask(() => focusBlockSelect(invoking));
  };

  // User dismissal (Escape, backdrop, Close) clears the transient selection
  // too, so reopening the inspector for the same block stays a single tap.
  // The Data selection is kept: only the canvas selection clears.
  const dismissSheet = () => {
    if (!sheetOpen()) return;
    const invoking = selectedId();
    setSheetOpen(false);
    setSelectedId(null);
    queueMicrotask(() => focusBlockSelect(invoking));
  };

  // Leaving the sheet viewport unmounts the sheet; an orphaned open sheet
  // returns focus to its invoking block, which stays selected inline.
  createEffect(() => {
    if (!sheetViewport() && sheetOpen()) closeSheetKeepSelection();
  });
  const [paletteTarget, setPaletteTarget] = createSignal<
    DraftInsertTarget | null
  >(null);
  const [pendingInsert, setPendingInsert] = createSignal<
    DraftInsertTarget | null
  >(null);
  const [saving, setSaving] = createSignal(false);
  const [saveError, setSaveError] = createSignal<string | null>(null);
  const [saveRetryAvailable, setSaveRetryAvailable] = createSignal(false);

  // Form labels are display metadata only. Composition fields keep their
  // stable field IDs, while the editor shows the authorized Form labels.
  const [forms] = createResource(
    () =>
      draft().sources.some((source) => source.kind === "entry_query")
        ? props.spaceId
        : undefined,
    (spaceId) => formApi.list(spaceId).catch(() => []),
  );
  const fieldNames: CompositionFieldNames = (formId, fieldId) => {
    return compositionFormFieldName(forms(), formId, fieldId);
  };
  const fieldProjectable = (formId: string, fieldId: number) => {
    const form = forms()?.find((entry) => entry.id === formId);
    if (!form) return undefined;
    const field = Object.values(form.fields ?? {}).find((entry) =>
      (entry.query_capability?.field.field_id ?? entry.id) === fieldId
    );
    return field?.query_capability?.projectable ?? true;
  };

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
  // Save readiness mirrors the shared canonical contract: a non-empty name,
  // valid refs with exactly-once placement and no dangling refs, and at
  // least one layout item. A source-only draft reports `layout` and cannot
  // save. Canonical parsing itself stays async in the save handler below.
  const readiness = () => draftSaveReadiness(draft());
  const canSave = () => readiness().ready && !saving();
  const saveBlockedReason = (): DraftSaveBlockedReason | undefined =>
    readiness().reason;
  // A disabled Save carries its reason in the accessible name and title,
  // mirroring the EntryQuery seed-navigation button pattern. No prose
  // paragraphs explain steady-state blocks.
  const saveReasonText = (): string | undefined => {
    switch (saveBlockedReason()) {
      case "name":
        return t("composition.studioSaveNeedsName");
      case "sources":
        return t("composition.studioSaveNeedsData");
      case "layout":
        return t("composition.studioSaveNeedsBlock");
      case "refs":
        return t("composition.studioSaveNeedsRefs");
      default:
        return undefined;
    }
  };
  const saveLabel = (): string => {
    const reason = canSave() ? undefined : saveReasonText();
    return reason
      ? `${t("composition.save")}, ${reason}`
      : t("composition.save");
  };

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

  const addSourceSeed = (seed: CompositionSourceSeed) => {
    const added = seed.kind === "saved_sql"
      ? addSavedSqlSource(draft(), seed.seed)
      : addEntryQuerySource(draft(), seed.seed);
    // Saved SQL variables bind same-named parameters; provision the
    // missing ones from the server-declared types so the draft previews
    // without dangling references.
    const provisioned = seed.kind === "saved_sql" && seed.seed.variableTypes
      ? ensureParametersForVariables(added.draft, seed.seed.variableTypes)
      : added.draft;
    return { draft: provisioned, draftId: added.draftId };
  };

  const addSeed = (seed: CompositionSourceSeed) => {
    const { draft: provisioned, draftId } = addSourceSeed(seed);
    const table = addTableDisplay(provisioned, draftId);
    if (!table.ok || !table.draftId) return;
    setDraft(table.draft);
    setExpandedId(draftId);
    setSelectedId(designBlockIdForComponent(table.draftId));
    if (sheetViewport()) setSheetOpen(true);
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
    // The display picker survives only as the canvas insertion delegate and
    // lands at the recorded canvas target.
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
        // The fresh block owns the inspector, matching canvas insertion:
        // on narrow viewports that means opening the bottom sheet.
        if (sheetViewport()) setSheetOpen(true);
      }
    }
    setPendingInsert(null);
    setDisplayPickerOpen(false);
    setDisplayPickerSourceId(null);
    setDisplayPickerAutoAddSingleCandidate(false);
  };

  const chooseSourceForDisplay = (kind: "table" | "metric") => {
    setSourcePickerKind(kind);
    setDisplayPickerKind(kind);
    setDisplayPickerOpen(false);
    setPickerOpen(true);
  };

  const addSourceForDisplay = (seed: CompositionSourceSeed) => {
    const kind = sourcePickerKind();
    if (!kind) {
      addSeed(seed);
      return;
    }
    const { draft: sourceDraft, draftId } = addSourceSeed(seed);
    if (kind === "table") {
      const table = addTableDisplay(
        sourceDraft,
        draftId,
        undefined,
        pendingInsert() ?? undefined,
      );
      if (!table.ok || !table.draftId) return;
      setDraft(table.draft);
      setExpandedId(draftId);
      setSelectedId(designBlockIdForComponent(table.draftId));
      if (sheetViewport()) setSheetOpen(true);
      setPendingInsert(null);
      setPickerOpen(false);
      setSourcePickerKind(null);
      setDisplayPickerAutoAddSingleCandidate(false);
      return;
    }
    // A metric needs a specific scalar field. Add the selected source first,
    // then return to the metric picker with that source selected so the next
    // action chooses its human-named field.
    setDraft(sourceDraft);
    setDisplayPickerSourceId(draftId);
    setDisplayPickerAutoAddSingleCandidate(true);
    setPickerOpen(false);
    setSourcePickerKind(null);
    setDisplayPickerKind("metric");
    setDisplayPickerOpen(true);
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
    // From the narrow bottom sheet the jump target is not rendered behind
    // the sheet: dismiss the sheet and show the Data workspace, then focus
    // the jumped row once it mounts.
    if (sheetViewport() && sheetOpen()) {
      setSheetOpen(false);
      setSelectedId(null);
      setMode("data");
      queueMicrotask(() => focusSourceRow(jump.sourceDraftId));
      return;
    }
    focusSourceRow(jump.sourceDraftId);
  };

  // Design block selection owns the inspector. Metric/table blocks also
  // auto-select their source in the Data workspace, reusing the RA5 jump
  // handoff (expand + scroll + focus only where the target row is
  // rendered; in Split the right pane shows it directly). Text blocks and
  // parameter controls carry no source, so the Data selection is kept and
  // never invented. Selection changes never refetch the preview. On narrow
  // viewports selecting a block opens the inspector bottom sheet; clearing
  // the selection dismisses it.
  const handleSelectBlock = (blockId: string | null) => {
    setSelectedId(blockId);
    if (!blockId) {
      setSheetOpen(false);
    } else if (sheetViewport()) {
      setSheetOpen(true);
    }
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
    // Fail closed without a save attempt when the readiness gate blocks:
    // surface the allowlisted invalid-composition summary for structural
    // blocks (empty layout, dangling refs) instead of a generic crash. The
    // idempotency-key and retry-same-payload semantics below stay untouched.
    if (!canSave()) {
      if (!saving() && !readiness().ready) {
        setSaveError(t("composition.diagnostic.invalid_composition"));
      }
      return;
    }
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
    } catch (error) {
      // Canonicalize failures carry shared-contract diagnostics (e.g. an
      // empty layout reaching canonicalization); allowlisted codes resolve
      // to the localized summary, anything else to the generic failure.
      setSaveError(compositionCanonicalizeErrorMessage(error));
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
  createEffect(() => {
    if (!hasSources() && dataPanel() === "parameters") {
      setDataPanel("sources");
    }
  });
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
  const parametersHeadingId = "studio-parameters-heading";
  const tagsHeadingId = "studio-tags-heading";
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
  // The bottom sheet owns the selected block identity on narrow viewports;
  // clearing the selection (or leaving the sheet viewport) dismisses it.
  const sheetSelection = (): string | null =>
    sheetViewport() && sheetOpen() ? selectedId() : null;
  const renderDataWorkspace = (headingId = dataHeadingId) => (
    <CompositionDataWorkspace
      spaceId={spaceId()}
      draft={draft()}
      headingId={headingId}
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
      previewActive={isPreviewing()}
      onNext={(sourceId) => previewHandle.next(sourceId)}
      onPrevious={(sourceId) => previewHandle.previous(sourceId)}
      onRetry={(sourceId) => previewHandle.retry(sourceId)}
      registerSourceRow={registerSourceRow}
    />
  );
  // The diagnostics strip replaces the removed Preview section: resolve
  // and preview diagnostics render through the shared CompositionDiagnostics
  // (structural alert roles, existing copy only) above the mode content in
  // every mode. The Design canvas already previews blocks inline through the
  // shared debounced handle, so no second preview path remains.
  const renderDiagnosticsStrip = () => (
    <>
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
    </>
  );
  const renderDesignWorkspace = () => (
    <div class="studioDesignWorkspace">
      <div class="studioDesign">
        <CompositionDesignCanvas
          draft={draft()}
          plan={canvasPlan()}
          parameterValues={{
            ...defaultParameterValues(draft()),
            ...previewHandle.parameters(),
          }}
          sources={readySources()}
          fieldNames={fieldNames}
          selectedId={selectedId()}
          highlightedIds={highlightedBlockIds()}
          onSelect={handleSelectBlock}
          onDraftChange={setDraft}
          onRequestDisplayPicker={(target) => {
            setPendingInsert(target);
            setDisplayPickerKind("table");
            setDisplayPickerSourceId(null);
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
        {
          /* The inspector renders exactly once: inline beside the canvas on
            wide viewports, or as a bottom sheet on narrow ones. */
        }
        <Show when={!sheetViewport()}>
          <CompositionInspector
            draft={draft()}
            selectedId={selectedId()}
            fieldNames={fieldNames}
            fieldProjectable={fieldProjectable}
            onDraftChange={setDraft}
            onDataJump={jumpToSource}
          />
        </Show>
      </div>
      <Show when={sheetSelection()}>
        {(activeId) => (
          <CompositionInspectorSheet
            draft={draft()}
            selectedId={activeId()}
            fieldNames={fieldNames}
            fieldProjectable={fieldProjectable}
            onDraftChange={setDraft}
            onDataJump={jumpToSource}
            onClose={dismissSheet}
          />
        )}
      </Show>
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
        {/* Visible text Save: the icon-button hid the primary mutation. */}
        <button
          class="ui-button"
          type="button"
          aria-label={saveLabel()}
          title={canSave() ? undefined : saveLabel()}
          disabled={!canSave()}
          onClick={() => void handleSave()}
        >
          {t("composition.save")}
        </button>
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

      {renderDiagnosticsStrip()}

      <Show when={mode() === "data"}>
        <section class="section" aria-labelledby={dataHeadingId}>
          <h2 id={dataHeadingId}>{t("composition.studioData")}</h2>
          <div
            class="tabs studioDataTabs"
            role="tablist"
            aria-labelledby={dataHeadingId}
          >
            <button
              type="button"
              id="studio-data-sources-tab"
              role="tab"
              aria-selected={dataPanel() === "sources"}
              aria-controls="studio-data-sources-panel"
              class="tab"
              classList={{ active: dataPanel() === "sources" }}
              onClick={() => setDataPanel("sources")}
            >
              {t("composition.studioSources")}
            </button>
            <Show when={hasSources()}>
              <button
                type="button"
                id="studio-data-parameters-tab"
                role="tab"
                aria-selected={dataPanel() === "parameters"}
                aria-controls="studio-data-parameters-panel"
                class="tab"
                classList={{ active: dataPanel() === "parameters" }}
                onClick={() => setDataPanel("parameters")}
              >
                {t("composition.studioParameters")}
              </button>
            </Show>
            <button
              type="button"
              id="studio-data-tags-tab"
              role="tab"
              aria-selected={dataPanel() === "tags"}
              aria-controls="studio-data-tags-panel"
              class="tab"
              classList={{ active: dataPanel() === "tags" }}
              onClick={() => setDataPanel("tags")}
            >
              {t("composition.studioTags")}
            </button>
          </div>
          <div
            id="studio-data-sources-panel"
            role="tabpanel"
            aria-labelledby="studio-data-sources-tab"
            hidden={dataPanel() !== "sources"}
          >
            {renderDataWorkspace("studio-data-sources-tab")}
          </div>
          <Show when={hasSources()}>
            <section
              id="studio-data-parameters-panel"
              role="tabpanel"
              aria-labelledby="studio-data-parameters-tab"
              hidden={dataPanel() !== "parameters"}
              class="section"
            >
              <h2 id={parametersHeadingId}>
                {t("composition.studioParameters")}
              </h2>
              <CompositionParameterList
                parameters={draft().parameters}
                headingId={parametersHeadingId}
                onAdd={addDraftParameter}
                onUpdate={updateDraftParameter}
                onRemove={removeDraftParameter}
              />
            </section>
          </Show>
          <section
            id="studio-data-tags-panel"
            role="tabpanel"
            aria-labelledby="studio-data-tags-tab"
            hidden={dataPanel() !== "tags"}
            class="section"
          >
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
              <h2 id={dataHeadingId}>{t("composition.studioData")}</h2>
              {renderDataWorkspace()}
            </div>
          </div>
        </section>
      </Show>

      {
        /* The legacy Display list lived here through RA7; the Design canvas
          plus inspector own display add/remove/reorder/label now, and the
          display picker survives only as the canvas insertion delegate. The
          legacy Preview section is gone too: the canvas previews blocks
          inline through the shared debounced handle, per-source results page
          inside the Data workspace, and resolve diagnostics render in the
          strip above. Parameters and Tags edit in Data mode only. */
      }

      <Show when={pickerOpen()}>
        <CompositionSourcePicker
          spaceId={spaceId()}
          onSelect={addSourceForDisplay}
          onClose={() => {
            setPickerOpen(false);
            setSourcePickerKind(null);
            setDisplayPickerAutoAddSingleCandidate(false);
          }}
        />
      </Show>

      <Show when={displayPickerOpen()}>
        <CompositionDisplayPicker
          sources={draft().sources}
          fieldNames={fieldNames}
          fieldProjectable={fieldProjectable}
          fieldNamesLoading={forms.loading}
          initialKind={displayPickerKind()}
          initialSourceDraftId={displayPickerSourceId()}
          autoAddSingleCandidate={displayPickerAutoAddSingleCandidate()}
          onAdd={addDisplaySeed}
          onChooseSource={chooseSourceForDisplay}
          onClose={() => {
            setPendingInsert(null);
            setDisplayPickerSourceId(null);
            setDisplayPickerOpen(false);
            setDisplayPickerAutoAddSingleCandidate(false);
          }}
        />
      </Show>
    </div>
  );
}
