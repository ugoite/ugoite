import { createSignal, Show } from "solid-js";
import { UiIcon } from "~/components/UiIcon";
import { SourceNavigator } from "~/components/SourceNavigator";
import {
  type EntryQuerySource,
  EntryQuerySourceEditor,
} from "~/components/EntryQuerySourceEditor";
import {
  type SavedSqlSource,
  SavedSqlSourceViewer,
} from "~/components/SavedSqlSourceViewer";
import type {
  CompositionResolveDiagnostic,
  CompositionResolvedSource,
} from "~/lib/composition-api";
import type { CompositionParameterType } from "~/lib/composition-api";
import type {
  CompositionDraft,
  SavedSqlRevisionUpdate,
} from "~/lib/composition-draft";
import type {
  EntryQueryCompositionFilter,
  EntryQueryCompositionProjection,
  EntryQueryCompositionSort,
  EntryQueryCompositionSystemField,
} from "~/lib/entry-query-composition";
import type { CompositionSourcePageState } from "~/lib/composition-query-handle";
import { t } from "~/lib/i18n";

export interface CompositionDataWorkspaceProps {
  spaceId: string;
  draft: CompositionDraft;
  headingId: string;
  /** Selected source draft ID; the editor area renders its detail. */
  selectedSourceId: string | null;
  onSelectSource: (sourceDraftId: string) => void;
  onMoveSource: (sourceDraftId: string, direction: "up" | "down") => void;
  onRemoveSource: (sourceDraftId: string) => void;
  onEntryQueryFilters: (
    sourceDraftId: string,
    filters: EntryQueryCompositionFilter[],
  ) => boolean;
  onEntryQuerySort: (
    sourceDraftId: string,
    sort: EntryQueryCompositionSort[],
  ) => boolean;
  onEntryQueryProjection: (
    sourceDraftId: string,
    projection: EntryQueryCompositionProjection,
  ) => boolean;
  onEntryQueryDisplaySystemFields: (
    sourceDraftId: string,
    fields: EntryQueryCompositionSystemField[],
  ) => boolean;
  onSavedSqlRevision: (
    sourceDraftId: string,
    revision: SavedSqlRevisionUpdate,
    variableTypes: Record<string, CompositionParameterType>,
  ) => boolean;
  savedSqlEditHref: (entryId: string) => string;
  planSources: readonly CompositionResolvedSource[];
  sourceStates: Record<string, CompositionSourcePageState>;
  diagnostics: readonly CompositionResolveDiagnostic[];
  /** True while the shared debounced preview is in flight. Result spinners
   * render only while active; a settled preview without a page for a source
   * renders nothing (the diagnostics strip already covers failures). */
  previewActive: boolean;
  onNext: (sourceId: string) => void;
  onPrevious: (sourceId: string) => void;
  onRetry: (sourceId: string) => void;
  registerSourceRow: (sourceDraftId: string, el: HTMLDivElement | null) => void;
}

/**
 * Data workspace for the Composition Studio: a source navigator beside the
 * per-source editor for the selected source. EntryQuery edits flow through
 * narrow draft updaters into the shared debounced preview; Saved SQL stays
 * read-only with an explicit exact-revision update. Selection and paging
 * stay transient Work; only the selected source renders an editor.
 */
