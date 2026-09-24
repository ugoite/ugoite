import { A, useNavigate, useParams } from "@solidjs/router";
import { createEffect, createMemo, createSignal, For, Match, Show, Switch } from "solid-js";
import { ActionIconBar } from "~/components/ActionIconBar";
import { BackLink } from "~/components/BackLink";
import { ConfirmDestructiveAction } from "~/components/ConfirmDestructiveAction";
import { LocalBusyIndicator } from "~/components/LocalBusyIndicator";
import { SqlQueryEditor } from "~/components";
import { formatDateLabel } from "~/lib/date-format";
import { buildSqlSchema, normalizeSqlVariables } from "~/lib/sql";
import { formApi, sqlApi } from "~/lib/ugoite-client";
import { createResource } from "~/lib/recoverable-resource";
import { t } from "~/lib/i18n";
import { displaySqlName } from "~/lib/sql-metadata";
import { formatUserFacingError } from "~/lib/user-facing-error";
import { spaceRoute } from "~/lib/space-shell-route";
import type { Form } from "~/lib/types";

export const route = spaceRoute({
  navigation: "search",
});

export default function SpaceSqlDetailRoute() {
  const params = useParams<{ space_id: string; sql_id: string }>();
  const navigate = useNavigate();
  const spaceId = () => params.space_id;
  const sqlId = () => params.sql_id;

  const [entry] = createResource(async () => sqlApi.get(spaceId(), sqlId()));
  const [forms] = createResource(async () => formApi.list(spaceId()));
  const [queryName, setQueryName] = createSignal("");
  const [sqlInput, setSqlInput] = createSignal("");
  const [savedName, setSavedName] = createSignal("");
  const [savedSql, setSavedSql] = createSignal("");
  const [revisionId, setRevisionId] = createSignal<string | null>(null);
  const [busy, setBusy] = createSignal<"save" | "delete" | null>(null);
  const [actionError, setActionError] = createSignal<string | null>(null);
  const [deleteConfirmOpen, setDeleteConfirmOpen] = createSignal(false);

  createEffect(() => {
    const current = entry();
    if (!current) return;
    setQueryName(current.name ?? "");
    setSqlInput(current.sql);
    setSavedName(current.name ?? "");
    setSavedSql(current.sql);
    setRevisionId(current.revision_id);
  });

  const normalized = createMemo(() => normalizeSqlVariables(sqlInput()));
  const variableCount = createMemo(() => normalized().variables.length);
  const isDirty = createMemo(() =>
    queryName().trim() !== savedName() || sqlInput() !== savedSql()
  );
  const queryVariablesHref = () =>
    `/spaces/${encodeURIComponent(spaceId())}/sql/${
      encodeURIComponent(sqlId())
    }/variables`;

  const handleRun = () => {
    if (!entry() || isDirty() || variableCount() > 0) return;
    navigate(
      `/spaces/${encodeURIComponent(spaceId())}/sql/${
        encodeURIComponent(sqlId())
      }/run`,
    );
  };

  const handleSave = async () => {
    const current = entry();
    if (!current || busy() !== null) return;
    setActionError(null);
    const sql = normalized().sql.trim();
    if (!sql) {
      setActionError(t("sqlPage.sqlRequired"));
      return;
    }
    const name = queryName().trim() || null;
    const nextRevisionId = revisionId() ?? current.revision_id;
    setBusy("save");
    try {
      const result = await sqlApi.update(spaceId(), sqlId(), {
        name,
        kind: "user-query",
        metadata: name ? undefined : { generatedName: "untitled" },
        sql,
        variables: normalized().variables,
        parent_revision_id: nextRevisionId,
      });
      setQueryName(name ?? "");
      setSqlInput(sql);
      setSavedName(name ?? "");
      setSavedSql(sql);
      setRevisionId(result.revisionId);
    } catch (error) {
      setActionError(formatUserFacingError(error, "sqlPage.failedSave"));
    } finally {
      setBusy(null);
    }
  };

  const openDeleteConfirm = () => {
    if (busy() !== null) return;
    setActionError(null);
    setDeleteConfirmOpen(true);
  };

  const closeDeleteConfirm = () => {
    if (busy() !== null) return;
    setActionError(null);
    setDeleteConfirmOpen(false);
  };

  const handleDelete = async () => {
    if (busy() !== null) return;
    setActionError(null);
    setBusy("delete");
    try {
      await sqlApi.delete(spaceId(), sqlId());
      setDeleteConfirmOpen(false);
      navigate(`/spaces/${encodeURIComponent(spaceId())}/sql`);
    } catch (error) {
      setActionError(formatUserFacingError(error, "sqlPage.failedDelete"));
      setBusy(null);
    }
  };

  return (
    <>
      <div class="screenHead">
        <div class="ui-stack-sm">
          <Show
            when={entry()}
            fallback={<h1>{t("sqlPage.detail")}</h1>}
          >
            {(data) => (
              <h1>{data().kind === "search-history"
                ? displaySqlName(data())
                : queryName().trim() || t("sqlPage.untitledQuery")}</h1>
            )}
          </Show>
        </div>
      </div>
      <section
        class="settingsMain surface"
        aria-busy={entry.loading || undefined}
      >
        <Switch>
          {/* Panel-local spinner: loaded query stays mounted on refetch. */}
          <Match when={entry.loading && !entry()}>
            <LocalBusyIndicator label={t("sqlPage.loadingQuery")} />
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
                <Show when={entry.loading}>
                  <LocalBusyIndicator
                    size="sm"
                    label={t("sqlPage.loadingQuery")}
                  />
                </Show>
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

                <Show when={data().kind === "user-query"}>
                  <div class="ui-stack-sm">
                    <label class="ui-label" for="saved-query-name">
                      {t("sqlPage.queryName")}
                    </label>
                    <input
                      id="saved-query-name"
                      class="ui-input"
                      placeholder={t("sqlPage.untitledQuery")}
                      value={queryName()}
                      disabled={busy() !== null}
                      onInput={(event) =>
                        setQueryName(event.currentTarget.value)}
                    />
                  </div>
                </Show>

                <div class="ui-stack-sm">
                  <label class="ui-label" for="saved-query-sql">
                    {t("sqlPage.sql")}
                  </label>
                  <SqlQueryEditor
                    id="saved-query-sql"
                    value={sqlInput()}
                    onChange={setSqlInput}
                    schema={buildSqlSchema((forms() || []) as Form[])}
                    disabled={data().kind !== "user-query" || busy() !== null}
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
                      <For each={normalized().variables}>
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
              </>
            )}
          </Match>
        </Switch>

        <Show when={actionError() && !deleteConfirmOpen()}>
          <p class="ui-alert ui-alert-error" role="alert">
            {actionError()}
          </p>
        </Show>
        <div class="flex flex-wrap items-center gap-3">
          <Show when={entry()?.kind === "user-query"}>
            <button
              type="button"
              class="btn primary"
              onClick={() => void handleSave()}
              disabled={!entry() || busy() !== null || !isDirty()}
              aria-busy={busy() === "save" || undefined}
            >
              {busy() === "save" ? t("sqlPage.saving") : t("common.save")}
            </button>
            <ActionIconBar
              label={t("sqlPage.detailActions")}
              actions={[{
                id: "delete-saved-sql",
                icon: "trash",
                label: t("sqlPage.delete"),
                danger: true,
                disabled: busy() !== null || !entry(),
                busy: busy() === "delete",
                onClick: openDeleteConfirm,
              }]}
            />
          </Show>
          <Show when={entry() && variableCount() === 0 && !isDirty()}>
            <button
              type="button"
              class="btn"
              onClick={handleRun}
            >
              {t("sqlPage.runQuery")}
            </button>
          </Show>
          <Show when={entry() && variableCount() > 0 && !isDirty()}>
            <A href={queryVariablesHref()} class="btn">
              {t("sqlPage.openVariables")}
            </A>
          </Show>
          <BackLink
            href={`/spaces/${encodeURIComponent(spaceId())}/sql`}
            label={t("sqlPage.backToSavedSql")}
          />
        </div>
        <Show when={isDirty()}>
          <p class="text-sm ui-muted">{t("sqlPage.unsavedChanges")}</p>
        </Show>
        <ConfirmDestructiveAction
          open={deleteConfirmOpen() && entry()?.kind === "user-query"}
          title={t("sqlPage.delete")}
          body={t("sqlPage.confirmDelete")}
          confirmLabel={t("sqlPage.delete")}
          busy={busy() === "delete"}
          error={actionError()}
          onConfirm={() => void handleDelete()}
          onClose={closeDeleteConfirm}
        />
      </section>
    </>
  );
}
