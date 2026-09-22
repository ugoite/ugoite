import { A, useNavigate, useParams } from "@solidjs/router";
import { createSignal, For, Show } from "solid-js";
import { ButtonSpinner } from "~/components/ButtonSpinner";
import { LocalBusyIndicator } from "~/components/LocalBusyIndicator";
import {
  RowList,
  RowListButton,
  RowListItem,
  RowListLink,
} from "~/components/RowList";
import { UiIcon } from "~/components/UiIcon";
import { sqlApi } from "~/lib/ugoite-client";
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

  const savedQueries = () =>
    (queries() ?? []).filter((query) => query.kind === "user-query");
  const searchHistory = () =>
    (queries() ?? []).filter((query) => query.kind === "search-history");

  const runSavedQuery = async (query: SqlEntry) => {
    if (runningQueryId() !== null) return;
    if (query.variables.length > 0) {
      navigate(
        `/spaces/${encodeURIComponent(spaceId())}/queries/${
          encodeURIComponent(query.id)
        }/variables`,
      );
      return;
    }

    setRunningQueryId(query.id);
    navigate(
      `/spaces/${encodeURIComponent(spaceId())}/sql/${
        encodeURIComponent(query.id)
      }/run`,
    );
  };

  return (
    <>
      <div class="screenHead">
        <div class="screenTitle">
          <div class="eyebrow">{t("searchPage.title")}</div>
          <h1>{t("sqlPage.savedSql")}</h1>
        </div>
        <A
          class="btn primary"
          href={`/spaces/${encodeURIComponent(spaceId())}/queries/new`}
        >
          {t("sqlPage.createQuery")}
        </A>
      </div>
      {/* Panel-local spinner: saved rows stay mounted during refetch. */}
      <Show when={queries.loading}>
        <LocalBusyIndicator label={t("sqlPage.loadingSavedSql")} />
      </Show>
      <Show when={queries.error}>
        <p class="ui-alert ui-alert-error">
          {formatUserFacingError(
            queries.error,
            "sqlPage.failedLoadSavedSql",
          )}
        </p>
      </Show>
      <Show when={!queries.error}>
        <Show
          when={savedQueries().length > 0}
          fallback={
            <Show when={!queries.loading}>
              <div class="rowBtn">
                <span class="glyph">
                  <UiIcon name="sql" />
                </span>
                <span>
                  <b>{t("sqlPage.noSavedSql")}</b>
                  <small>{t("sqlPage.createDescription")}</small>
                </span>
              </div>
            </Show>
          }
        >
          <RowList label={t("sqlPage.savedSql")}>
            <For each={savedQueries()}>
              {(query) => (
                <RowListItem
                  main={
                    <RowListLink
                      href={`/spaces/${encodeURIComponent(spaceId())}/sql/${
                        encodeURIComponent(query.id)
                      }`}
                      primary={displaySqlName(query)}
                      secondary={query.variables.length > 0
                        ? t("searchPage.variables")
                        : undefined}
                      meta={formatDateLabel(query.updated_at)}
                      chevron
                    />
                  }
                  actions={query.variables.length > 0
                    ? (
                      <RowListButton
                        primary={t("searchPage.variables")}
                        onActivate={() =>
                          navigate(
                            `/spaces/${encodeURIComponent(spaceId())}/queries/${
                              encodeURIComponent(query.id)
                            }/variables`,
                          )}
                      />
                    )
                    : (
                      <button
                        type="button"
                        class="rowListAction"
                        aria-label={`${t("searchPage.runAgain")}: ${
                          displaySqlName(query)
                        }`}
                        disabled={runningQueryId() !== null}
                        aria-busy={runningQueryId() === query.id || undefined}
                        onClick={() => void runSavedQuery(query)}
                      >
                        <Show when={runningQueryId() === query.id}>
                          <ButtonSpinner />
                        </Show>
                        {t("searchPage.runAgain")}
                      </button>
                    )}
                />
              )}
            </For>
          </RowList>
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
          <div
            class="rowList sqlHistoryRows"
            role="list"
            aria-label={t("searchPage.searchHistory")}
          >
            <For
              each={searchHistory()}
              fallback={
                <div class="rowListItem" role="listitem">
                  <span class="rowListMain">
                    <span class="rowListText">
                      <span class="rowListPrimary">
                        {t("searchPage.noSearchHistory")}
                      </span>
                    </span>
                  </span>
                </div>
              }
            >
              {(query) => (
                <RowListItem
                  main={
                    <RowListLink
                      href={`/spaces/${encodeURIComponent(spaceId())}/sql/${
                        encodeURIComponent(query.id)
                      }`}
                      primary={displaySqlName(query)}
                      meta={formatDateLabel(query.updated_at)}
                      chevron
                    />
                  }
                  actions={
                    <button
                      type="button"
                      class="rowListAction"
                      aria-label={`${
                        query.variables.length > 0
                          ? t("searchPage.variables")
                          : t("searchPage.runAgain")
                      }: ${displaySqlName(query)}`}
                      disabled={runningQueryId() !== null}
                      aria-busy={runningQueryId() === query.id || undefined}
                      onClick={() => void runSavedQuery(query)}
                    >
                      <Show when={runningQueryId() === query.id}>
                        <ButtonSpinner />
                      </Show>
                      {query.variables.length > 0
                        ? t("searchPage.variables")
                        : t("searchPage.runAgain")}
                    </button>
                  }
                />
              )}
            </For>
          </div>
        </section>
      </Show>
    </>
  );
}