export function CompositionDataWorkspace(props: CompositionDataWorkspaceProps) {
  const [navigatorOpen, setNavigatorOpen] = createSignal(true);
  const navigatorId = `${props.headingId}-source-navigator`;
  let hideNavigatorButton: HTMLButtonElement | undefined;
  let showNavigatorButton: HTMLButtonElement | undefined;
  const selectedSource = () =>
    props.draft.sources.find((source) =>
      source.draftId === props.selectedSourceId
    );
  const metricFieldIds = (
    sourceDraftId: string,
  ): number[] => [
    ...new Set(
      props.draft.displays.flatMap((display) =>
        display.kind === "metric" && display.sourceDraftId === sourceDraftId &&
          "fieldId" in display.valueField
          ? [display.valueField.fieldId]
          : []
      ),
    ),
  ];

  return (
    <div
      class="dataWorkspace"
      classList={{ "dataWorkspace--navigator-collapsed": !navigatorOpen() }}
    >
      <aside
        id={navigatorId}
        class="dataWorkspaceNavigator"
        hidden={!navigatorOpen()}
      >
        <div class="dataWorkspaceNavigatorActions">
          <button
            class="pill iconpill icononly"
            type="button"
            ref={hideNavigatorButton}
            aria-label={t("composition.studioHideDataSources")}
            aria-expanded={navigatorOpen()}
            aria-controls={navigatorId}
            title={t("composition.studioHideDataSources")}
            onClick={() => {
              setNavigatorOpen(false);
              queueMicrotask(() => showNavigatorButton?.focus());
            }}
          >
            <UiIcon name="chevron-left" />
            <span class="ui-sr-only">
              {t("composition.studioHideDataSources")}
            </span>
          </button>
        </div>
        <SourceNavigator
          sources={props.draft.sources}
          headingId={props.headingId}
          selectedId={props.selectedSourceId}
          onSelect={props.onSelectSource}
          onMove={props.onMoveSource}
          onRemove={props.onRemoveSource}
          registerRow={props.registerSourceRow}
        />
      </aside>
      <div class="dataWorkspaceMain">
        <Show when={!navigatorOpen()}>
          <div class="dataWorkspaceMainActions">
            <button
              class="pill iconpill icononly"
              type="button"
              ref={showNavigatorButton}
              aria-label={t("composition.studioShowDataSources")}
              aria-expanded={navigatorOpen()}
              aria-controls={navigatorId}
              title={t("composition.studioShowDataSources")}
              onClick={() => {
                setNavigatorOpen(true);
                queueMicrotask(() => hideNavigatorButton?.focus());
              }}
            >
              <UiIcon name="chevron-right" />
              <span class="ui-sr-only">
                {t("composition.studioShowDataSources")}
              </span>
            </button>
          </div>
        </Show>
        <Show when={selectedSource()}>
          {(source) => (
            <div
              class="dataWorkspaceEditor"
              aria-label={source().name}
            >
              <h3>{source().name}</h3>
              <Show
                when={source().kind === "entry_query"}
                fallback={
                  <Show when={source().kind === "saved_sql"}>
                    <SavedSqlSourceViewer
                      spaceId={props.spaceId}
                      source={source() as SavedSqlSource}
                      onUpdateRevision={(revision, variableTypes) =>
                        props.onSavedSqlRevision(
                          (source() as SavedSqlSource).draftId,
                          revision,
                          variableTypes,
                        )}
                      editHref={props.savedSqlEditHref(
                        (source() as SavedSqlSource).entryId,
                      )}
                      planSources={props.planSources}
                      sourceStates={props.sourceStates}
                      diagnostics={props.diagnostics}
                      previewActive={props.previewActive}
                      onNext={props.onNext}
                      onPrevious={props.onPrevious}
                      onRetry={props.onRetry}
                    />
                  </Show>
                }
              >
                <EntryQuerySourceEditor
                  spaceId={props.spaceId}
                  source={source() as EntryQuerySource}
                  requiredMetricFieldIds={metricFieldIds(
                    (source() as EntryQuerySource).draftId,
                  )}
                  onFilters={(filters) =>
                    props.onEntryQueryFilters(
                      (source() as EntryQuerySource).draftId,
                      filters,
                    )}
                  onSort={(sort) =>
                    props.onEntryQuerySort(
                      (source() as EntryQuerySource).draftId,
                      sort,
                    )}
                  onProjection={(projection) =>
                    props.onEntryQueryProjection(
                      (source() as EntryQuerySource).draftId,
                      projection,
                    )}
                  onDisplaySystemFields={(fields) =>
                    props.onEntryQueryDisplaySystemFields(
                      (source() as EntryQuerySource).draftId,
                      fields,
                    )}
                  planSources={props.planSources}
                  sourceStates={props.sourceStates}
                  diagnostics={props.diagnostics}
                  previewActive={props.previewActive}
                  onNext={props.onNext}
                  onPrevious={props.onPrevious}
                  onRetry={props.onRetry}
                />
              </Show>
            </div>
          )}
        </Show>
      </div>
    </div>
  );
}
