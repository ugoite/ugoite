import { Show } from "solid-js";
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
} from "~/lib/entry-query-composition";
import type { CompositionSourcePageState } from "~/lib/composition-query-handle";

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
  onSavedSqlRevision: (
    sourceDraftId: string,
    revision: SavedSqlRevisionUpdate,
    variableTypes: Record<string, CompositionParameterType>,
  ) => boolean;
  savedSqlEditHref: (entryId: string) => string;
  planSources: readonly CompositionResolvedSource[];
  sourceStates: Record<string, CompositionSourcePageState>;
  diagnostics: readonly CompositionResolveDiagnostic[];
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
  const selectedSource = () =>
    props.draft.sources.find((source) =>
      source.draftId === props.selectedSourceId
    );

  return (
    <div class="dataWorkspace">
      <SourceNavigator
        sources={props.draft.sources}
        headingId={props.headingId}
        selectedId={props.selectedSourceId}
        onSelect={props.onSelectSource}
        onMove={props.onMoveSource}
        onRemove={props.onRemoveSource}
        registerRow={props.registerSourceRow}
      />
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
                    onNext={props.onNext}
                    onPrevious={props.onPrevious}
                    onRetry={props.onRetry}
                  />
                </Show>
              }
            >
              <EntryQuerySourceEditor
                source={source() as EntryQuerySource}
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
                planSources={props.planSources}
                sourceStates={props.sourceStates}
                diagnostics={props.diagnostics}
                onNext={props.onNext}
                onPrevious={props.onPrevious}
                onRetry={props.onRetry}
              />
            </Show>
          </div>
        )}
      </Show>
    </div>
  );
}
