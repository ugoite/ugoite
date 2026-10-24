import { A, useNavigate, useParams } from "@solidjs/router";
import type { Diagnostic } from "@codemirror/lint";
import { createEffect, createMemo, createSignal, For, Show } from "solid-js";
import { BackLink } from "~/components/BackLink";
import { ButtonSpinner } from "~/components/ButtonSpinner";
import { SQLWorkspace, SQLWorkspaceActionBar } from "~/components/SQLWorkspace";
import type { SQLWorkspaceAction } from "~/components/SQLWorkspace";
import { SqlStructureToolbar } from "~/components/SqlStructureToolbar";
import {
  formApi,
  renameSqlDocumentParameter,
  sqlApi,
} from "~/lib/ugoite-client";
import type { SqlStructureVariableType } from "~/lib/ugoite-client";
import {
  applySqlVariableDefaultDrafts,
  buildSqlSchema,
  buildSqlStructureContexts,
  mergeSqlStructureVariables,
  normalizeSqlVariables,
} from "~/lib/sql";
import type { SqlVariableDefaultDraft } from "~/lib/sql";
import type { Form, SqlFormBinding, SqlVariable } from "~/lib/types";
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
  const [argumentDefinitions, setArgumentDefinitions] = createSignal<
    SqlVariable[]
  >([]);
  const [sqlVariables, setSqlVariables] = createSignal<SqlVariable[]>([]);
  const [formBindingGuards, setFormBindingGuards] = createSignal<
    SqlFormBinding[]
  >([]);
  const [normalizedSql, setNormalizedSql] = createSignal("");
  const [undeclaredArgument, setUndeclaredArgument] = createSignal<
    string | null
  >(null);
  const [normalizationPending, setNormalizationPending] = createSignal(true);
  const [structureActions, setStructureActions] = createSignal<
    readonly SQLWorkspaceAction[]
  >([]);

  const [forms] = createResource(async () => {
    return await formApi.list(spaceId());
  });

  const schema = () => buildSqlSchema((forms() || []) as Form[]);
  let normalizationGeneration = 0;
  createEffect(() => {
    const sql = sqlInput().trim();
    const existing = argumentDefinitions();
    const generation = ++normalizationGeneration;
    setNormalizationPending(true);
    setNormalizedSql("");
    setSqlVariables([]);
    setUndeclaredArgument(null);
    void normalizeSqlVariables(sql, existing).then((normalized) => {
      if (generation !== normalizationGeneration) return;
      setNormalizedSql(normalized.sql);
      const declared = new Set(existing.map((variable) => variable.name));
      setSqlVariables(
        normalized.variables.filter((variable) => declared.has(variable.name)),
      );
      setUndeclaredArgument(
        normalized.variables.find((variable) => !declared.has(variable.name))
          ?.name ?? null,
      );
    }).catch((error) => {
      if (generation !== normalizationGeneration) return;
      setNormalizedSql("");
      setSqlVariables([]);
    }).finally(() => {
      if (generation === normalizationGeneration) {
        setNormalizationPending(false);
      }
    });
  });
  const variables = sqlVariables;
  const workspaceVariables = createMemo(() => {
    const defaults = applySqlVariableDefaultDrafts(
      variables(),
      defaultDrafts(),
    );
    const resolved = defaults.invalid ? variables() : defaults.variables;
    return resolved.map((variable) => ({
      ...variable,
      required: variable.default === undefined,
    }));
  });
  const definitionError = createMemo(() => {
    if (normalizationPending()) return t("common.loading");
    const undeclared = undeclaredArgument();
    if (undeclared) {
      return t("sqlPage.undeclaredArgument", { name: undeclared });
    }
    const referenced = new Set(variables().map((variable) => variable.name));
    const unused = argumentDefinitions().find((variable) =>
      !referenced.has(variable.name)
    );
    if (unused) return t("sqlPage.unusedArgument", { name: unused.name });
    const defaults = applySqlVariableDefaultDrafts(
      argumentDefinitions(),
      defaultDrafts(),
    );
    return defaults.invalid
      ? t("sqlPage.defaultInvalid", defaults.invalid)
      : undefined;
  });
  const workspace = createSqlWorkspaceController({
    spaceId,
    sql: normalizedSql,
    variables: workspaceVariables,
    definitionError,
    planParameters: "definition",
    planSavedSql: () => undefined,
    formBindingGuards,
  });

  const handleSqlChange = (sql: string) => {
    setSqlInput(sql);
  };

  const registerCreatedVariables = (
    added: readonly { name: string; type: SqlStructureVariableType }[],
  ) => {
    setArgumentDefinitions((current) =>
      mergeSqlStructureVariables(current, added)
    );
  };

  const updateArgument = async (oldName: string, variable: SqlVariable) => {
    if (oldName !== variable.name) {
      const source = sqlInput();
      try {
        const document = await renameSqlDocumentParameter(
          (await normalizeSqlVariables(source)).sql,
          oldName,
          variable.name,
        );
        if (sqlInput() !== source) return false;
        setSqlInput(document.source);
      } catch {
        return false;
      }
      setDefaultDrafts((current) => {
        if (!current[oldName]) return current;
        const next = { ...current, [variable.name]: current[oldName] };
        delete next[oldName];
        return next;
      });
    }
    setArgumentDefinitions((current) =>
      current.map((item) => item.name === oldName ? variable : item)
    );
  };

  const removeArgument = (name: string) => {
    if (variables().some((variable) => variable.name === name)) return false;
    setArgumentDefinitions((current) =>
      current.filter((item) => item.name !== name)
    );
    setDefaultDrafts((current) => {
      const next = { ...current };
      delete next[name];
      return next;
    });
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
    const saveIdentity = JSON.stringify({
      sql: sqlInput(),
      arguments: argumentDefinitions(),
      defaults: defaultDrafts(),
    });
    if (!sql) {
      setError(t("sqlPage.sqlRequired"));
      return;
    }
    let normalized: Awaited<ReturnType<typeof normalizeSqlVariables>>;
    try {
      normalized = await normalizeSqlVariables(sql, argumentDefinitions());
    } catch (err) {
      setError(formatUserFacingError(err, "sqlPage.failedSave"));
      return;
    }
    const referenced = new Set(
      normalized.variables.map((variable) => variable.name),
    );
    const declared = new Set(
      argumentDefinitions().map((variable) => variable.name),
    );
    const undeclared = normalized.variables.find((variable) =>
      !declared.has(variable.name)
    );
    if (undeclared) {
      setError(t("sqlPage.undeclaredArgument", { name: undeclared.name }));
      return;
    }
    const unused = argumentDefinitions().filter((variable) =>
      !referenced.has(variable.name)
    );
    if (unused.length > 0) {
      setError(t("sqlPage.unusedArgument", { name: unused[0].name }));
      return;
    }
    const withTypes = normalized.variables;
    const defaults = applySqlVariableDefaultDrafts(
      withTypes,
      defaultDrafts(),
    );
    if (defaults.invalid) {
      setError(t("sqlPage.defaultInvalid", defaults.invalid));
      return;
    }

    const currentPlan = workspace.plan();
    if (
      JSON.stringify({
          sql: sqlInput(),
          arguments: argumentDefinitions(),
          defaults: defaultDrafts(),
        }) !== saveIdentity ||
      currentPlan.status !== "success" ||
      !currentPlan.isCurrent
    ) {
      setError(currentPlan.error ?? t("sqlPage.planPending"));
      return;
    }

    setIsSaving(true);
    try {
      await sqlApi.create(spaceId(), {
        name: name || null,
        kind: "user-query",
        metadata: !name || formBindingGuards().length
          ? {
            ...(!name ? { generatedName: "untitled" as const } : {}),
            ...(formBindingGuards().length
              ? { formBindings: formBindingGuards() }
              : {}),
          }
          : undefined,
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
          onPlanColumns={workspace.planColumns}
          formBindingGuards={formBindingGuards}
          scope={() => `new-sql:${spaceId()}`}
          sql={sqlInput}
          contexts={() =>
            buildSqlStructureContexts(
              (forms() || []) as Form[],
              workspaceVariables(),
              `new-sql:${spaceId()}`,
            )}
          onSqlChange={handleSqlChange}
          onFormBindingSelected={(selection) => {
            setSqlInput(selection.sql);
            setFormBindingGuards((current) => [
              ...current.filter((item) => item.name !== selection.name),
              { name: selection.name, formId: selection.form_id },
            ]);
          }}
          onVariablesAdded={registerCreatedVariables}
          argumentDefinitions={argumentDefinitions}
          onArgumentAdded={(variable) => {
            if (
              argumentDefinitions().some((item) => item.name === variable.name)
            ) {
              return false;
            }
            setArgumentDefinitions((current) => [...current, variable]);
          }}
          onArgumentUpdated={updateArgument}
          onArgumentRemoved={removeArgument}
          argumentDefaults={defaultDrafts}
          onArgumentDefaultsChange={setDefaultDrafts}
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
          planner={workspace.plan()}
          previewDisabled={isSaving() || Boolean(definitionError()) ||
            normalizationPending() ||
            undeclaredArgument() !== null ||
            !normalizedSql().trim()}
          onPreview={workspace.run}
          onCount={workspace.count}
          onPrevious={workspace.previous}
          onNext={workspace.next}
          preview={workspace.preview()}
        />

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
        <Show when={undeclaredArgument()}>
          {(name) => (
            <p class="ui-alert ui-alert-error" role="alert">
              {t("sqlPage.undeclaredArgument", { name: name() })}
            </p>
          )}
        </Show>
        <Show when={error()}>
          <p class="text-sm ui-text-danger">{error()}</p>
        </Show>

        <button
          type="button"
          class="btn primary"
          onClick={() => void handleSave()}
          disabled={isSaving() || workspace.plan().status !== "success" ||
            !workspace.plan().isCurrent}
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
