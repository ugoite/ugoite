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
import {
  spaceCompositionRevisionPath,
  spaceCompositionsPath,
} from "~/lib/space-path";
import { spaceRoute } from "~/lib/space-shell-route";

export const route = spaceRoute({ navigation: "home" });

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

export default function CompositionNewRoute() {
  const params = useParams<{ space_id: string }>();
  const location = useLocation();
  const navigate = useNavigate();
  const spaceId = () => params.space_id;

  const [draft, setDraft] = createSignal<CompositionDraft>(createEmptyDraft());
  const [pickerOpen, setPickerOpen] = createSignal(false);
  const [expandedId, setExpandedId] = createSignal<string | null>(null);
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
          await previewHandle.preview(spaceId(), canonical.canonical_yaml, {});
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
  const isPreviewing = () => hasSources() && previewState().previewing;
  const previewFailed = () =>
    hasSources() &&
    previewState().previewError !== undefined &&
    previewState().preview === undefined;

  const dataHeadingId = "studio-data-heading";
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
    </div>
  );
}
