import { useLocation, useNavigate, useParams } from "@solidjs/router";
import { createEffect, createMemo, createSignal, For, Show } from "solid-js";
import { BackLink } from "~/components/BackLink";
import { LocalBusyIndicator } from "~/components/LocalBusyIndicator";
import { sqlApi } from "~/lib/ugoite-client";
import { createResource } from "~/lib/recoverable-resource";
import { t } from "~/lib/i18n";
import { normalizeSqlVariables } from "~/lib/sql";
import { displaySqlName } from "~/lib/sql-metadata";
import { formatUserFacingError } from "~/lib/user-facing-error";
import { spaceRoute } from "~/lib/space-shell-route";
import type { SqlQueryPage } from "~/lib/types";

export const route = spaceRoute({
  navigation: "search",
});

type SqlRunState = {
  parameters?: Record<string, unknown>;
  parameterTypes?: Record<string, string>;
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
  const state = createMemo(() => runState(location.state));

  const [entry] = createResource(
    () => sqlId(),
    (id) => sqlApi.get(spaceId(), id),
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
      ...(continuation() ? { continuation: continuation() } : {}),
    };
  });
  const [page] = createResource<
    ReturnType<typeof request>,
    SqlQueryPage | undefined
  >(
    request,
    async (value) => value ? await sqlApi.query(spaceId(), value) : undefined,
  );

  createEffect(() => {
    sqlId();
    setContinuation(undefined);
    setContinuationStack([]);
    setCount(null);
    setCountError(null);
  });

  const handleNext = () => {
    const next = page()?.next;
    if (!next) return;
    setContinuationStack((stack) => [...stack, next]);
    setContinuation(next);
  };

  const handlePrevious = () => {
    const previous = continuationStack().slice(0, -1);
    setContinuationStack(previous);
    setContinuation(previous.at(-1));
  };

  const handleCount = async () => {
    const current = entry();
    if (!current || counting()) return;
    setCountError(null);
    setCounting(true);
    try {
      const currentState = state();
      setCount(
        await sqlApi.count(spaceId(), {
          sql: normalizeSqlVariables(current.sql).sql,
          parameters: currentState.parameters ?? {},
          parameter_types: currentState.parameterTypes ?? {},
        }),
      );
    } catch (error) {
      setCountError(formatUserFacingError(error, "sqlPage.failedCount"));
    } finally {
      setCounting(false);
    }
  };

  const resultError = () => entry.error || page.error;

  return (
    <>
      <div class="screenHead">
        <div class="screenTitle">
          <h1>{entry() ? displaySqlName(entry()!) : t("sqlPage.results")}</h1>
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
        aria-busy={entry.loading || page.loading || undefined}
      >
        <Show when={entry.loading || page.loading}>
          <LocalBusyIndicator label={t("sqlPage.loadingResults")} />
        </Show>
        <Show when={resultError()}>
          <p class="text-sm ui-text-danger">
            {formatUserFacingError(resultError(), "sqlPage.failedQuery")}
          </p>
        </Show>
        <Show when={page()}>
          {(result) => (
            <>
              <div class="flex flex-wrap items-center justify-between gap-3">
                <p class="text-sm ui-muted">
                  {t("sqlPage.pageNumber", {
                    page: continuationStack().length + 1,
                  })}
                </p>
                <div class="flex flex-wrap items-center gap-2">
                  <Show when={count() !== null}>
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
                when={result().rows.length > 0}
                fallback={
                  <p class="mt-4 text-sm ui-muted">{t("sqlPage.noResults")}</p>
                }
              >
                <div class="ui-table-wrapper mt-4 overflow-x-auto">
                  <table class="ui-table">
                    <thead class="ui-table-head">
                      <tr>
                        <For each={result().columns}>
                          {(column) => <th scope="col">{column}</th>}
                        </For>
                      </tr>
                    </thead>
                    <tbody class="ui-table-body">
                      <For each={result().rows}>
                        {(row) => (
                          <tr>
                            <For each={result().columns}>
                              {(column, index) => (
                                <td>
                                  {formatCell(rowCell(row, column, index()))}
                                </td>
                              )}
                            </For>
                          </tr>
                        )}
                      </For>
                    </tbody>
                  </table>
                </div>
              </Show>

              <Show when={countError()}>
                <p class="mt-4 text-sm ui-text-danger">{countError()}</p>
              </Show>
              <div class="mt-6 flex flex-wrap items-center justify-between gap-3">
                <button
                  type="button"
                  class="ui-button ui-button-secondary"
                  disabled={continuationStack().length === 0 || page.loading}
                  onClick={handlePrevious}
                >
                  {t("common.previous")}
                </button>
                <button
                  type="button"
                  class="ui-button ui-button-secondary"
                  disabled={!result().has_more || !result().next ||
                    page.loading}
                  onClick={handleNext}
                >
                  {t("common.next")}
                </button>
              </div>
            </>
          )}
        </Show>
        <Show when={entry.error && !page.error}>
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
