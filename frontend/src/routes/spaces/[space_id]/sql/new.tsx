import { A, useNavigate, useParams } from "@solidjs/router";
import type { Diagnostic } from "@codemirror/lint";
import { createMemo, createSignal, For, Show } from "solid-js";
import { BackLink } from "~/components/BackLink";
import { ButtonSpinner } from "~/components/ButtonSpinner";
import { SQLWorkspace, SQLWorkspaceActionBar } from "~/components/SQLWorkspace";
import type { SQLWorkspaceAction } from "~/components/SQLWorkspace";
import { SqlStructureToolbar } from "~/components/SqlStructureToolbar";
import { SqlVariableDefaultsEditor } from "~/components/SqlVariableDefaultsEditor";
import { formApi, sqlApi } from "~/lib/ugoite-client";
import type { SqlStructureVariableType } from "~/lib/ugoite-client";
import {
  applySqlVariableDefaultDrafts,
  buildSqlSchema,
  buildSqlStructureContexts,
  normalizeSqlVariables,
} from "~/lib/sql";
import type { SqlVariableDefaultDraft } from "~/lib/sql";
import type { Form } from "~/lib/types";
import { createResource } from "~/lib/recoverable-resource";
import { t } from "~/lib/i18n";
import { formatUserFacingError } from "~/lib/user-facing-error";
import { spaceRoute } from "~/lib/space-shell-route";
import { createSqlWorkspaceController } from "~/lib/sql-workspace";

export const route = spaceRoute({ navigation: "search" });

