import { A, useNavigate, useParams } from "@solidjs/router";
import type { Diagnostic } from "@codemirror/lint";
import { createEffect, createMemo, createSignal, For, Show } from "solid-js";
import { BackLink } from "~/components/BackLink";
import { ButtonSpinner } from "~/components/ButtonSpinner";
import { SQLWorkspace } from "~/components/SQLWorkspace";
import type { SQLWorkspaceAction } from "~/components/SQLWorkspace";
import { SqlStructureToolbar } from "~/components/SqlStructureToolbar";
import { SqlVariableDefaultsEditor } from "~/components/SqlVariableDefaultsEditor";
import { formApi } from "~/lib/ugoite-client";
import {
  applySqlVariableDefaultDrafts,
  buildSqlAggregateQuery,
  buildSqlSchema,
  buildSqlStarterQuery,
  buildSqlStructureContexts,
  normalizeSqlVariables,
  sqlAggregateFieldOptions,
  sqlFilterFieldOptions,
} from "~/lib/sql";
import type { SqlAggregateOperation, SqlVariableDefaultDraft } from "~/lib/sql";
import { sqlApi } from "~/lib/ugoite-client";
import { filterCreatableEntryForms } from "~/lib/metadata-forms";
import type { Form, SqlVariable } from "~/lib/types";
import { createResource } from "~/lib/recoverable-resource";
import { t } from "~/lib/i18n";
import { formatUserFacingError } from "~/lib/user-facing-error";
import { spaceRoute } from "~/lib/space-shell-route";
import { createSqlWorkspaceController } from "~/lib/sql-workspace";
import { buildSqlLineDiff } from "~/lib/sql-diff";
import type { SqlLineDiffEntry } from "~/lib/sql-diff";

export const route = spaceRoute({ navigation: "search" });

type SqlVariableMetadata = Pick<SqlVariable, "name" | "type" | "description">;

