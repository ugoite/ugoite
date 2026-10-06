import { createSignal, For, onCleanup, Show } from "solid-js";
import { A } from "@solidjs/router";
import { LocalBusyIndicator } from "~/components/LocalBusyIndicator";
import { ResultPagination } from "~/components/ResultPagination";
import { SqlResultTable } from "~/components/SqlResultTable";
import type {
  CompositionResolveDiagnostic,
  CompositionResolvedSource,
} from "~/lib/composition-api";
import type {
  CompositionParameterType,
  CompositionResultType,
} from "~/lib/composition-api";
import type { DraftSource } from "~/lib/composition-draft";
import type { SavedSqlRevisionUpdate } from "~/lib/composition-draft";
import type { CompositionSourcePageState } from "~/lib/composition-query-handle";
import { t } from "~/lib/i18n";
import { normalizeSqlVariables } from "~/lib/sql";
import { sqlApi } from "~/lib/ugoite-client";
import type { SqlEntry, SqlQueryPage } from "~/lib/types";

export type SavedSqlSource = Extract<DraftSource, { kind: "saved_sql" }>;

type EntryLoad =
  | { status: "loading" }
  | { status: "ready"; entry: SqlEntry }
  | { status: "error" };

const isAbort = (error: unknown): boolean =>
  !!error && typeof error === "object" &&
  (error as { name?: unknown }).name === "AbortError";

interface SavedSqlSourceViewerProps {
  spaceId: string;
  source: SavedSqlSource;
  /** Exact revision update; composition save stays a separate explicit save. */
  onUpdateRevision: (
    revision: SavedSqlRevisionUpdate,
    variableTypes: Record<string, CompositionParameterType>,
  ) => boolean;
  /** Existing Saved SQL editor route for the Edit action. */
  editHref: string;
  planSources: readonly CompositionResolvedSource[];
  sourceStates: Record<string, CompositionSourcePageState>;
  diagnostics: readonly CompositionResolveDiagnostic[];
  onNext: (sourceId: string) => void;
  onPrevious: (sourceId: string) => void;
  onRetry: (sourceId: string) => void;
}

/**
 * Saved SQL source viewer for the Data workspace. SQL text is read-only;
 * edits happen in the existing Saved SQL editor route. When that editor
 * publishes a new revision, the viewer offers an explicit exact-revision
 * update — SQL save and composition save are never conflated.
 */