export default function SpaceQueryCreateRoute() {
  const params = useParams<{ space_id: string }>();
  const navigate = useNavigate();
  const spaceId = () => params.space_id;
  const [queryName, setQueryName] = createSignal("");
  const [sqlInput, setSqlInput] = createSignal("SELECT 1");
  const [diagnostics, setDiagnostics] = createSignal<Diagnostic[]>([]);
  const [error, setError] = createSignal<string | null>(null);
  const [isSaving, setIsSaving] = createSignal(false);
  const [defaultDrafts, setDefaultDrafts] = createSignal<
    Record<string, SqlVariableDefaultDraft>
  >({});
  const [createdVariableTypes, setCreatedVariableTypes] = createSignal<
    Record<string, SqlStructureVariableType>
  >({});
  const [structureActions, setStructureActions] = createSignal<
    readonly SQLWorkspaceAction[]
  >([]);

  const [forms] = createResource(async () => {
    return await formApi.list(spaceId());
  });

  const schema = () => buildSqlSchema((forms() || []) as Form[]);
  const variables = createMemo(() =>
    normalizeSqlVariables(
      sqlInput().trim(),
      Object.entries(createdVariableTypes()).map(([name, type]) => ({
        name,
        type,
        description: "",
      })),
    ).variables
  );
  const workspaceVariables = createMemo(() => {
    const defaults = applySqlVariableDefaultDrafts(
      variables(),
      defaultDrafts(),
    );
    return defaults.invalid ? variables() : defaults.variables;
  });
  const workspace = createSqlWorkspaceController({
    spaceId,
    sql: sqlInput,
    variables: workspaceVariables,
  });

  const handleSqlChange = (sql: string) => {
    setSqlInput(sql);
    const referenced = new Set(
      normalizeSqlVariables(sql).variables.map((variable) => variable.name),
    );
    setCreatedVariableTypes((current) =>
      Object.fromEntries(
        Object.entries(current).filter(([name]) => referenced.has(name)),
      )
    );
  };

  const registerCreatedVariables = (
    added: readonly { name: string; type: SqlStructureVariableType }[],
  ) => {
    setCreatedVariableTypes((current) => ({
      ...current,
      ...Object.fromEntries(added.map(({ name, type }) => [name, type])),
    }));
  };

  const removeCreatedVariables = (names: readonly string[]) => {
    const removed = new Set(names);
    setCreatedVariableTypes((current) =>
      Object.fromEntries(
        Object.entries(current).filter(([name]) => !removed.has(name)),
      )
    );
  };

  // Line/column rendering for editor diagnostics. Offsets come from the
  // CodeMirror lint gutter; the list mirrors them as text so the error
  // position survives without pointer access.
  const diagnosticLine = (offset: number): number =>
    sqlInput().slice(0, Math.max(0, offset)).split("\n").length;
  const diagnosticColumn = (offset: number): number => {
    const before = sqlInput().slice(0, Math.max(0, offset));
    return offset - (before.lastIndexOf("\n") + 1) + 1;
  };

  const handleSave = async () => {
    if (isSaving()) return;
    setError(null);
    const name = queryName().trim();
    const sql = sqlInput().trim();
    if (!sql) {
      setError(t("sqlPage.sqlRequired"));
      return;
    }
    const normalized = normalizeSqlVariables(sql);
    const withTypes = variables();
    const defaults = applySqlVariableDefaultDrafts(
      withTypes,
      defaultDrafts(),
    );
    if (defaults.invalid) {
      setError(t("sqlPage.defaultInvalid", defaults.invalid));
      return;
    }

    setIsSaving(true);
    try {
      await sqlApi.create(spaceId(), {
        name: name || null,
        kind: "user-query",
        metadata: name ? undefined : { generatedName: "untitled" },
        sql: normalized.sql,
        variables: defaults.variables,
      });
      navigate(`/spaces/${encodeURIComponent(spaceId())}/sql`);
    } catch (err) {
      setError(formatUserFacingError(err, "sqlPage.failedSave"));
    } finally {
      setIsSaving(false);
    }
  };

  return (
    <>
      <div class="screenHead">
        <div class="screenHeadStart">
          <BackLink
            href={`/spaces/${encodeURIComponent(spaceId())}/sql`}
            label={t("sqlPage.backToSavedSql")}
          />
          <div class="screenTitle">
            <h1>{t("sqlPage.newSql")}</h1>
          </div>
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

        <SqlStructureToolbar
          sql={sqlInput}
          contexts={() =>
            buildSqlStructureContexts(
              (forms() || []) as Form[],
              workspaceVariables(),
              `new-sql:${spaceId()}`,
            )}
          onSqlChange={handleSqlChange}
          onVariablesAdded={registerCreatedVariables}
          onVariablesRemoved={removeCreatedVariables}
          onActionsChange={setStructureActions}
          disabled={isSaving}
        />
        <SQLWorkspaceActionBar actions={structureActions()} />
        <SQLWorkspace
          id="new-sql-workspace"
          sql={sqlInput()}
          onSqlChange={handleSqlChange}
          actions={structureActions()}
          hideActions
          schema={schema()}
          variables={workspaceVariables()}
          parameterValues={workspace.parameterValues()}
          onParameterChange={workspace.onParameterChange}
          readOnly={isSaving()}
          onDiagnostics={setDiagnostics}
          previewDisabled={isSaving()}
          onPreview={workspace.run}
          onCount={workspace.count}
          onPrevious={workspace.previous}
          onNext={workspace.next}
          preview={workspace.preview()}
        />

        <Show when={variables().length > 0}>
          <SqlVariableDefaultsEditor
            idPrefix="new-sql"
            variables={variables()}
            defaults={defaultDrafts()}
            onDefaultsChange={setDefaultDrafts}
            disabled={isSaving()}
          />
        </Show>

        <Show when={diagnostics().length > 0}>
          <ul class="text-sm ui-text-warning ui-stack-sm">
            <For each={diagnostics()}>
              {(diag) => (
                <li>
                  {t("sqlPage.diagnosticAt", {
                    line: diagnosticLine(diag.from),
                    column: diagnosticColumn(diag.from),
                    message: diag.message,
                  })}
                </li>
              )}
            </For>
          </ul>
        </Show>
        <Show when={error()}>
          <p class="text-sm ui-text-danger">{error()}</p>
        </Show>

        <button
          type="button"
          class="btn primary"
          onClick={() => void handleSave()}
          disabled={isSaving()}
          aria-busy={isSaving() || undefined}
        >
          <Show when={isSaving()}>
            <ButtonSpinner />
          </Show>
          {t("common.save")}
        </button>
      </div>
    </>
  );
}
