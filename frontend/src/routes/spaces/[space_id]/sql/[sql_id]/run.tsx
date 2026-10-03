import { useLocation, useNavigate, useParams } from "@solidjs/router";
import {
  createEffect,
  createMemo,
  createSignal,
  onCleanup,
  Show,
} from "solid-js";
import { BackLink } from "~/components/BackLink";
import { SaveAsToolDialog } from "~/components/SaveAsToolDialog";
import {
  PagedResultTable,
  type ResultColumn,
} from "~/components/PagedResultTable";
import { sqlApi } from "~/lib/ugoite-client";
import {
  buildSavedSqlCompositionDocument,
  canCreateSavedSqlComposition,
  compositionApi,
} from "~/lib/composition-api";
import {
  clearPendingCompositionSaveAttempt,
  getPendingCompositionSaveAttempt,
  type PendingCompositionSaveAttempt,
  rememberPendingCompositionSaveAttempt,
  subscribeToPendingCompositionSaveAttempt,
} from "~/lib/composition-save-attempt";
import { createResource } from "~/lib/recoverable-resource";
import { t } from "~/lib/i18n";
import { normalizeSqlVariables } from "~/lib/sql";
import { displaySqlName } from "~/lib/sql-metadata";
import { formatUserFacingError } from "~/lib/user-facing-error";
import { spaceRoute } from "~/lib/space-shell-route";
import type { SqlEntry, SqlQueryPage } from "~/lib/types";

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

const formatCell = (value: unknown): string => {
  if (value === null || value === undefined) return "—";
  if (typeof value === "string") return value;
  if (typeof value === "number" || typeof value === "boolean") {
    return String(value);
  }
  try {
    return JSON.stringify(value);
  } catch {
    return String(value);
  }
};

