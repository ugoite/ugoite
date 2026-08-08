import { A, useNavigate, useParams } from "@solidjs/router";
import { createMemo, createSignal, For, Match, Show, Switch } from "solid-js";
import { SqlQueryEditor } from "~/components";
import { formatDateLabel } from "~/lib/date-format";
import { buildSqlSchema } from "~/lib/sql";
import { sqlApi } from "~/lib/ugoite-client";
import { sqlSessionApi } from "~/lib/ugoite-client";
import { createResource } from "~/lib/recoverable-resource";
import { t } from "~/lib/i18n";
import { displaySqlName } from "~/lib/sql-metadata";
import { formatUserFacingError } from "~/lib/user-facing-error";
import { spaceRoute } from "~/lib/space-shell-route";

export const route = spaceRoute({ navigation: "search", title: "savedSqlDetail" });

const READ_ONLY_SQL_SCHEMA = buildSqlSchema([]);

export default function SpaceSqlDetailRoute() {
  const params = useParams<{ space_id: string; sql_id: string }>();
  const navigate = useNavigate();
  const spaceId = () => params.space_id;
  const sqlId = () => params.sql_id;
  const [runError, setRunError] = createSignal<string | null>(null);
  const [running, setRunning] = createSignal(false);

  const [entry] = createResource(async () => sqlApi.get(spaceId(), sqlId()));
  const variableCount = createMemo(() => entry()?.variables.length ?? 0);
  const queryVariablesHref = () =>
    `/spaces/${spaceId()}/queries/${encodeURIComponent(sqlId())}/variables`;

  const handleRun = async () => {
    const current = entry();
    if (!current || variableCount() > 0 || running()) {
      return;
    }

    setRunError(null);
    setRunning(true);
    try {
      const session = await sqlSessionApi.create(spaceId(), current.sql);
      if (session.status === "failed") {
        setRunError(
          formatUserFacingError(
            session.error,
            "querySession.failed",
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
    } catch (err) {
      setRunError(formatUserFacingError(err, "querySession.failed"));
    } finally {
      setRunning(false);
    }
  };

  return (
    <>
      <div class="screenHead">
        <div class="ui-stack-sm">
          <p class="eyebrow">{t("sqlPage.searchSavedSql")}</p>
          <Show
            when={entry()}
            fallback={<h1>{t("sqlPage.detail")}</h1>}
          >
            {(data) => (
              <>
                <h1>{displaySqlName(data())}</h1>
                <p class="ui-page-subtitle max-w-2xl">
                  {t("sqlPage.reviewDescription")}
                </p>
              </>
            )}
          </Show>
        </div>
      </div>
      <section class="settingsMain surface">
        <Switch>
          <Match when={entry.loading}>
            <p class="text-sm ui-muted">{t("sqlPage.loadingQuery")}</p>
          </Match>
          <Match when={entry.error}>
            <div class="ui-stack-sm">
              <p class="text-sm ui-text-danger">
                {formatUserFacingError(
                  entry.error,
                  "sqlPage.failedLoadQuery",
                )}
              </p>
              <p class="text-sm ui-muted">
                {t("sqlPage.failedLoadQueryDescription")}
              </p>
            </div>
          </Match>
          <Match when={entry()}>
            {(data) => (
              <>
                <dl class="grid gap-4 text-sm sm:grid-cols-3">
                  <div class="ui-stack-sm">
                    <dt class="font-semibold">{t("sqlPage.updated")}</dt>
                    <dd class="ui-muted">
                      {formatDateLabel(data().updated_at)}
                    </dd>
                  </div>
                  <div class="ui-stack-sm">
                    <dt class="font-semibold">{t("sqlPage.created")}</dt>
                    <dd class="ui-muted">
                      {formatDateLabel(data().created_at)}
                    </dd>
                  </div>
                  <div class="ui-stack-sm">
                    <dt class="font-semibold">{t("sqlPage.variables")}</dt>
                    <dd class="ui-muted">
                      {variableCount() === 0 ? t("sqlPage.noVariables") : t(
                        variableCount() === 1
                          ? "sqlPage.variableCount.one"
                          : "sqlPage.variableCount.other",
                        { count: variableCount() },
                      )}
                    </dd>
                  </div>
                </dl>

                <div class="ui-stack-sm">
                  <h2 class="text-lg font-semibold">{t("sqlPage.sql")}</h2>
                  <SqlQueryEditor
                    value={data().sql}
                    onChange={() => undefined}
                    schema={READ_ONLY_SQL_SCHEMA}
                    disabled
                  />
                </div>

                <div class="ui-stack-sm">
                  <h2 class="text-lg font-semibold">
                    {t("sqlPage.variables")}
                  </h2>
                  <Show
                    when={variableCount() > 0}
                    fallback={
                      <p class="text-sm ui-muted">
                        {t("sqlPage.noTemplateVariables")}
                      </p>
                    }
                  >
                    <ul class="list-disc space-y-2 pl-5 text-sm ui-muted">
                      <For each={data().variables}>
                        {(variable) => (
                          <li>
                            <span class="font-medium">{variable.name}</span>
                            <span class="ml-2 text-xs">{variable.type}</span>
                            <span class="ml-2">
                              {variable.description || t(
                                "sqlPage.variableDescription",
                                { name: variable.name },
                              )}
                            </span>
                          </li>
                        )}
                      </For>
                    </ul>
                  </Show>
                </div>

                <Show when={runError()}>
                  <p class="text-sm ui-text-danger">{runError()}</p>
                </Show>
              </>
            )}
          </Match>
        </Switch>

        <div class="flex flex-wrap gap-3">
          <Show when={entry() && variableCount() === 0}>
            <button
              type="button"
              class="btn primary"
              onClick={handleRun}
              disabled={running()}
            >
              {running() ? t("sqlPage.running") : t("sqlPage.runQuery")}
            </button>
          </Show>
          <Show when={entry() && variableCount() > 0}>
            <A
              href={queryVariablesHref()}
              class="btn primary"
            >
              {t("sqlPage.openVariables")}
            </A>
          </Show>
          <A
            href={`/spaces/${spaceId()}/sql`}
            class="btn"
          >
            {t("sqlPage.backToSavedSql")}
          </A>
          <A
            href={`/spaces/${spaceId()}/search`}
            class="btn"
          >
            {t("sqlPage.openSearch")}
          </A>
          <A
            href={`/spaces/${spaceId()}/dashboard`}
            class="btn"
          >
            {t("sqlPage.backToDashboard")}
          </A>
        </div>
      </section>
    </>
  );
}
