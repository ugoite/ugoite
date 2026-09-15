import { A, useNavigate, useParams } from "@solidjs/router";
import { createSignal, For, Show } from "solid-js";
import { UiIcon } from "~/components/UiIcon";
import { normalizeSqlVariables } from "~/lib/sql";
import { sqlApi, sqlSessionApi } from "~/lib/ugoite-client";
import { createResource } from "~/lib/recoverable-resource";
import { t } from "~/lib/i18n";
import { displaySqlName } from "~/lib/sql-metadata";
import { formatDateLabel } from "~/lib/date-format";
import { formatUserFacingError } from "~/lib/user-facing-error";
import { spaceRoute } from "~/lib/space-shell-route";
import type { SqlEntry } from "~/lib/types";

export const route = spaceRoute({ navigation: "search", title: "savedSql" });

export default function SpaceSqlIndexRoute() {
  const params = useParams<{ space_id: string }>();
  const navigate = useNavigate();
  const spaceId = () => params.space_id;
  const [queries] = createResource(spaceId, sqlApi.list);
  const [runningQueryId, setRunningQueryId] = createSignal<string | null>(
    null,
  );
  const [runError, setRunError] = createSignal<string | null>(null);

  const savedQueries = () =>
    (queries() ?? []).filter((query) => query.kind === "user-query");
  const searchHistory = () =>
    (queries() ?? []).filter((query) => query.kind === "search-history");

  const runSavedQuery = async (query: SqlEntry) => {
    if (query.variables.length > 0) {
      navigate(
        `/spaces/${spaceId()}/queries/${
          encodeURIComponent(query.id)
        }/variables`,
      );
      return;
    }

    setRunError(null);
    setRunningQueryId(query.id);
    try {
      const session = await sqlSessionApi.create(
        spaceId(),
        normalizeSqlVariables(query.sql).sql,
      );
      if (session.status === "failed") {
        setRunError(
          formatUserFacingError(
            session.error,
            "searchPage.error.searchFailed",
            "sql_session.create",
          ),
        );
        return;
      }
      navigate(
        `/spaces/${spaceId()}/entries?session=${
          encodeURIComponent(session.id)
        }`,
      );
    } catch (error) {
      setRunError(
        formatUserFacingError(error, "searchPage.error.savedSearchFailed"),
      );
    } finally {
      setRunningQueryId(null);
    }
  };

  return (
    <>
      <div class="screenHead">
        <div class="screenTitle">
          <div class="eyebrow">{t("searchPage.title")}</div>
          <h1>{t("sqlPage.savedSql")}</h1>
        </div>
        <A class="btn primary" href={`/spaces/${spaceId()}/queries/new`}>
          <UiIcon name="plus" /> {t("sqlPage.createButton")}
        </A>
      </div>
      <Show when={queries.loading}>
        <p class="ui-muted">{t("sqlPage.loadingSavedSql")}</p>
      </Show>
      <Show when={queries.error}>
        <p class="ui-alert ui-alert-error">
          {formatUserFacingError(
            queries.error,
            "sqlPage.failedLoadSavedSql",
          )}
        </p>
      </Show>
      <Show when={!queries.loading && !queries.error}>
        <Show
          when={savedQueries().length > 0}
          fallback={
            <div class="rowBtn">
              <span class="glyph">
                <UiIcon name="sql" />
              </span>
              <span>
                <b>{t("sqlPage.noSavedSql")}</b>
                <small>{t("sqlPage.createDescription")}</small>
              </span>
            </div>
          }
        >
          <div class="rowStack sqlRows">
            <For each={savedQueries()}>
              {(query) => (
                <A
                  class="rowBtn"
                  href={`/spaces/${spaceId()}/sql/${
                    encodeURIComponent(query.id)
                  }`}
                >
                  <span class="glyph active">
                    <UiIcon name="sql" />
                  </span>
                  <span>
                    <b>{displaySqlName(query)}</b>
                    <small>{formatDateLabel(query.updated_at)}</small>
                  </span>
                  <span>›</span>
                </A>
              )}
            </For>
          </div>
        </Show>

        <section class="sqlHistoryGroup" aria-labelledby="sql-history-title">
          <div class="sqlHistoryHeading">
            <div>
              <h2 id="sql-history-title" class="text-lg font-semibold">
                {t("searchPage.searchHistory")}
              </h2>
              <p class="mt-1 text-sm ui-muted">
                {t("searchPage.searchHistoryDescription")}
              </p>
            </div>
          </div>
          <Show when={runError()}>
            <p class="mt-3 text-sm ui-text-danger">{runError()}</p>
          </Show>
          <div class="rowStack sqlHistoryRows">
            <For
              each={searchHistory()}
              fallback={
                <div class="rowBtn">
                  <span class="glyph">
                    <UiIcon name="history" />
                  </span>
                  <span>
                    <b>{t("searchPage.noSearchHistory")}</b>
                  </span>
                </div>
              }
            >
              {(query) => (
                <div class="sqlHistoryRow">
                  <A
                    class="sqlRowLink"
                    href={`/spaces/${spaceId()}/sql/${
                      encodeURIComponent(query.id)
                    }`}
                  >
                    <span class="glyph active">
                      <UiIcon name="history" />
                    </span>
                    <span>
                      <b>{displaySqlName(query)}</b>
                      <small>{formatDateLabel(query.updated_at)}</small>
                    </span>
                  </A>
                  <button
                    type="button"
                    class="sqlRowAction"
                    aria-label={`${
                      query.variables.length > 0
                        ? t("searchPage.variables")
                        : t("searchPage.runAgain")
                    }: ${displaySqlName(query)}`}
                    disabled={runningQueryId() !== null}
                    onClick={() => void runSavedQuery(query)}
                  >
                    {runningQueryId() === query.id
                      ? t("searchPage.runningSaved")
                      : query.variables.length > 0
                      ? t("searchPage.variables")
                      : t("searchPage.runAgain")}
                  </button>
                </div>
              )}
            </For>
          </div>
        </section>
      </Show>
    </>
  );
}
