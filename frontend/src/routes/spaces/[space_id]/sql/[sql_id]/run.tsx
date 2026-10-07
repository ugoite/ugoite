import { useLocation, useNavigate, useParams } from "@solidjs/router";
import {
  createEffect,
  createMemo,
  createSignal,
  onCleanup,
  Show,
} from "solid-js";
import { BackLink } from "~/components/BackLink";
import { LocalBusyIndicator } from "~/components/LocalBusyIndicator";
import { ResultPagination } from "~/components/ResultPagination";
import { SqlResultTable } from "~/components/SqlResultTable";
import { sqlApi } from "~/lib/ugoite-client";
import {
  canCreateSavedSqlComposition,
  type CompositionParameterType,
  type CompositionResultType,
} from "~/lib/composition-api";
import type { CompositionStudioSeed } from "~/lib/composition-draft";
import { spaceCompositionNewPath } from "~/lib/space-path";
import { t } from "~/lib/i18n";
import { normalizeSqlVariables } from "~/lib/sql";
import { displaySqlName } from "~/lib/sql-metadata";
import { formatUserFacingError } from "~/lib/user-facing-error";
import { spaceRoute } from "~/lib/space-shell-route";
import { createResource } from "~/lib/recoverable-resource";
import type { SqlQueryPage } from "~/lib/types";

export const route = spaceRoute({
  navigation: "search",
});

type SqlRunState = {
  parameters?: Record<string, unknown>;
  parameterTypes?: Record<string, string>;
};

type DisplaySqlQueryPage = SqlQueryPage & {
  pageNumber: number;
  pageIdentity: string;
  queryIdentity: string;
};

const runState = (value: unknown): SqlRunState => {
  if (!value || typeof value !== "object") return {};
  const state = value as Record<string, unknown>;
  const parameters = state.parameters;
  const parameterTypes = state.parameterTypes;
  return {
    ...(parameters && typeof parameters === "object" &&
        !Array.isArray(parameters)
      ? { parameters: parameters as Record<string, unknown> }
      : {}),
    ...(parameterTypes && typeof parameterTypes === "object" &&
        !Array.isArray(parameterTypes)
      ? { parameterTypes: parameterTypes as Record<string, string> }
      : {}),
  };
};

