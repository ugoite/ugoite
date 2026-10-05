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
import { LocalBusyIndicator } from "~/components/LocalBusyIndicator";
import { ResultPagination } from "~/components/ResultPagination";
import { SqlResultTable } from "~/components/SqlResultTable";
import { sqlApi } from "~/lib/ugoite-client";
import {
  buildSavedSqlCompositionDocument,
  canCreateSavedSqlComposition,
  compositionApi,
} from "~/lib/composition-api";
import { compositionSaveErrorMessage } from "~/lib/composition-save-error";
import type { SqlResultColumn } from "~/lib/types";
import {
  beginCompositionSaveRouteVisit,
  clearPendingCompositionSaveAttempt,
  getPendingCompositionSaveAttempt,
  isCurrentCompositionSaveRouteVisit,
  markPendingCompositionSaveAttemptUncertain,
  type PendingCompositionSaveAttempt,
  stagePendingCompositionSaveAttempt,
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
  const [rejectedSaveName, setRejectedSaveName] = createSignal<string | null>(
    null,
  );
  let saveSeed: {
    spaceId: string;
    sqlId: string;
    routePath: string;
    entry: SqlEntry;
    columns: string[];
    resultSchema?: SqlResultColumn[];
    parameters: Record<string, unknown>;
  } | undefined;
  let pendingSave: PendingCompositionSaveAttempt | undefined;
  let rejectedSaveRoute:
    | Pick<
      PendingCompositionSaveAttempt,
      "spaceId" | "sqlId" | "routePath"
    >
    | undefined;
  let saveRouteVisitId: number | undefined;
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
      sqlId: sqlId(),
      routePath: location.pathname,
      entry: current,
      columns: [...result.columns],
      resultSchema: result.result_schema
        ? [...result.result_schema]
        : undefined,
      parameters: { ...(state().parameters ?? {}) },
    };
    pendingSave = undefined;
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

  const isCurrentSaveVisit = (
    attempt: PendingCompositionSaveAttempt,
    visitId: number | undefined,
  ) =>
    isCurrentSaveRoute(attempt) &&
    isCurrentCompositionSaveRouteVisit(attempt, visitId);

  const currentSaveRoute = () => ({
    spaceId: spaceId(),
    sqlId: sqlId(),
    routePath: location.pathname,
  });

  const clearSaveForStaleRoute = () => {
    setSaveDialogOpen(false);
    setSaveBusy(false);
    setSaveRetryAvailable(false);
    setSaveError(null);
    setRejectedSaveName(null);
    rejectedSaveRoute = undefined;
    saveSeed = undefined;
    pendingSave = undefined;
  };

  createEffect(() => {
    const route = currentSaveRoute();
    const routeVisitId = beginCompositionSaveRouteVisit(route);
    saveRouteVisitId = routeVisitId;
    if (
      (rejectedSaveRoute && !isCurrentSaveRoute(rejectedSaveRoute)) ||
      (saveSeed && !isCurrentSaveRoute(saveSeed)) ||
      (pendingSave && !isCurrentSaveRoute(pendingSave))
    ) {
      clearSaveForStaleRoute();
    }
    const restoreAttempt = (
      stored: NonNullable<ReturnType<typeof getPendingCompositionSaveAttempt>>,
    ) => {
      pendingSave = stored.attempt;
      setSaveDialogOpen(true);
      setSaveBusy(false);
      setSaveRetryAvailable(true);
      setSaveError(
        stored.state === "uncertain" ? t("composition.saveFailed") : null,
      );
    };
    const savedAttempt = getPendingCompositionSaveAttempt(route);
    if (savedAttempt) restoreAttempt(savedAttempt);
    const unsubscribe = subscribeToPendingCompositionSaveAttempt(
      route,
      (event) => {
        if (event.type === "pending") {
          if (event.stored.routeVisitId !== routeVisitId) return;
          restoreAttempt(event.stored);
          return;
        }
        if (event.routeVisitId !== routeVisitId) return;
        if (pendingSave?.idempotencyKey !== event.idempotencyKey) return;
        pendingSave = undefined;
        setSaveBusy(false);
        setSaveRetryAvailable(false);
        rejectedSaveRoute = event.outcome === "rejected" ? route : undefined;
        setRejectedSaveName(
          event.outcome === "rejected" ? event.attemptName : null,
        );
        setSaveError(
          event.outcome === "rejected" ? t("composition.saveFailed") : null,
        );
        if (!saveSeed) setSaveDialogOpen(false);
      },
    );
    onCleanup(unsubscribe);
  });

  const saveRequest = async (
    attempt: PendingCompositionSaveAttempt,
    visitId: number | undefined,
    isRetry = false,
  ) => {
    stagePendingCompositionSaveAttempt(attempt, visitId);
    try {
      const response = await compositionApi.save(
        attempt.spaceId,
        attempt.yaml,
        attempt.idempotencyKey,
      );
      if (!isCurrentSaveVisit(attempt, visitId)) {
        markPendingCompositionSaveAttemptUncertain(attempt, visitId);
        return;
      }
      clearPendingCompositionSaveAttempt(attempt, "completed", visitId);
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
      const status = error && typeof error === "object" && "status" in error
        ? (error as { status?: unknown }).status
        : undefined;
      const retryWasDenied = isRetry && (status === 401 || status === 403);
      if (outcome === "rejected" && !retryWasDenied) {
        clearPendingCompositionSaveAttempt(attempt, "rejected", visitId);
        if (isCurrentSaveVisit(attempt, visitId)) {
          setSaveError(compositionSaveErrorMessage(error));
        }
      } else {
        markPendingCompositionSaveAttemptUncertain(attempt, visitId);
      }
    }
  };

  const handleSaveAsTool = async (name: string) => {
    if (saveBusy() || saveRetryAvailable()) return;
    const seed = saveSeed;
    if (!seed || !saveDialogOpen()) return;
    const visitId = saveRouteVisitId;
    pendingSave = undefined;
    rejectedSaveRoute = undefined;
    setSaveError(null);
    setRejectedSaveName(null);
    setSaveBusy(true);
    try {
      const document = buildSavedSqlCompositionDocument(
        seed.entry,
        name,
        seed.columns,
        seed.parameters,
        seed.resultSchema,
      );
      const canonical = await compositionApi.canonicalizeDocument(document);
      if (
        saveRouteVisitId !== visitId || !isCurrentSaveRoute(seed)
      ) return;
      const attempt: PendingCompositionSaveAttempt = {
        spaceId: seed.spaceId,
        sqlId: seed.sqlId,
        routePath: seed.routePath,
        name,
        yaml: canonical.canonical_yaml,
        idempotencyKey: crypto.randomUUID(),
      };
      pendingSave = attempt;
      await saveRequest(attempt, visitId);
    } catch {
      if (saveRouteVisitId === visitId) {
        setSaveError(t("composition.saveFailed"));
      }
    } finally {
      if (saveRouteVisitId === visitId) setSaveBusy(false);
    }
  };

  const handleRetrySave = async () => {
    if (saveBusy() || !saveRetryAvailable() || !pendingSave) return;
    const attempt = pendingSave;
    const visitId = saveRouteVisitId;
    setSaveError(null);
    setSaveBusy(true);
    try {
      await saveRequest(attempt, visitId, true);
    } finally {
      if (saveRouteVisitId === visitId) setSaveBusy(false);
    }
  };

  const closeSaveDialog = () => {
    if (saveBusy() || saveRetryAvailable()) return;
    setSaveDialogOpen(false);
    saveSeed = undefined;
    pendingSave = undefined;
    if (!rejectedSaveName()) setSaveError(null);
  };

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
        <Show when={saveError() && !saveDialogOpen()}>
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
      <Show when={saveDialogOpen()}>
        <SaveAsToolDialog
          initialName={pendingSave?.name ?? rejectedSaveName() ??
            saveSeed?.entry.name ??
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
