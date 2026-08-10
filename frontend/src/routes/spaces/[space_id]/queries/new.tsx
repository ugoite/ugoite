import { A, useNavigate, useParams } from "@solidjs/router";
import type { Diagnostic } from "@codemirror/lint";
import { createEffect, createSignal, For, Show } from "solid-js";
import { SqlQueryEditor } from "~/components";
import { formApi } from "~/lib/ugoite-client";
import {
  buildSqlSchema,
  buildSqlStarterQuery,
  normalizeSqlVariables,
} from "~/lib/sql";
import { sqlApi } from "~/lib/ugoite-client";
import { filterCreatableEntryForms } from "~/lib/metadata-forms";
import type { Form } from "~/lib/types";
import { createResource } from "~/lib/recoverable-resource";
import { t } from "~/lib/i18n";
import { formatUserFacingError } from "~/lib/user-facing-error";
import { spaceRoute } from "~/lib/space-shell-route";

export const route = spaceRoute({ navigation: "search", title: "sqlNew" });

export default function SpaceQueryCreateRoute() {
  const params = useParams<{ space_id: string }>();
  const navigate = useNavigate();
  const spaceId = () => params.space_id;
  const [queryName, setQueryName] = createSignal("");
  const [sqlInput, setSqlInput] = createSignal("");
  const [hasUserEditedSql, setHasUserEditedSql] = createSignal(false);
  const [diagnostics, setDiagnostics] = createSignal<Diagnostic[]>([]);
  const [error, setError] = createSignal<string | null>(null);
  const [isSaving, setIsSaving] = createSignal(false);

  const [forms] = createResource(async () => {
    return await formApi.list(spaceId());
  });

  createEffect(() => {
    if (hasUserEditedSql()) return;
    const relation = filterCreatableEntryForms(forms() ?? [])
      .find((form) => form.sql_relation?.trim())
      ?.sql_relation?.trim();
    if (relation) setSqlInput(buildSqlStarterQuery(relation));
  });

  const schema = () => buildSqlSchema((forms() || []) as Form[]);

  const handleSave = async () => {
    setError(null);
    const name = queryName().trim();
    const sql = sqlInput().trim();
    if (!sql) {
      setError(t("sqlPage.sqlRequired"));
      return;
    }
    const normalized = normalizeSqlVariables(sql);

    setIsSaving(true);
    try {
      await sqlApi.create(spaceId(), {
        name: name || null,
        kind: "user-query",
        metadata: name ? undefined : { generatedName: "untitled" },
        sql: normalized.sql,
        variables: normalized.variables,
      });
      navigate(`/spaces/${spaceId()}/search`);
    } catch (err) {
      setError(formatUserFacingError(err, "sqlPage.failedSave"));
    } finally {
      setIsSaving(false);
    }
  };

  return (
    <>
      <div class="screenHead">
        <div class="screenTitle">
          <div class="eyebrow">{t("sqlPage.searchSavedSql")}</div>
          <h1>{t("sqlPage.newSql")}</h1>
        </div>
      </div>
      <div class="settingsMain surface">
        <label class="ui-label" for="query-title">
          {t("sqlPage.queryName")}
        </label>
        <input
          id="query-title"
          class="ui-input"
          placeholder={t("sqlPage.untitledQuery")}
          value={queryName()}
          onInput={(e) => setQueryName(e.currentTarget.value)}
        />

        <div>
          <label class="ui-label mb-2" for="query-sql">
            {t("sqlPage.sql")}
          </label>
          <SqlQueryEditor
            id="query-sql"
            value={sqlInput()}
            onChange={(value) => {
              setHasUserEditedSql(true);
              setSqlInput(value);
            }}
            schema={schema()}
            onDiagnostics={setDiagnostics}
            disabled={isSaving()}
          />
        </div>

        <Show when={diagnostics().length > 0}>
          <ul class="text-sm ui-text-warning ui-stack-sm">
            <For each={diagnostics()}>
              {(diag) => <li>{diag.message}</li>}
            </For>
          </ul>
        </Show>
        <Show when={error()}>
          <p class="text-sm ui-text-danger">{error()}</p>
        </Show>

        <button
          type="button"
          class="btn primary"
          onClick={handleSave}
          disabled={isSaving()}
        >
          {isSaving() ? t("sqlPage.saving") : t("common.save")}
        </button>
      </div>
    </>
  );
}