export default function SpaceSqlRunRoute() {
  const params = useParams<{ space_id: string; sql_id: string }>();
  const location = useLocation();
  const navigate = useNavigate();
  const spaceId = () => params.space_id;
  const sqlId = () => params.sql_id;
  const [continuation, setContinuation] = createSignal<string | undefined>();
  const [continuationStack, setContinuationStack] = createSignal<string[]>([]);
  const [count, setCount] = createSignal<number | null>(null);
  const [counting, setCounting] = createSignal(false);
  const [countError, setCountError] = createSignal<string | null>(null);
  const [page, setPage] = createSignal<DisplaySqlQueryPage>();
  const [pageLoading, setPageLoading] = createSignal(false);
  const [pageError, setPageError] = createSignal<unknown>(null);
  const [pageRetry, setPageRetry] = createSignal(0);
  const [countResultIdentity, setCountResultIdentity] = createSignal("");
  const [saveError, setSaveError] = createSignal<string | null>(null);
  const state = createMemo(() => runState(location.state));

  const [entry] = createResource(
    () => `${spaceId()}\u0000${sqlId()}`,
    (key) => {
      const [requestedSpace, requestedSql] = key.split("\u0000");
      return sqlApi.get(requestedSpace, requestedSql);
    },
  );
  const request = createMemo(() => {
    const current = entry();
    if (!current) return undefined;
    const currentState = state();
    return {
      sql: normalizeSqlVariables(current.sql).sql,
      parameters: currentState.parameters ?? {},
      parameter_types: currentState.parameterTypes ?? {},
      limit: 100,
      saved_sql: { id: current.id, revision_id: current.revision_id },
      ...(continuation() ? { continuation: continuation() } : {}),
    };
  });
  const makeQueryIdentity = (
    requestedSpace: string,
    requestedSql: string,
    value: ReturnType<typeof request>,
  ) =>
    JSON.stringify({
      spaceId: requestedSpace,
      sqlId: requestedSql,
      sql: value?.sql,
      savedSql: value?.saved_sql,
      parameters: value?.parameters,
      parameter_types: value?.parameter_types,
    });
  const visiblePage = () =>
    page()?.queryIdentity === makeQueryIdentity(spaceId(), sqlId(), request())
      ? page()
      : undefined;
  let pageGeneration = 0;
  let countGeneration = 0;
  let pageController: AbortController | undefined;
  let countController: AbortController | undefined;
  let previousQueryIdentity = "";

  createEffect(() => {
    const requestedSpace = spaceId();
    const requestedSql = sqlId();
    const value = request();
    pageRetry();
    const currentQueryIdentity = makeQueryIdentity(
      requestedSpace,
      requestedSql,
      value,
    );
    if (currentQueryIdentity !== previousQueryIdentity) {
      previousQueryIdentity = currentQueryIdentity;
      setPage(undefined);
      setCount(null);
      setCountError(null);
      setCountResultIdentity("");
      setContinuation(undefined);
      setContinuationStack([]);
      countGeneration += 1;
      countController?.abort();
      countController = undefined;
      setCounting(false);
      if (value?.continuation) {
        pageGeneration += 1;
        pageController?.abort();
        pageController = undefined;
        setContinuation(undefined);
        return;
      }
    }
    const generation = ++pageGeneration;
    pageController?.abort();
    pageController = undefined;
    if (!value || entry.loading) {
      setPage(undefined);
      setPageError(null);
      setPageLoading(false);
      return;
    }
    const controller = new AbortController();
    pageController = controller;
    setPageError(null);
    setPageLoading(true);
    void sqlApi.query(requestedSpace, value, controller.signal).then(
      (result) => {
        if (generation === pageGeneration && !controller.signal.aborted) {
          setPage({
            ...result,
            pageNumber: continuationStack().length + 1,
            pageIdentity: JSON.stringify({
              queryIdentity: currentQueryIdentity,
              continuation: value.continuation,
            }),
            queryIdentity: currentQueryIdentity,
          });
        }
      },
      (error: unknown) => {
        if (
          generation === pageGeneration && !controller.signal.aborted &&
          !(error && typeof error === "object" &&
            (error as { name?: unknown }).name === "AbortError")
        ) {
          setPageError(error);
        }
      },
    ).finally(() => {
      if (generation === pageGeneration) {
        if (pageController === controller) pageController = undefined;
        setPageLoading(false);
      }
    });
  });

  onCleanup(() => {
    pageGeneration += 1;
    countGeneration += 1;
    pageController?.abort();
    countController?.abort();
    pageController = undefined;
    countController = undefined;
  });

  createEffect(() => {
    sqlId();
    setContinuation(undefined);
    setContinuationStack([]);
    setCount(null);
    setCountError(null);
  });

  const handleNext = () => {
    const next = visiblePage()?.next;
    if (!next || pageLoading()) return;
    setContinuationStack((stack) => [...stack, next]);
    setContinuation(next);
  };

  const handlePrevious = () => {
    if (pageLoading()) return;
    const previous = continuationStack().slice(0, -1);
    setContinuationStack(previous);
    setContinuation(previous.at(-1));
  };

  const handleCount = async () => {
    const current = entry();
    if (!current || counting() || entry.loading) return;
    countController?.abort();
    const controller = new AbortController();
    countController = controller;
    const generation = ++countGeneration;
    const requestedSpace = spaceId();
    const requestedSql = sqlId();
    const value = request();
    if (!value) return;
    const requestedIdentity = makeQueryIdentity(
      requestedSpace,
      requestedSql,
      value,
    );
    setCountError(null);
    setCounting(true);
    try {
      const result = await sqlApi.count(requestedSpace, {
        sql: value.sql,
        parameters: value.parameters,
        parameter_types: value.parameter_types,
        saved_sql: value.saved_sql,
      }, controller.signal);
      if (
        generation !== countGeneration || requestedIdentity !==
          makeQueryIdentity(spaceId(), sqlId(), request())
      ) return;
      setCount(result);
      setCountResultIdentity(requestedIdentity);
    } catch (error) {
      if (
        generation === countGeneration && requestedIdentity ===
          makeQueryIdentity(spaceId(), sqlId(), request()) &&
        !controller.signal.aborted &&
        !(error && typeof error === "object" &&
          (error as { name?: unknown }).name === "AbortError")
      ) {
        setCountError(formatUserFacingError(error, "sqlPage.failedCount"));
        setCountResultIdentity(requestedIdentity);
      }
    } finally {
      if (generation === countGeneration) setCounting(false);
    }
  };

  const resultError = () => entry.error || pageError();
  const canSaveAsTool = () => {
    const current = entry();
    const result = visiblePage();
    return !!current && canCreateSavedSqlComposition(current) && !!result &&
      !entry.loading && !pageLoading() && !resultError() &&
      result.columns.length > 0 &&
      new Set(result.columns).size === result.columns.length;
  };

  const openInStudio = () => {
    if (!canSaveAsTool()) return;
    const current = entry();
    const result = visiblePage();
    if (!current || !result) return;
    try {
      // The seed reuses the save-as-tool computation: the exact Saved SQL
      // revision with server-owned column types from this result page
      // (json fallback, unique columns required) and same-named variables.
      const columns = [...result.columns];
      if (
        columns.length === 0 || new Set(columns).size !== columns.length
      ) {
        setSaveError(t("composition.saveFailed"));
        return;
      }
      const schemaByName = new Map(
        (result.result_schema ?? []).map((column) => [
          column.name,
          column.type,
        ]),
      );
      const seed: CompositionStudioSeed = {
        kind: "saved_sql",
        seed: {
          entryId: current.id,
          revisionId: current.revision_id,
          name: displaySqlName(current),
          expectedResult: columns.map((column) => ({
            name: column,
            type: (schemaByName.get(column) ?? "json") as CompositionResultType,
          })),
          variables: Object.fromEntries(
            current.variables.map((variable) => [
              variable.name,
              { parameter: variable.name },
            ]),
          ),
          variableTypes: Object.fromEntries(
            current.variables.map((variable) => [
              variable.name,
              variable.type as CompositionParameterType,
            ]),
          ),
          // The run that was just executed supplies the parameter defaults
          // so the seeded Studio opens showing the same result.
          variableDefaults: Object.fromEntries(
            current.variables.flatMap((variable) => {
              const value = state().parameters?.[variable.name];
              return value === undefined ? [] : [[variable.name, value]];
            }),
          ),
        },
      };
      setSaveError(null);
      navigate(spaceCompositionNewPath(spaceId()), { state: { seed } });
    } catch {
      setSaveError(t("composition.saveFailed"));
    }
  };

  return (
    <>
      <div class="screenHead">
        <div class="screenHeadStart">
          <BackLink
            href={`/spaces/${encodeURIComponent(spaceId())}/sql/${
              encodeURIComponent(sqlId())
            }`}
            label={t("sqlPage.backToSavedSql")}
          />
          <div class="screenTitle">
            <h1>
              {!entry.loading && entry()
                ? displaySqlName(entry()!)
                : t("sqlPage.results")}
            </h1>
          </div>
        </div>
      </div>

      <section
        class="settingsMain surface"
        aria-busy={entry.loading || pageLoading() || undefined}
      >
        <Show when={visiblePage()}>
          {(result) => (
            <>
              <div class="flex flex-wrap items-center justify-between gap-3">
                <p class="text-sm ui-muted">
                  {t("sqlPage.pageNumber", {
                    page: result().pageNumber,
                  })}
                </p>
                <div class="flex flex-wrap items-center gap-2">
                  <Show when={canSaveAsTool()}>
                    <button
                      type="button"
                      class="ui-button ui-button-secondary"
                      onClick={openInStudio}
                    >
                      {t("composition.saveAsTool")}
                    </button>
                  </Show>
                  <Show
                    when={count() !== null &&
                      countResultIdentity() === makeQueryIdentity(
                          spaceId(),
                          sqlId(),
                          request(),
                        )}
                  >
                    <span class="text-sm ui-muted">
                      {t("sqlPage.resultCount", { count: count()! })}
                    </span>
                  </Show>
                  <button
                    type="button"
                    class="ui-button ui-button-secondary"
                    disabled={counting()}
                    aria-busy={counting() || undefined}
                    onClick={() => void handleCount()}
                  >
                    {counting()
                      ? t("sqlPage.counting")
                      : t("sqlPage.loadCount")}
                  </button>
                </div>
              </div>
              <Show
                when={countError() &&
                  countResultIdentity() === makeQueryIdentity(
                      spaceId(),
                      sqlId(),
                      request(),
                    )}
              >
                <p class="mt-4 text-sm ui-text-danger">{countError()}</p>
              </Show>
            </>
          )}
        </Show>
        <Show when={entry.loading || pageLoading()}>
          <LocalBusyIndicator label={t("sqlPage.loadingResults")} />
        </Show>
        <Show when={resultError()}>
          <p class="ui-text-danger" role="alert">
            {formatUserFacingError(resultError(), "sqlPage.failedQuery")}
          </p>
          <Show when={pageError() && !entry.error}>
            <button
              type="button"
              class="ui-button ui-button-secondary"
              disabled={entry.loading || pageLoading()}
              onClick={() => setPageRetry((value) => value + 1)}
            >
              {t("common.retry")}
            </button>
          </Show>
        </Show>
        <Show
          when={!entry.loading && !pageLoading() && !resultError() &&
            (visiblePage()?.rows ?? []).length === 0}
        >
          <p class="ui-muted">{t("sqlPage.noResults")}</p>
        </Show>
        <Show
          when={!resultError() && (visiblePage()?.rows ?? []).length > 0}
        >
          <SqlResultTable
            columns={visiblePage()?.columns ?? []}
            rows={visiblePage()?.rows ?? []}
            pageIdentity={visiblePage()?.pageIdentity ?? makeQueryIdentity(
              spaceId(),
              sqlId(),
              request(),
            )}
            tableLabel={t("sqlPage.results")}
          />
        </Show>
        <ResultPagination
          canPrevious={continuationStack().length > 0}
          canNext={!!visiblePage()?.has_more && !!visiblePage()?.next}
          busy={entry.loading || pageLoading() || !!resultError()}
          previousLabel={t("common.previous")}
          nextLabel={t("common.next")}
          ariaLabel={t("sqlPage.results")}
          onPrevious={handlePrevious}
          onNext={handleNext}
        />
        <Show when={saveError()}>
          <p class="ui-alert ui-alert-error mt-4" role="alert">
            {saveError()}
          </p>
        </Show>
        <Show when={entry.error && !pageError()}>
          <button
            type="button"
            class="ui-button ui-button-secondary mt-4"
            onClick={() =>
              navigate(`/spaces/${encodeURIComponent(spaceId())}/sql`)}
          >
            {t("sqlPage.backToSavedSql")}
          </button>
        </Show>
      </section>
    </>
  );
}
