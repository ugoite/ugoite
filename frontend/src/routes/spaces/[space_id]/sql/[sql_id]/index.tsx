import { A, useNavigate, useParams } from "@solidjs/router";
import {
  createEffect,
  createMemo,
  createSignal,
  For,
  Match,
  Show,
  Switch,
} from "solid-js";
import { ActionIconBar } from "~/components/ActionIconBar";
import { BackLink } from "~/components/BackLink";
import { ConfirmDestructiveAction } from "~/components/ConfirmDestructiveAction";
import { LocalBusyIndicator } from "~/components/LocalBusyIndicator";
import { SQLWorkspace } from "~/components/SQLWorkspace";
import type { SQLWorkspaceAction } from "~/components/SQLWorkspace";
import { SqlStructureToolbar } from "~/components/SqlStructureToolbar";
import { formatDateLabel } from "~/lib/date-format";
import {
  applySqlVariableDefaultDrafts,
  buildSqlSchema,
  buildSqlStructureContexts,
  mergeSqlStructureVariables,
  normalizeSqlVariables,
  type SqlVariableDefaultDraft,
} from "~/lib/sql";
import { createSqlWorkspaceController } from "~/lib/sql-workspace";
import {
  formApi,
  renameSqlDocumentParameter,
  sqlApi,
} from "~/lib/ugoite-client";
import type { SqlStructureVariableType } from "~/lib/ugoite-client";
import { createResource } from "~/lib/recoverable-resource";
import { t } from "~/lib/i18n";
import { displaySqlName } from "~/lib/sql-metadata";
import { formatUserFacingError } from "~/lib/user-facing-error";
import { spaceRoute } from "~/lib/space-shell-route";
import type { Form, SqlVariable } from "~/lib/types";

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
  const [savedVariables, setSavedVariables] = createSignal<SqlVariable[]>([]);
  const [normalized, setNormalized] = createSignal({
    sql: "",
    variables: [] as SqlVariable[],
  });
  const [normalizationPending, setNormalizationPending] = createSignal(true);
  const [undeclaredArgument, setUndeclaredArgument] = createSignal<
    string | null
  >(null);
  const [defaultDrafts, setDefaultDrafts] = createSignal<
    Record<string, SqlVariableDefaultDraft>
  >({});
  const [revisionVariables, setRevisionVariables] = createSignal<SqlVariable[]>(
    [],
  );
  const [revisionId, setRevisionId] = createSignal<string | null>(null);
  const [busy, setBusy] = createSignal<"save" | "delete" | null>(null);
  const [actionError, setActionError] = createSignal<string | null>(null);
  const [deleteConfirmOpen, setDeleteConfirmOpen] = createSignal(false);
  const [structureActions, setStructureActions] = createSignal<
    readonly SQLWorkspaceAction[]
  >([]);
  let normalizationGeneration = 0;

  createEffect(() => {
    const current = entry();
    if (!current) return;
    setQueryName(current.name ?? "");
    setSqlInput(current.sql);
    setSavedName(current.name ?? "");
    setSavedSql(current.sql);
    const declared = current.variables ?? [];
    setSavedVariables([...declared]);
    setRevisionVariables([...declared]);
    setDefaultDrafts({});
    setRevisionId(current.revision_id);
  });

  createEffect(() => {
    const source = sqlInput();
    const existing = savedVariables();
    const generation = ++normalizationGeneration;
    setNormalizationPending(true);
    setNormalized({ sql: "", variables: [] });
    setUndeclaredArgument(null);
    void normalizeSqlVariables(source, existing).then((result) => {
      if (generation !== normalizationGeneration) return;
      setNormalized({ sql: result.sql, variables: result.variables });
      const declared = new Set(existing.map((variable) => variable.name));
      setUndeclaredArgument(
        result.variables.find((variable) => !declared.has(variable.name))
          ?.name ?? null,
      );
    }).catch(() => {
      if (generation === normalizationGeneration) {
        setNormalized({ sql: "", variables: [] });
      }
    }).finally(() => {
      if (generation === normalizationGeneration) {
        setNormalizationPending(false);
      }
    });
  });
  const declaredVariablesWithDefaults = createMemo(() =>
    applySqlVariableDefaultDrafts(savedVariables(), defaultDrafts())
  );
  const variablesWithDefaults = createMemo(() => {
    const declared = new Set(savedVariables().map((variable) => variable.name));
    return applySqlVariableDefaultDrafts(
      normalized().variables.filter((variable) => declared.has(variable.name)),
      defaultDrafts(),
    );
  });
  const workspaceVariables = createMemo(() => {
    const values = variablesWithDefaults();
    const declared = new Set(savedVariables().map((variable) => variable.name));
    const referenced = normalized().variables.filter((variable) =>
      declared.has(variable.name)
    );
    const resolved = values.invalid ? referenced : values.variables;
    return resolved.map((variable) => ({
      ...variable,
      required: variable.default === undefined,
    }));
  });
  const variableCount = createMemo(() => normalized().variables.length);
  const handleSqlChange = (sql: string) => {
    setSqlInput(sql);
  };
  const bindingDiagnostics = createMemo(() => {
    if (forms.state !== "ready") return [];
    const bindings = entry()?.metadata?.formBindings ?? [];
    const available = (forms() ?? []) as Form[];
    return bindings.map((binding) => ({
      ...binding,
      currentName: available.find((form) => form.id === binding.formId)?.name ??
        null,
    }));
  });
  const isDirty = createMemo(() => {
    const nextVariables = declaredVariablesWithDefaults();
    return queryName().trim() !== savedName() || sqlInput() !== savedSql() ||
      nextVariables.invalid !== undefined ||
      JSON.stringify(nextVariables.variables) !==
        JSON.stringify(revisionVariables());
  });
  const queryVariablesHref = () =>
    `/spaces/${encodeURIComponent(spaceId())}/sql/${
      encodeURIComponent(sqlId())
    }/variables`;

  const workspace = createSqlWorkspaceController({
    spaceId,
    sql: () => normalized().sql,
    variables: workspaceVariables,
    planParameters: "definition",
    planSavedSql: () => undefined,
    savedSql: () => {
      const current = entry();
      if (!current || sqlInput() !== savedSql()) return undefined;
      const types = (variables: readonly SqlVariable[]) =>
        variables.map(({ name, type }) => ({ name, type }));
      if (
        JSON.stringify(types(savedVariables())) !==
          JSON.stringify(types(revisionVariables()))
      ) return undefined;
      return {
        id: current.id,
        revisionId: revisionId() ?? current.revision_id,
      };
    },
  });

  const registerCreatedVariables = (
    added: readonly { name: string; type: SqlStructureVariableType }[],
  ) => {
    const existingTypes = new Map(
      savedVariables().map((variable) => [variable.name, variable.type]),
    );
    const replacedNames = added.filter(({ name, type }) =>
      existingTypes.has(name) && existingTypes.get(name) !== type
    ).map(({ name }) => name);
    if (replacedNames.length > 0) {
      setDefaultDrafts((current) => {
        const next = { ...current };
        for (const name of replacedNames) delete next[name];
        return next;
      });
    }
    setSavedVariables((current) => mergeSqlStructureVariables(current, added));
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
    setSavedVariables((current) =>
      current.map((item) => item.name === oldName ? variable : item)
    );
  };

  const removeArgument = (name: string) => {
    if (normalized().variables.some((variable) => variable.name === name)) {
      return false;
    }
    setSavedVariables((current) =>
      current.filter((item) => item.name !== name)
    );
    setDefaultDrafts((current) => {
      const next = { ...current };
      delete next[name];
      return next;
    });
  };

  const handleRun = () => {
    if (
      !entry() || isDirty() || normalizationPending() || variableCount() > 0
    ) return;
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
    const saveIdentity = JSON.stringify({
      sql: sqlInput(),
      arguments: savedVariables(),
      defaults: defaultDrafts(),
      revision: revisionId() ?? current.revision_id,
    });
    let normalizedQuery: Awaited<ReturnType<typeof normalizeSqlVariables>>;
    try {
      normalizedQuery = await normalizeSqlVariables(
        sqlInput(),
        savedVariables(),
      );
    } catch (error) {
      setActionError(formatUserFacingError(error, "sqlPage.failedSave"));
      return;
    }
    const sql = normalizedQuery.sql.trim();
    if (!sql) {
      setActionError(t("sqlPage.sqlRequired"));
      return;
    }
    const referenced = new Set(
      normalizedQuery.variables.map((variable) => variable.name),
    );
    const declared = new Set(savedVariables().map((variable) => variable.name));
    const undeclared = normalizedQuery.variables.find((variable) =>
      !declared.has(variable.name)
    );
    if (undeclared) {
      setActionError(
        t("sqlPage.undeclaredArgument", { name: undeclared.name }),
      );
      return;
    }
    const unused = savedVariables().filter((variable) =>
      !referenced.has(variable.name)
    );
    if (unused.length > 0) {
      setActionError(t("sqlPage.unusedArgument", { name: unused[0].name }));
      return;
    }
    const nextVariables = applySqlVariableDefaultDrafts(
      normalizedQuery.variables,
      defaultDrafts(),
    );
    if (nextVariables.invalid) {
      setActionError(t("sqlPage.defaultInvalid", nextVariables.invalid));
      return;
    }
    const currentPlan = workspace.plan();
    if (
      JSON.stringify({
          sql: sqlInput(),
          arguments: savedVariables(),
          defaults: defaultDrafts(),
          revision: revisionId() ?? entry()?.revision_id,
        }) !== saveIdentity ||
      currentPlan.status !== "success" ||
      !currentPlan.isCurrent
    ) {
      setActionError(currentPlan.error ?? t("sqlPage.planPending"));
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
        variables: nextVariables.variables,
        parent_revision_id: nextRevisionId,
      });
      setQueryName(name ?? "");
      setSqlInput(sql);
      setSavedName(name ?? "");
      setSavedSql(sql);
      setSavedVariables(nextVariables.variables);
      setRevisionVariables(nextVariables.variables);
      setDefaultDrafts({});
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
        <div class="screenHeadStart">
          <BackLink
            href={`/spaces/${encodeURIComponent(spaceId())}/sql`}
            label={t("sqlPage.backToSavedSql")}
          />
          <div class="ui-stack-sm">
            <Show
              when={entry()}
              fallback={<h1>{t("sqlPage.detail")}</h1>}
            >
              {(data) => (
                <h1>
                  {data().kind === "search-history"
                    ? displaySqlName(data())
                    : queryName().trim() || t("sqlPage.untitledQuery")}
                </h1>
              )}
            </Show>
          </div>
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

                <Show when={bindingDiagnostics().length > 0}>
                  <section
                    class="ui-stack-sm"
                    aria-label={t("sqlPage.formBindings")}
                  >
                    <h2 class="text-sm font-semibold">
                      {t("sqlPage.formBindings")}
                    </h2>
                    <ul class="list-disc space-y-1 pl-5 text-sm ui-muted">
                      <For each={bindingDiagnostics()}>
                        {(binding) => {
                          const [
                            technicalDetailsOpen,
                            setTechnicalDetailsOpen,
                          ] = createSignal(false);
                          return (
                            <li>
                              {binding.currentName === binding.name
                                ? t("sqlPage.boundForm", {
                                  name: binding.name,
                                })
                                : binding.currentName
                                ? t("sqlPage.renamedBoundForm", {
                                  savedName: binding.name,
                                  currentName: binding.currentName,
                                })
                                : t("sqlPage.missingBoundForm", {
                                  name: binding.name,
                                })}
                              <details
                                class="mt-1"
                                onToggle={(event) =>
                                  setTechnicalDetailsOpen(
                                    event.currentTarget.open,
                                  )}
                              >
                                <summary>
                                  {t("sqlPage.technicalDetails")}
                                </summary>
                                <Show when={technicalDetailsOpen()}>
                                  <dl>
                                    <dt>{t("sqlPage.formId")}</dt>
                                    <dd class="font-mono break-all">
                                      {binding.formId}
                                    </dd>
                                  </dl>
                                </Show>
                              </details>
                            </li>
                          );
                        }}
                      </For>
                    </ul>
                  </section>
                </Show>

                <SQLWorkspace
                  id="saved-sql-workspace"
                  sql={sqlInput()}
                  onSqlChange={data().kind === "user-query"
                    ? handleSqlChange
                    : undefined}
                  actions={data().kind === "user-query"
                    ? structureActions()
                    : []}
                  schema={buildSqlSchema((forms() || []) as Form[])}
                  readOnly={data().kind !== "user-query" || busy() !== null}
                  variables={workspaceVariables()}
                  parameterValues={workspace.parameterValues()}
                  onParameterChange={workspace.onParameterChange}
                  parametersReadOnly={data().kind !== "user-query" ||
                    busy() !== null}
                  planner={workspace.plan()}
                  previewDisabled={data().kind !== "user-query" ||
                    busy() !== null || normalizationPending() ||
                    undeclaredArgument() !== null ||
                    !normalized().sql.trim()}
                  onPreview={workspace.run}
                  onCount={workspace.count}
                  onPrevious={workspace.previous}
                  onNext={workspace.next}
                  preview={workspace.preview()}
                />
                <Show when={undeclaredArgument()}>
                  {(name) => (
                    <p class="ui-alert ui-alert-error" role="alert">
                      {t("sqlPage.undeclaredArgument", { name: name() })}
                    </p>
                  )}
                </Show>
                <Show when={data().kind === "user-query"}>
                  <SqlStructureToolbar
                    sql={sqlInput}
                    contexts={() =>
                      buildSqlStructureContexts(
                        (forms() || []) as Form[],
                        workspaceVariables(),
                        `saved-sql:${sqlId()}:${revisionId() ?? ""}`,
                      )}
                    onSqlChange={handleSqlChange}
                    onVariablesAdded={registerCreatedVariables}
                    argumentDefinitions={savedVariables}
                    onArgumentAdded={(variable) => {
                      if (
                        savedVariables().some((item) =>
                          item.name === variable.name
                        )
                      ) return false;
                      setSavedVariables((current) => [...current, variable]);
                    }}
                    onArgumentUpdated={updateArgument}
                    onArgumentRemoved={removeArgument}
                    argumentDefaults={defaultDrafts}
                    onArgumentDefaultsChange={setDefaultDrafts}
                    onActionsChange={setStructureActions}
                    disabled={() => busy() !== null ||
                      data().kind !== "user-query"}
                  />
                </Show>
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
              disabled={!entry() || busy() !== null || !isDirty() ||
                workspace.plan().status !== "success" ||
                !workspace.plan().isCurrent}
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
              {t("sqlPage.results")}
            </button>
          </Show>
          <Show when={entry() && variableCount() > 0 && !isDirty()}>
            <A href={queryVariablesHref()} class="btn">
              {t("sqlPage.openVariables")}
            </A>
          </Show>
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