export function SavedSqlSourceViewer(props: SavedSqlSourceViewerProps) {
  const [load, setLoad] = createSignal<EntryLoad>({ status: "loading" });
  const [updating, setUpdating] = createSignal(false);
  const [updateError, setUpdateError] = createSignal<string | null>(null);
  let generation = 0;

  const reload = () => {
    const requestGeneration = ++generation;
    setLoad({ status: "loading" });
    void sqlApi.get(props.spaceId, props.source.entryId).then(
      (entry) => {
        if (requestGeneration !== generation) return;
        setLoad({ status: "ready", entry });
      },
      (error: unknown) => {
        if (requestGeneration !== generation || isAbort(error)) return;
        // Conceal denied and missing alike: no existence oracle, no
        // identifiers, just the shared unavailable statement.
        setLoad({ status: "error" });
      },
    );
  };

  reload();
  onCleanup(() => {
    generation += 1;
  });

  const entry = () => {
    const current = load();
    return current.status === "ready" ? current.entry : undefined;
  };
  const newerRevisionAvailable = () => {
    const current = entry();
    return !!current && current.revision_id !== props.source.revisionId;
  };

  const updateToEdited = () => {
    const current = entry();
    if (!current || updating()) return;
    setUpdating(true);
    setUpdateError(null);
    void (async () => {
      try {
        // Bounded probe for the server-owned column descriptor of the new
        // exact revision. The Browser never infers types from row values;
        // columns absent from the schema keep the json fallback, mirroring
        // the source picker seeding path.
        const page: SqlQueryPage = await sqlApi.query(props.spaceId, {
          sql: normalizeSqlVariables(current.sql).sql,
          parameters: {},
          parameter_types: {},
          limit: 1,
          saved_sql: { id: current.id, revision_id: current.revision_id },
        });
        const columns = [...page.columns];
        if (
          columns.length === 0 || new Set(columns).size !== columns.length
        ) {
          setUpdateError(t("composition.studioCannotAdd"));
          return;
        }
        const schemaByName = new Map(
          (page.result_schema ?? []).map((column) => [
            column.name,
            column.type,
          ]),
        );
        const ok = props.onUpdateRevision(
          {
            revisionId: current.revision_id,
            expectedResult: columns.map((name) => ({
              name,
              type: (schemaByName.get(name) ??
                "json") as CompositionResultType,
            })),
            variableNames: current.variables.map((variable) => variable.name),
          },
          Object.fromEntries(
            current.variables.map((variable) => [
              variable.name,
              variable.type as CompositionParameterType,
            ]),
          ),
        );
        if (!ok) setUpdateError(t("composition.studioCannotAdd"));
      } catch (error) {
        if (!isAbort(error)) setUpdateError(t("composition.queryFailed"));
      } finally {
        setUpdating(false);
      }
    })();
  };

  const planSource = () =>
    props.planSources.find((source) =>
      source.kind === "saved_sql" && source.source_id === props.source.draftId
    );
  const sourceState = () => props.sourceStates[props.source.draftId];
  const unavailable = () =>
    load().status === "error" ||
    (!planSource() &&
      props.diagnostics.some((diagnostic) =>
        diagnostic.code === "source_unavailable"
      ));

  return (
    <div class="ui-stack">
      <Show when={load().status === "loading"}>
        <LocalBusyIndicator label={t("sqlPage.loadingSavedSql")} />
      </Show>
      <Show when={unavailable()}>
        <p class="ui-text-danger" role="alert">
          {t("composition.diagnostic.source_unavailable")}
        </p>
      </Show>
      <Show when={!unavailable() ? entry() : undefined}>
        {(loaded) => (
          <>
            <section aria-label={t("sqlPage.sql")}>
              <div class="flex flex-wrap items-center justify-between gap-2">
                <h3 class="ui-label">{t("sqlPage.sql")}</h3>
                <A
                  class="ui-button ui-button-secondary"
                  href={props.editHref}
                >
                  {t("composition.studioEditSavedSql")}
                </A>
              </div>
              <pre class="code">{loaded().sql}</pre>
            </section>

            <section aria-label={t("sqlPage.variables")}>
              <h3 class="ui-label">{t("sqlPage.variables")}</h3>
              <Show
                when={Object.keys(props.source.variables).length > 0}
                fallback={<p class="ui-muted">{t("sqlPage.noVariables")}</p>}
              >
                <ul class="ui-stack-sm">
                  <For each={Object.entries(props.source.variables)}>
                    {([name, binding]) => (
                      <li>
                        <span class="pill">
                          <span>{name}</span>
                          <span class="ui-muted">{binding.parameter}</span>
                        </span>
                      </li>
                    )}
                  </For>
                </ul>
              </Show>
            </section>

            <section aria-label={t("entryBrowser.columns")}>
              <h3 class="ui-label">{t("entryBrowser.columns")}</h3>
              <Show
                when={props.source.expectedResult.length > 0}
                fallback={<p class="ui-muted">{t("composition.queryEmpty")}</p>}
              >
                <ul class="ui-stack-sm">
                  <For each={props.source.expectedResult}>
                    {(column) => (
                      <li>
                        <span class="pill">
                          <span>{column.name}</span>
                          <span class="ui-muted">{column.type}</span>
                        </span>
                      </li>
                    )}
                  </For>
                </ul>
              </Show>
            </section>

            <Show when={newerRevisionAvailable()}>
              <div>
                <button
                  class="ui-button ui-button-secondary"
                  type="button"
                  disabled={updating()}
                  onClick={updateToEdited}
                >
                  {t("composition.studioUseSavedSqlRevision")}
                </button>
                <Show when={updateError()}>
                  <p class="ui-text-danger" role="alert">{updateError()}</p>
                </Show>
              </div>
            </Show>

            <section aria-label={t("composition.resultPages")}>
              <h3 class="ui-label">{t("composition.resultPages")}</h3>
              <SavedSqlSourceResult
                source={props.source}
                planSource={planSource()}
                sourceState={sourceState()}
                onNext={() => props.onNext(props.source.draftId)}
                onPrevious={() => props.onPrevious(props.source.draftId)}
                onRetry={() => props.onRetry(props.source.draftId)}
              />
            </section>

            <details class="ui-stack-sm">
              <summary>{props.source.name}</summary>
              <p class="ui-muted">
                {t("composition.studioRevisionDetail", {
                  revision: props.source.revisionId,
                })}
              </p>
            </details>
          </>
        )}
      </Show>
    </div>
  );
}

function SavedSqlSourceResult(props: {
  source: SavedSqlSource;
  planSource:
    | Extract<CompositionResolvedSource, { kind: "saved_sql" }>
    | undefined;
  sourceState: CompositionSourcePageState | undefined;
  onNext: () => void;
  onPrevious: () => void;
  onRetry: () => void;
}) {
  const page = () => {
    const state = props.sourceState?.page;
    return state?.kind === "saved_sql" ? state.page : undefined;
  };
  const rows = () => page()?.rows ?? [];
  const status = () => props.sourceState?.status;
  const loading = () => !props.sourceState || status() === "loading";

  return (
    <div>
      <Show when={loading()}>
        <LocalBusyIndicator label={t("composition.queryLoading")} />
      </Show>
      <Show when={status() === "error"}>
        <p class="ui-text-danger" role="alert">
          {t("composition.queryFailed")}
        </p>
        <button
          class="ui-button ui-button-secondary"
          type="button"
          onClick={props.onRetry}
        >
          {t("composition.retry")}
        </button>
      </Show>
      <Show when={status() === "ready" && rows().length === 0}>
        <p class="ui-muted">{t("composition.queryEmpty")}</p>
      </Show>
      <Show when={status() === "ready" && rows().length > 0}>
        <SqlResultTable
          columns={page()?.columns ?? []}
          rows={rows()}
          pageIdentity={`${props.source.draftId}:${
            props.sourceState?.cursor ?? "first"
          }`}
          tableLabel={t("composition.resultPages")}
        />
        <ResultPagination
          canPrevious={(props.sourceState?.cursorStack.length ?? 0) > 1}
          canNext={!!page()?.has_more && !!page()?.next}
          busy={status() !== "ready"}
          previousLabel={t("composition.previous")}
          nextLabel={t("composition.next")}
          ariaLabel={t("composition.resultPages")}
          onPrevious={props.onPrevious}
          onNext={props.onNext}
        />
      </Show>
    </div>
  );
}