const rowCell = (row: unknown, column: string, index: number): unknown => {
  if (Array.isArray(row)) return row[index];
  if (row && typeof row === "object") {
    return (row as Record<string, unknown>)[column];
  }
  return index === 0 ? row : undefined;
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
  const [saveDialogOpen, setSaveDialogOpen] = createSignal(false);
  const [saveBusy, setSaveBusy] = createSignal(false);
  const [saveRetryAvailable, setSaveRetryAvailable] = createSignal(false);
  const [saveError, setSaveError] = createSignal<string | null>(null);
  let saveSeed: {
    spaceId: string;
    routePath: string;
    entry: SqlEntry;
    columns: string[];
    parameters: Record<string, unknown>;
  } | undefined;
  let pendingSave: PendingCompositionSaveAttempt | undefined;
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

  const openSaveDialog = () => {
    if (!canSaveAsTool()) return;
    const current = entry();
    const result = visiblePage();
    if (!current || !result) return;
    saveSeed = {
      spaceId: spaceId(),
      routePath: location.pathname,
      entry: current,
      columns: [...result.columns],
      parameters: { ...(state().parameters ?? {}) },
    };
    pendingSave = undefined;
    setSaveError(null);
    setSaveRetryAvailable(false);
    setSaveDialogOpen(true);
  };

  const isCurrentSaveRoute = (attempt: {
    spaceId: string;
    sqlId: string;
    routePath: string;
  }) =>
    spaceId() === attempt.spaceId && sqlId() === attempt.sqlId &&
    location.pathname === attempt.routePath;

  const currentSaveRoute = () => ({
    spaceId: spaceId(),
    sqlId: sqlId(),
    routePath: location.pathname,
  });

  const clearSaveForStaleRoute = () => {
    setSaveDialogOpen(false);
    setSaveRetryAvailable(false);
    setSaveError(null);
    saveSeed = undefined;
    pendingSave = undefined;
  };

  createEffect(() => {
    const route = currentSaveRoute();
    if (pendingSave && !isCurrentSaveRoute(pendingSave)) {
      clearSaveForStaleRoute();
    }
    const restoreAttempt = (attempt: PendingCompositionSaveAttempt) => {
      pendingSave = attempt;
      setSaveDialogOpen(true);
      setSaveRetryAvailable(true);
      setSaveError(t("composition.saveFailed"));
    };
    const savedAttempt = getPendingCompositionSaveAttempt(route);
    if (savedAttempt) restoreAttempt(savedAttempt);
    const unsubscribe = subscribeToPendingCompositionSaveAttempt(
      route,
      restoreAttempt,
    );
    onCleanup(unsubscribe);
  });

  const saveRequest = async (attempt: PendingCompositionSaveAttempt) => {
    try {
      const response = await compositionApi.save(
        attempt.spaceId,
        attempt.yaml,
        attempt.idempotencyKey,
      );
      clearPendingCompositionSaveAttempt(attempt);
      if (!isCurrentSaveRoute(attempt)) {
        clearSaveForStaleRoute();
        return;
      }
      pendingSave = undefined;
      navigate(
        `/spaces/${encodeURIComponent(spaceId())}/compositions/${
          encodeURIComponent(response.composition_id)
        }/${encodeURIComponent(response.revision_id)}`,
      );
    } catch (error) {
      const outcome = error && typeof error === "object" &&
          "mutationOutcome" in error
        ? (error as { mutationOutcome?: unknown }).mutationOutcome
        : "unknown";
      if (outcome === "rejected") {
        clearPendingCompositionSaveAttempt(attempt);
      } else {
        rememberPendingCompositionSaveAttempt(attempt);
      }
      if (!isCurrentSaveRoute(attempt)) {
        clearSaveForStaleRoute();
        return;
      }
      setSaveRetryAvailable(outcome !== "rejected");
      setSaveError(t("composition.saveFailed"));
    }
  };

  const handleSaveAsTool = async (name: string) => {
    if (saveBusy() || saveRetryAvailable()) return;
    const seed = saveSeed;
    if (!seed || !saveDialogOpen()) return;
    pendingSave = undefined;
    setSaveError(null);
    setSaveBusy(true);
    try {
      const document = buildSavedSqlCompositionDocument(
        seed.entry,
        name,
        seed.columns,
        seed.parameters,
      );
      const canonical = await compositionApi.canonicalizeDocument(document);
      if (location.pathname !== seed.routePath) {
        clearSaveForStaleRoute();
        return;
      }
      pendingSave = {
        spaceId: seed.spaceId,
        sqlId: seed.entry.id,
        routePath: seed.routePath,
        name,
        yaml: canonical.canonical_yaml,
        idempotencyKey: crypto.randomUUID(),
      };
      await saveRequest(pendingSave);
    } catch {
      setSaveError(t("composition.saveFailed"));
    } finally {
      setSaveBusy(false);
    }
  };

  const handleRetrySave = async () => {
    if (saveBusy() || !saveRetryAvailable() || !pendingSave) return;
    setSaveError(null);
    setSaveBusy(true);
    try {
      await saveRequest(pendingSave);
    } finally {
      setSaveBusy(false);
    }
  };

  const closeSaveDialog = () => {
    if (saveBusy() || saveRetryAvailable()) return;
    setSaveDialogOpen(false);
    saveSeed = undefined;
    pendingSave = undefined;
    setSaveError(null);
  };

  const resultColumns = (): ResultColumn<unknown>[] =>
    (visiblePage()?.columns ?? []).map((column, index) => ({
      key: `column-${index}`,
      label: column,
      cell: (row) => {
        const value = formatCell(rowCell(row, column, index));
        return <span title={value}>{value}</span>;
      },
    }));

  return (
    <>
      <div class="screenHead">
        <div class="screenTitle">
          <h1>
            {!entry.loading && entry()
              ? displaySqlName(entry()!)
              : t("sqlPage.results")}
          </h1>
          <p class="ui-page-subtitle">{t("sqlPage.resultsDescription")}</p>
        </div>
        <BackLink
          href={`/spaces/${encodeURIComponent(spaceId())}/sql/${
            encodeURIComponent(sqlId())
          }`}
          label={t("sqlPage.backToSavedSql")}
        />
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
                      onClick={openSaveDialog}
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
        <PagedResultTable
          columns={resultColumns()}
          rows={visiblePage()?.rows ?? []}
          rowKey={(_row, index) =>
            `${visiblePage()?.pageIdentity ?? ""}:${index}`}
          pageIdentity={visiblePage()?.pageIdentity ?? makeQueryIdentity(
            spaceId(),
            sqlId(),
            request(),
          )}
          loading={entry.loading || pageLoading()}
          loadingLabel={t("sqlPage.loadingResults")}
          error={resultError()
            ? formatUserFacingError(resultError(), "sqlPage.failedQuery")
            : null}
          emptyLabel={t("sqlPage.noResults")}
          retryLabel={t("common.retry")}
          onRetry={pageError() && !entry.error
            ? () => setPageRetry((value) => value + 1)
            : undefined}
          canPrevious={continuationStack().length > 0}
          canNext={!!visiblePage()?.has_more && !!visiblePage()?.next}
          previousLabel={t("common.previous")}
          nextLabel={t("common.next")}
          onPrevious={handlePrevious}
          onNext={handleNext}
          paginationLabel={t("sqlPage.results")}
          classNames={{
            table: "ui-table",
            scroll: "ui-table-wrapper mt-4 overflow-x-auto",
          }}
        />
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
      <Show when={saveDialogOpen()}>
        <SaveAsToolDialog
          initialName={pendingSave?.name ?? saveSeed?.entry.name ??
            (saveSeed ? displaySqlName(saveSeed.entry) : "")}
          busy={saveBusy()}
          retryAvailable={saveRetryAvailable()}
          error={saveError()}
          onSave={(name) => void handleSaveAsTool(name)}
          onRetry={() => void handleRetrySave()}
          onClose={closeSaveDialog}
        />
      </Show>
    </>
  );
}