type SqlVariableMetadataChange = {
  name: string;
  current?: Omit<SqlVariableMetadata, "name">;
  generated?: Omit<SqlVariableMetadata, "name">;
};

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
  const [selectedFormId, setSelectedFormId] = createSignal("");
  const [aggregateOperation, setAggregateOperation] = createSignal<
    SqlAggregateOperation
  >("count-rows");
  const [aggregateFieldKey, setAggregateFieldKey] = createSignal("");
  const [filterFieldKey, setFilterFieldKey] = createSignal("");
  const [generatedParameter, setGeneratedParameter] = createSignal<{
    name: "filter_value";
    type: string;
    description: string;
  }>();
  const [defaultDrafts, setDefaultDrafts] = createSignal<
    Record<string, SqlVariableDefaultDraft>
  >({});
  const [structureActions, setStructureActions] = createSignal<
    readonly SQLWorkspaceAction[]
  >([]);
  const [replacementProposal, setReplacementProposal] = createSignal<
    {
      kind: "form" | "aggregate";
      baseSql: string;
      sql: string;
      sqlDiff: SqlLineDiffEntry[];
      variableMetadataChanges: SqlVariableMetadataChange[];
      parameter?: {
        name: "filter_value";
        type: string;
        description: string;
      };
    } | null
  >(null);

  const [forms] = createResource(async () => {
    return await formApi.list(spaceId());
  });

  const selectableForms = createMemo(() =>
    filterCreatableEntryForms((forms() ?? []) as Form[])
      .filter((form) => Boolean(form.id && form.name.trim()))
  );

  const selectedForm = createMemo(() =>
    selectableForms().find((form) => form.id === selectedFormId())
  );
  const aggregateFields = createMemo(() =>
    sqlAggregateFieldOptions(selectedForm(), aggregateOperation())
  );
  const filterFields = createMemo(() => sqlFilterFieldOptions(selectedForm()));

  createEffect(() => {
    const available = selectableForms();
    const selected = available.find((form) => form.id === selectedFormId()) ??
      available[0];
    if (!selected) return;
    if (selected.id !== selectedFormId()) setSelectedFormId(selected.id!);
    if (!hasUserEditedSql()) setSqlInput(buildSqlStarterQuery(selected.name));
  });

  createEffect(() => {
    const availableAggregateFields = aggregateFields();
    if (
      !availableAggregateFields.some((field) =>
        field.key === aggregateFieldKey()
      )
    ) {
      setAggregateFieldKey(availableAggregateFields[0]?.key ?? "");
    }
    if (!filterFields().some((field) => field.key === filterFieldKey())) {
      setFilterFieldKey("");
    }
  });

  const schema = () => buildSqlSchema((forms() || []) as Form[]);
  const variables = createMemo(() => {
    const normalized = normalizeSqlVariables(sqlInput().trim());
    const generated = generatedParameter();
    return normalized.variables.map((variable) =>
      generated && variable.name === generated.name
        ? { ...variable, ...generated }
        : variable
    );
  });
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

  const applySqlReplacement = (
    kind: "form" | "aggregate",
    sql: string,
    parameter?: {
      name: "filter_value";
      type: string;
      description: string;
    },
  ) => {
    const currentVariables = variables().map(({ name, type, description }) => ({
      name,
      type,
      description,
    }));
    const generatedVariables = normalizeSqlVariables(sql).variables.map((
      variable,
    ) => ({
      name: variable.name,
      type: variable.type,
      description: variable.description,
    }));
    if (parameter) {
      const generatedVariable = generatedVariables.find((variable) =>
        variable.name === parameter.name
      );
      if (generatedVariable) {
        generatedVariable.type = parameter.type;
        generatedVariable.description = parameter.description;
      }
    }
    const currentByName = new Map(
      currentVariables.map((variable) => [variable.name, variable]),
    );
    const generatedByName = new Map(
      generatedVariables.map((variable) => [variable.name, variable]),
    );
    const variableMetadataChanges = Array.from(
      new Set([...currentByName.keys(), ...generatedByName.keys()]),
    ).sort().flatMap((name) => {
      const current = currentByName.get(name);
      const generated = generatedByName.get(name);
      if (
        current && generated && current.type === generated.type &&
        current.description === generated.description
      ) return [];
      return [{
        name,
        ...(current
          ? {
            current: { type: current.type, description: current.description },
          }
          : {}),
        ...(generated
          ? {
            generated: {
              type: generated.type,
              description: generated.description,
            },
          }
          : {}),
      }];
    });
    if (
      hasUserEditedSql() &&
      (sqlInput() !== sql || variableMetadataChanges.length > 0)
    ) {
      setReplacementProposal({
        kind,
        baseSql: sqlInput(),
        sql,
        sqlDiff: buildSqlLineDiff(sqlInput(), sql),
        variableMetadataChanges,
        parameter,
      });
      return;
    }
    setReplacementProposal(null);
    setSqlInput(sql);
    setGeneratedParameter(parameter);
    setHasUserEditedSql(hasUserEditedSql() || kind === "aggregate");
  };

  const acceptSqlReplacement = () => {
    if (isSaving()) return;
    const proposal = replacementProposal();
    if (!proposal || sqlInput() !== proposal.baseSql) {
      setReplacementProposal(null);
      return;
    }
    setSqlInput(proposal.sql);
    setGeneratedParameter(proposal.parameter);
    setHasUserEditedSql(true);
    setReplacementProposal(null);
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

        <div class="ui-stack-sm">
          <label class="ui-label" for="query-form">
            {t("sqlPage.formForQuery")}
          </label>
          <div class="flex flex-wrap items-center gap-2">
            <select
              id="query-form"
              class="ui-input"
              value={selectedFormId()}
              onChange={(event) => {
                setReplacementProposal(null);
                setSelectedFormId(event.currentTarget.value);
              }}
              disabled={isSaving() || selectableForms().length === 0}
            >
              <For each={selectableForms()}>
                {(form) => <option value={form.id}>{form.name}</option>}
              </For>
            </select>
            <button
              type="button"
              class="btn secondary"
              onClick={() => {
                const selected = selectableForms().find((form) =>
                  form.id === selectedFormId()
                );
                if (selected) {
                  applySqlReplacement(
                    "form",
                    buildSqlStarterQuery(selected.name),
                  );
                }
              }}
              disabled={isSaving() || !selectedFormId()}
            >
              {t("sqlPage.generateFromForm")}
            </button>
          </div>
        </div>

        <div class="ui-stack-sm">
          <label class="ui-label" for="query-aggregate">
            {t("sqlPage.aggregate")}
          </label>
          <select
            id="query-aggregate"
            class="ui-input"
            value={aggregateOperation()}
            onChange={(event) => {
              setReplacementProposal(null);
              setAggregateOperation(
                event.currentTarget.value as SqlAggregateOperation,
              );
            }}
            disabled={isSaving() || !selectedForm()}
          >
            <option value="count-rows">{t("sqlPage.countRows")}</option>
            <option value="count-values">{t("sqlPage.countValues")}</option>
            <option value="sum">{t("sqlPage.sum")}</option>
            <option value="average">{t("sqlPage.average")}</option>
            <option value="minimum">{t("sqlPage.minimum")}</option>
            <option value="maximum">{t("sqlPage.maximum")}</option>
          </select>

          <Show when={aggregateOperation() !== "count-rows"}>
            <label class="ui-label" for="query-aggregate-field">
              {t("sqlPage.aggregateField")}
            </label>
            <select
              id="query-aggregate-field"
              class="ui-input"
              value={aggregateFieldKey()}
              onChange={(event) => {
                setReplacementProposal(null);
                setAggregateFieldKey(event.currentTarget.value);
              }}
              disabled={isSaving() || aggregateFields().length === 0}
            >
              <For each={aggregateFields()}>
                {(field) => <option value={field.key}>{field.label}</option>}
              </For>
            </select>
          </Show>

          <label class="ui-label" for="query-filter-field">
            {t("sqlPage.filterField")}
          </label>
          <select
            id="query-filter-field"
            class="ui-input"
            value={filterFieldKey()}
            onChange={(event) => {
              setReplacementProposal(null);
              setFilterFieldKey(event.currentTarget.value);
            }}
            disabled={isSaving() || !selectedForm()}
          >
            <option value="">{t("sqlPage.noFilter")}</option>
            <For each={filterFields()}>
              {(field) => <option value={field.key}>{field.label}</option>}
            </For>
          </select>
          <button
            type="button"
            class="btn secondary"
            onClick={() => {
              const form = selectedForm();
              if (!form) return;
              setError(null);
              try {
                const query = buildSqlAggregateQuery(
                  form,
                  aggregateOperation(),
                  aggregateFieldKey(),
                  filterFieldKey(),
                );
                applySqlReplacement(
                  "aggregate",
                  query.sql,
                  query.parameter,
                );
              } catch {
                setError(t("sqlPage.aggregateFieldUnavailable"));
              }
            }}
            disabled={isSaving() || !selectedForm() || (
              aggregateOperation() !== "count-rows" &&
              aggregateFields().length === 0
            )}
          >
            {t("sqlPage.generateAggregate")}
          </button>
        </div>

        <Show when={replacementProposal()}>
          {(proposal) => (
            <section
              class="ui-stack-sm"
              role="region"
              aria-label={t("sqlPage.generatedSqlPreview")}
            >
              <h2 aria-live="polite">
                {proposal().kind === "form"
                  ? t("sqlPage.generatedFormSql")
                  : t("sqlPage.generatedAggregateSql")}
              </h2>
              <pre
                class="max-w-full overflow-x-auto"
                tabIndex={0}
                aria-label={t("sqlPage.generatedSqlDiff")}
              >
                <For each={proposal().sqlDiff}>
                  {(line) => (
                    <span class="block whitespace-pre">
                      <span class="ui-sr-only">
                        {line.kind === "removed"
                          ? t("sqlPage.sqlDiffRemovedLine", { line: line.line })
                          : line.kind === "added"
                          ? t("sqlPage.sqlDiffAddedLine", { line: line.line })
                          : line.kind === "omitted-changes"
                          ? t("sqlPage.sqlDiffOmittedChanges", {
                            removed: line.removedCount ?? 0,
                            added: line.addedCount ?? 0,
                          })
                          : line.kind === "preview-limit"
                          ? t("sqlPage.sqlDiffPreviewLimit")
                          : line.kind === "omitted-range"
                          ? t("sqlPage.sqlDiffOmittedRange")
                          : line.kind === "omitted"
                          ? t("sqlPage.sqlDiffOmittedLines", {
                            count: line.omittedCount ?? 0,
                          })
                          : line.kind === "unchanged"
                          ? t("sqlPage.sqlUnchanged")
                          : t("sqlPage.sqlDiffContextLine", { line: line.line })}
                      </span>
                      <span
                        aria-hidden="true"
                        class={line.kind === "removed"
                          ? "ui-text-danger"
                          : line.kind === "added"
                          ? "ui-text-success"
                          : ""}
                      >
                        {line.kind === "removed"
                          ? "− "
                          : line.kind === "added"
                          ? "+ "
                          : line.kind === "omitted" ||
                              line.kind === "omitted-changes" ||
                              line.kind === "omitted-range" ||
                              line.kind === "preview-limit"
                          ? "⋯ "
                          : line.kind === "unchanged"
                          ? ""
                          : "  "}
                        {line.kind === "omitted-changes"
                          ? t("sqlPage.sqlDiffOmittedChanges", {
                            removed: line.removedCount ?? 0,
                            added: line.addedCount ?? 0,
                          })
                          : line.kind === "preview-limit"
                          ? t("sqlPage.sqlDiffPreviewLimit")
                          : line.kind === "omitted-range"
                          ? t("sqlPage.sqlDiffOmittedRange")
                          : line.kind === "omitted"
                          ? t("sqlPage.sqlDiffOmittedLines", {
                            count: line.omittedCount ?? 0,
                          })
                          : line.kind === "unchanged"
                          ? t("sqlPage.sqlUnchanged")
                          : line.line || " "}
                      </span>
                    </span>
                  )}
                </For>
              </pre>
              <Show
                when={proposal().sqlDiff.some((line) =>
                  line.kind === "omitted-changes" ||
                  line.kind === "omitted-range" ||
                  line.kind === "preview-limit"
                )}
              >
                <details>
                  <summary class="flex min-h-11 cursor-pointer items-center">
                    {t("sqlPage.fullGeneratedSql")}
                  </summary>
                  <pre class="max-w-full overflow-x-auto" tabIndex={0}>
                    {proposal().sql}
                  </pre>
                </details>
              </Show>
              <For each={proposal().variableMetadataChanges}>
                {(change) => (
                  <p class="text-sm">
                    {change.current && change.generated
                      ? t("sqlPage.parameterMetadataChange", {
                        name: change.name,
                        currentType: change.current.type,
                        nextType: change.generated.type,
                        currentDescription: change.current.description ||
                          t("sqlPage.noFilter"),
                        nextDescription: change.generated.description ||
                          t("sqlPage.noFilter"),
                      })
                      : change.current
                      ? t("sqlPage.parameterMetadataRemoved", {
                        name: change.name,
                        type: change.current.type,
                        description: change.current.description ||
                          t("sqlPage.noFilter"),
                      })
                      : t("sqlPage.parameterMetadataAdded", {
                        name: change.name,
                        type: change.generated?.type ?? "",
                        description: change.generated?.description ||
                          t("sqlPage.noFilter"),
                      })}
                  </p>
                )}
              </For>
              <div class="flex flex-wrap items-center gap-2">
                <button
                  type="button"
                  class="btn primary"
                  disabled={isSaving()}
                  onClick={acceptSqlReplacement}
                >
                  {t("sqlPage.applyGeneratedSql")}
                </button>
                <button
                  type="button"
                  class="btn secondary"
                  disabled={isSaving()}
                  onClick={() => setReplacementProposal(null)}
                >
                  {t("sqlPage.keepCurrentSql")}
                </button>
              </div>
            </section>
          )}
        </Show>

        <SQLWorkspace
          id="new-sql-workspace"
          sql={sqlInput()}
          onSqlChange={(value, userInitiated = false) => {
            setReplacementProposal(null);
            if (userInitiated) setHasUserEditedSql(true);
            setSqlInput(value);
          }}
          actions={structureActions()}
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
        <SqlStructureToolbar
          sql={sqlInput}
          contexts={() =>
            buildSqlStructureContexts(
              (forms() || []) as Form[],
              workspaceVariables(),
              `new-sql:${spaceId()}:${selectedFormId()}`,
            )}
          onSqlChange={(value) => {
            setReplacementProposal(null);
            setHasUserEditedSql(true);
            setSqlInput(value);
          }}
          onActionsChange={setStructureActions}
          disabled={isSaving}
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
