import { A, useNavigate, useParams } from "@solidjs/router";
import type { Diagnostic } from "@codemirror/lint";
import { createEffect, createMemo, createSignal, For, Show } from "solid-js";
import { BackLink } from "~/components/BackLink";
import { ButtonSpinner } from "~/components/ButtonSpinner";
import { SQLWorkspace } from "~/components/SQLWorkspace";
import { SqlArgumentDefinitionsEditor } from "~/components/SqlArgumentDefinitionsEditor";
import { SqlFormSourceControl } from "~/components/SqlFormSourceControl";
import {
  formApi,
  renameSqlDocumentParameter,
  sqlApi,
} from "~/lib/ugoite-client";
import {
  applySqlVariableDefaultDrafts,
  buildSqlSchema,
  normalizeSqlVariables,
  type SqlVariableDefaultDraft,
} from "~/lib/sql";
import type { Form, SqlFormBinding, SqlVariable } from "~/lib/types";
import { createResource } from "~/lib/recoverable-resource";
import { t } from "~/lib/i18n";
import { formatUserFacingError } from "~/lib/user-facing-error";
import { spaceRoute } from "~/lib/space-shell-route";
import { createSqlWorkspaceController } from "~/lib/sql-workspace";
import {
  replaceSqlFormBindingGuard,
  retainReferencedSqlFormBindingGuards,
} from "~/lib/form-binding-guards";

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
  const [formGuardPruningPending, setFormGuardPruningPending] = createSignal(
    false,
  );
  const [forms] = createResource(async () => {
    return await formApi.list(spaceId());
  });

  const schema = () => buildSqlSchema((forms() || []) as Form[]);
  const workspaceVariables = createMemo(() => {
    const defaults = applySqlVariableDefaultDrafts(
      sqlVariables(),
      defaultDrafts(),
    );
    const resolved = defaults.invalid ? sqlVariables() : defaults.variables;
    return resolved.map((variable) => ({
      ...variable,
      required: variable.default === undefined,
    }));
  });
  let normalizationGeneration = 0;
  let formGuardGeneration = 0;
  createEffect(() => {
    const sql = sqlInput().trim();
    const existing = argumentDefinitions();
    const generation = ++normalizationGeneration;
    setNormalizationPending(true);
    setNormalizedSql("");
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
  const definitionError = createMemo(() => {
    if (formGuardPruningPending() || normalizationPending()) {
      return t("common.loading");
    }
    const undeclared = undeclaredArgument();
    if (undeclared) {
      return t("sqlPage.undeclaredArgument", { name: undeclared });
    }
    const referenced = new Set(sqlVariables().map((variable) => variable.name));
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
    return true;
  };

  const removeArgument = (name: string) => {
    if (sqlVariables().some((variable) => variable.name === name)) return false;
    setArgumentDefinitions((current) =>
      current.filter((item) => item.name !== name)
    );
    setDefaultDrafts((current) => {
      const next = { ...current };
      delete next[name];
      return next;
    });
    return true;
  };

  const pruneFormGuardsForSql = (sql: string, selected?: SqlFormBinding) => {
    const generation = ++formGuardGeneration;
    setFormGuardPruningPending(true);
    const current = formBindingGuards();
    const pruning = selected
      ? replaceSqlFormBindingGuard(sql, current, selected)
      : retainReferencedSqlFormBindingGuards(sql, current);
    void pruning.then((retained) => {
      if (generation !== formGuardGeneration || sqlInput() !== sql) return;
      setFormBindingGuards(retained);
      setFormGuardPruningPending(false);
    });
  };

  const handleSqlChange = (sql: string) => {
    setFormGuardPruningPending(true);
    setSqlInput(sql);
    pruneFormGuardsForSql(sql);
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
      setError(
        t("sqlPage.undeclaredArgument", {
          name: undeclared.name,
        }),
      );
      return;
    }
    const unused = argumentDefinitions().find((variable) =>
      !referenced.has(variable.name)
    );
    if (unused) {
      setError(t("sqlPage.unusedArgument", { name: unused.name }));
      return;
    }
    const variablesWithDefaults = applySqlVariableDefaultDrafts(
      normalized.variables,
      defaultDrafts(),
    );
    if (variablesWithDefaults.invalid) {
      setError(t("sqlPage.defaultInvalid", variablesWithDefaults.invalid));
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
        variables: variablesWithDefaults.variables,
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

        <SqlFormSourceControl
          formBindingGuards={formBindingGuards}
          scope={() => `new-sql:${spaceId()}`}
          sql={sqlInput}
          forms={() => (forms() || []) as Form[]}
          onFormBindingSelected={(selection) => {
            setFormGuardPruningPending(true);
            setSqlInput(selection.sql);
            pruneFormGuardsForSql(selection.sql, {
              name: selection.name,
              formId: selection.form_id,
            });
          }}
          disabled={isSaving}
        />
        <SQLWorkspace
          id="new-sql-workspace"
          sql={sqlInput()}
          onSqlChange={handleSqlChange}
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
        <Show
          when={!normalizationPending() &&
            (undeclaredArgument() !== null || argumentDefinitions().length > 0)}
        >
          <SqlArgumentDefinitionsEditor
            idPrefix="new-sql-parameter"
            variables={argumentDefinitions()}
            referencedNames={sqlVariables().map((variable) => variable.name)}
            defaults={defaultDrafts()}
            disabled={isSaving()}
            onAdd={(variable) => {
              if (
                argumentDefinitions().some((item) => item.name === variable.name)
              ) return false;
              setArgumentDefinitions((current) => [...current, variable]);
            }}
            onUpdate={updateArgument}
            onRemove={removeArgument}
            onDefaultsChange={setDefaultDrafts}
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
