import { useLocation, useNavigate } from "@solidjs/router";
import { createEffect, createResource, createSignal, Show } from "solid-js";
import { BackLink } from "~/components/BackLink";
import { CompositionGraphStudio } from "~/components/CompositionGraphStudio";
import type { SQLWorkspacePreview } from "~/components/SQLWorkspace";
import { LocalBusyIndicator } from "~/components/LocalBusyIndicator";
import {
  compositionApi,
  type CompositionEvaluationResponse,
  type CompositionGraphPreviewResponse,
} from "~/lib/composition-api";
import {
  compositionBindingDiagnostics,
  type CompositionBindingProvider,
  type CompositionBindingSession,
  type CompositionComponent,
  type CompositionFormChoice,
  compositionGraphDocumentDraft,
  type CompositionGraphDraft,
  type CompositionLiteral,
  type CompositionQueryDefinition,
  type CompositionQueryReturn,
  type CompositionResolvedBinding,
  type CompositionSavedSqlChoice,
  type CompositionSavedSqlRevisionRef,
  type CompositionScalarDescriptor,
  type CompositionVariableNode,
  createCompositionGraphDraft,
  sameResolvedQueryMeaning,
} from "~/lib/composition-variable-binding";
import { t } from "~/lib/i18n";
import {
  compositionCanonicalizeErrorMessage,
  compositionSaveErrorMessage,
} from "~/lib/composition-save-error";
import {
  clearPendingCompositionSaveAttempt,
  getPendingCompositionSaveAttempt,
  markPendingCompositionSaveAttemptUncertain,
  type PendingCompositionSaveAttempt,
  stagePendingCompositionSaveAttempt,
} from "~/lib/composition-save-attempt";
import {
  changedCompositionQueryIds,
  dependentCompositionQueryIds,
} from "~/lib/composition-evaluation";
import { spaceCompositionRevisionPath } from "~/lib/space-path";
import { authApi, formApi, sqlApi } from "~/lib/ugoite-client";
import type { Form, SqlEntry } from "~/lib/types";
import { buildSqlSchema } from "~/lib/sql";
import type { SqlResultColumnType } from "~/lib/types";

export type CompositionStudioSaveMode =
  | { kind: "create" }
  | { kind: "update"; compositionId: string; baseRevisionId: string };

export interface CompositionStudioProps {
  spaceId: string;
  initialDraft?: CompositionGraphDraft;
  initialSavedSqlBinding?: CompositionSavedSqlRevisionRef;
  saveMode: CompositionStudioSaveMode;
  backHref: string;
  backLabel: string;
}

const quoteIdentifier = (identifier: string) =>
  `"${identifier.replaceAll('"', '""')}"`;

const draftQuery = (
  kind: "inline_sql" | "saved_sql",
  details: Record<string, unknown>,
  arguments_: Array<{ name: string; variable: string }> = [],
  returns: CompositionQueryReturn = { kind: "table", columns: [] },
): CompositionVariableNode & { kind: "query" } => ({
  id: "__binding_candidate",
  kind: "query",
  returns,
  query: {
    kind,
    ...details,
    arguments: arguments_,
    form_bindings: [],
    schema_fingerprint: "",
  } as CompositionQueryDefinition,
});

const inputDefaultValue = (node: CompositionVariableNode): unknown => {
  if (node.kind === "input" && node.default) {
    return node.default.kind === "null" ? null : node.default.value;
  }
  return undefined;
};

const scalarFor = (
  type: string,
  nullable = false,
): CompositionScalarDescriptor => ({
  scalar_type: type as CompositionScalarDescriptor["scalar_type"],
  nullable,
});

const formChoice = (form: Form): CompositionFormChoice | undefined => {
  if (!form.id || !form.sql_relation?.trim()) return undefined;
  return {
    id: form.id,
    name: form.name,
    relation: form.sql_relation,
    fields: Object.entries(form.fields ?? {}).flatMap(([name, field]) => {
      const type = field.type;
      if (!type) return [];
      return [{
        name: field.sql_column?.trim() || name,
        type: type as CompositionScalarDescriptor["scalar_type"],
      }];
    }),
  };
};

const savedSqlChoice = (
  entry: SqlEntry,
): CompositionSavedSqlChoice | undefined => {
  if (entry.kind !== "user-query" || !entry.revision_id) return undefined;
  const types = new Set([
    "string",
    "boolean",
    "integer",
    "float",
    "date",
    "timestamp",
  ]);
  if (entry.variables.some((argument) => !types.has(argument.type))) {
    return undefined;
  }
  return {
    entry_id: entry.id,
    revision_id: entry.revision_id,
    name: entry.name?.trim() || entry.id,
    arguments: entry.variables.map(({ name, type }) => ({
      name,
      type: type as CompositionSavedSqlChoice["arguments"][number]["type"],
    })),
  };
};

export function CompositionStudio(props: CompositionStudioProps) {
  const navigate = useNavigate();
  const location = useLocation();
  const [draft, setDraft] = createSignal<CompositionGraphDraft>(
    props.initialDraft ?? createCompositionGraphDraft(),
  );
  const [saving, setSaving] = createSignal(false);
  const routeIdentity = () => ({
    spaceId: props.spaceId,
    routePath: location.pathname,
  });
  let pendingSave: PendingCompositionSaveAttempt | undefined =
    getPendingCompositionSaveAttempt(routeIdentity())?.attempt;
  const [saveRetryAvailable, setSaveRetryAvailable] = createSignal(
    Boolean(getPendingCompositionSaveAttempt(routeIdentity())),
  );
  const [saveError, setSaveError] = createSignal<string | null>(null);
  const [invalidEditorVariables, setInvalidEditorVariables] = createSignal(
    new Set<string>(),
  );
  const editorValid = () => invalidEditorVariables().size === 0;
  const [saveGeneration, setSaveGeneration] = createSignal(0);
  const [queryPreviews, setQueryPreviews] = createSignal<
    Record<string, SQLWorkspacePreview>
  >({});
  const [queryEvaluations, setQueryEvaluations] = createSignal<
    Record<string, {
      evaluationId: string;
      yaml: string;
      next?: string;
      pageNumber: number;
    }>
  >({});
  const runGenerations = new Map<string, number>();
  const [formsResource] = createResource(
    () => props.spaceId,
    (spaceId) => formApi.list(spaceId),
  );
  const [sqlResource] = createResource(
    () => props.spaceId,
    (spaceId) => sqlApi.list(spaceId),
  );
  const forms = () =>
    (formsResource() ?? []).flatMap((form) => {
      const choice = formChoice(form);
      return choice ? [choice] : [];
    });
  const savedSql = () =>
    (sqlResource() ?? []).flatMap((entry) => {
      const choice = savedSqlChoice(entry);
      return choice ? [choice] : [];
    });
  const savedSqlForPicker = () => {
    const exact = props.initialSavedSqlBinding;
    return exact
      ? savedSql().filter((entry) =>
        entry.entry_id === exact.entry_id &&
        entry.revision_id === exact.revision_id
      )
      : savedSql();
  };
  const savedSqlSeedUnavailable = () =>
    Boolean(
      props.initialSavedSqlBinding && !sqlResource.loading &&
        !sqlResource.error && savedSqlForPicker().length === 0,
    );
  let seedNameApplied = false;
  createEffect(() => {
    const exact = props.initialSavedSqlBinding;
    if (
      !exact || seedNameApplied || sqlResource.loading || sqlResource.error
    ) return;
    const entry = (sqlResource() ?? []).find((candidate) =>
      candidate.id === exact.entry_id &&
      candidate.revision_id === exact.revision_id
    );
    if (!entry) return;
    seedNameApplied = true;
    const name = entry.name?.trim() || entry.id;
    setDraft((current) => current.name.trim() ? current : { ...current, name });
  });
  const sqlSchema = () => buildSqlSchema(formsResource() ?? []);

  const literalValue = (literal: { kind: string; value?: unknown }) =>
    literal.kind === "null" ? null : literal.value;

  const workspacePage = (
    evaluation: CompositionEvaluationResponse,
    variableId: string,
    pageNumber: number,
  ): SQLWorkspacePreview => {
    const evaluated = evaluation.query_results[variableId];
    if (!evaluation.ok || !evaluated) {
      const diagnostic = evaluation.query_diagnostics?.find((entry) =>
        entry.variable_id === variableId
      ) ?? evaluation.diagnostics?.find((entry) =>
        entry.variable_id === variableId
      );
      return {
        status: "error",
        isCurrent: true,
        error: diagnostic?.code || t("composition.queryFailed"),
      };
    }
    const result = evaluated.value;
    const columns = result.kind === "table"
      ? result.columns.map((column) => column.name)
      : ["value"];
    const columnTypes = result.kind === "table"
      ? result.columns.map((column) =>
        column.value_type.scalar_type as SqlResultColumnType
      )
      : [result.value.kind as SqlResultColumnType];
    const literalRows = result.kind === "table"
      ? result.rows
      : [[result.value]];
    const rows = literalRows.map((row) =>
      row.map((literal) => literalValue(literal))
    );
    const exactIntegerRows = literalRows.map((row) =>
      row.map((literal, index) =>
        columnTypes[index] === "integer" && literal.kind === "integer"
          ? String(literal.value)
          : null
      )
    );
    return {
      status: "success",
      isCurrent: true,
      page: {
        columns,
        columnTypes,
        rows,
        exactIntegerRows,
        pageIdentity: `${evaluation.evaluation_id}:${variableId}:${pageNumber}`,
        pageNumber,
        canNext: evaluated.has_more && Boolean(evaluated.next),
      },
    };
  };

  const runQuery = async (variableId: string, cursor?: string) => {
    const generation = (runGenerations.get(variableId) ?? 0) + 1;
    runGenerations.set(variableId, generation);
    const previous = queryEvaluations()[variableId];
    const isContinuation = Boolean(cursor && previous);
    setQueryPreviews((current) => ({
      ...current,
      [variableId]: { status: "loading", isCurrent: false },
    }));
    try {
      let yaml: string;
      let evaluationId: string;
      let pageNumber: number;
      if (isContinuation && previous) {
        ({ yaml, evaluationId } = {
          yaml: previous.yaml,
          evaluationId: previous.evaluationId,
        });
        pageNumber = previous.pageNumber + 1;
      } else {
        yaml = (await compositionApi.canonicalizeDocument(
          compositionGraphDocumentDraft(draft()),
        )).canonical_yaml;
        evaluationId = crypto.randomUUID();
        pageNumber = 1;
      }
      const evaluation = await compositionApi.evaluate(props.spaceId, {
        evaluation_id: evaluationId,
        target: { kind: "draft", yaml },
        ...(props.saveMode.kind === "update"
          ? {
            base_entry_id: props.saveMode.compositionId,
            base_revision_id: props.saveMode.baseRevisionId,
          }
          : {}),
        variables: {},
        ...(!cursor ? { requested_queries: [variableId] } : {}),
        ...(cursor ? { page: { variable_id: variableId, cursor } } : {}),
      });
      if (runGenerations.get(variableId) !== generation) return;
      const evaluated = evaluation.query_results[variableId];
      if (!isContinuation) {
        setQueryEvaluations((current) => ({
          ...current,
          [variableId]: {
            evaluationId,
            yaml,
            next: evaluated?.next,
            pageNumber,
          },
        }));
      } else {
        setQueryEvaluations((current) => ({
          ...current,
          [variableId]: {
            evaluationId,
            yaml,
            next: evaluated?.next,
            pageNumber,
          },
        }));
      }
      setQueryPreviews((current) => ({
        ...current,
        [variableId]: workspacePage(evaluation, variableId, pageNumber),
      }));
    } catch {
      if (runGenerations.get(variableId) === generation) {
        setQueryPreviews((current) => ({
          ...current,
          [variableId]: {
            status: "error",
            isCurrent: true,
            error: t("composition.queryFailed"),
          },
        }));
      }
    }
  };
  const session: CompositionBindingSession = {
    session_id: crypto.randomUUID(),
    space_id: props.spaceId,
    document_revision: props.saveMode.kind === "update"
      ? props.saveMode.baseRevisionId
      : null,
    principal_generation: "not-captured",
  };
  const getPrincipalGeneration = async () => {
    const authSession = await authApi.getSession();
    if (!authSession.authenticated) return "anonymous";
    const accountId = authSession.account?.account_id.trim();
    if (!accountId) throw new Error("Authenticated account is unavailable");
    return `account:${accountId}`;
  };
  const isSessionCurrent = async (value: CompositionBindingSession) => {
    if (
      value.session_id !== session.session_id ||
      value.space_id !== props.spaceId ||
      value.document_revision !== session.document_revision
    ) return false;
    try {
      return value.principal_generation === await getPrincipalGeneration();
    } catch {
      return false;
    }
  };

  const previewCandidate = async (
    candidate: CompositionGraphDraft,
  ): Promise<CompositionGraphPreviewResponse> => {
    const document = compositionGraphDocumentDraft(candidate);
    return await compositionApi.previewGraph(
      props.spaceId,
      JSON.stringify(document),
      Object.fromEntries(candidate.variables.flatMap((node) => {
        if (node.kind !== "input") return [];
        const value = inputDefaultValue(node);
        return value === undefined ? [] : [[node.id, value]];
      })),
      props.saveMode.kind === "update"
        ? {
          entryId: props.saveMode.compositionId,
          revisionId: props.saveMode.baseRevisionId,
        }
        : undefined,
    );
  };

  const bindFromPreview = async (
    candidate: CompositionGraphDraft,
    candidateNode: CompositionVariableNode & { kind: "query" },
    expectedKind: CompositionQueryReturn["kind"],
  ): Promise<CompositionResolvedBinding> => {
    const componentKind = expectedKind === "table" ? "table" : "metric";
    const componentIds = new Set(candidate.components.map(({ id }) => id));
    let componentIndex = 0;
    const previewComponents: CompositionComponent[] = [];
    const makePreviewComponent = (): CompositionComponent => {
      let id = `__binding_preview_${componentIndex++}`;
      while (componentIds.has(id)) id = `__binding_preview_${componentIndex++}`;
      componentIds.add(id);
      return { id, kind: componentKind, variable: candidateNode.id };
    };
    const rows = candidate.layout.rows.length > 0
      ? candidate.layout.rows
      : [{ id: "__binding_preview_row", items: [] }];
    let placedProbe = false;
    const previewRows = rows.map((row) => {
      if (row.items.length > 0 && placedProbe) return row;
      const component = makePreviewComponent();
      previewComponents.push(component);
      placedProbe = true;
      return {
        ...row,
        items: [...row.items, {
          kind: "component" as const,
          component: component.id,
        }],
      };
    });
    // Empty rows all receive a probe while existing nonempty rows stay intact.
    const withCandidate: CompositionGraphDraft = {
      ...candidate,
      variables: [...candidate.variables, candidateNode],
      components: [...candidate.components, ...previewComponents],
      layout: { ...candidate.layout, rows: previewRows },
    };
    const response = await previewCandidate(withCandidate);
    const planned = response.plan?.queries.find((query) =>
      query.variable_id === candidateNode.id
    );
    if (!response.ok || !planned) throw new Error("query preview rejected");
    if (expectedKind && planned.returns.kind !== expectedKind) {
      throw new Error("query result shape does not fit component");
    }
    if (
      planned.returns.kind === "scalar" &&
      planned.scalar_cardinality !== "exactly_one_row"
    ) throw new Error("scalar query must return exactly one row");
    const variable: CompositionVariableNode & { kind: "query" } = {
      ...candidateNode,
      returns: planned.returns,
      query: {
        ...candidateNode.query,
        form_bindings: planned.form_bindings,
        schema_fingerprint: planned.schema_fingerprint,
      },
    };
    const match = candidate.variables.find((node) =>
      sameResolvedQueryMeaning(node, variable)
    );
    return {
      variable,
      ...(match ? { reuse_variable_id: match.id } : {}),
    };
  };

  const provider: CompositionBindingProvider = {
    resolveForm: async ({ form_id, component, aggregate, draft: current }) => {
      const form = forms().find((candidate) => candidate.id === form_id);
      if (!form) throw new Error("Form unavailable");
      const relation = quoteIdentifier(form.relation);
      let sql = `SELECT * FROM ${relation}`;
      let returns: CompositionQueryReturn = { kind: "table", columns: [] };
      if (component === "metric") {
        if (!aggregate) throw new Error("Aggregate selection required");
        if (aggregate.kind === "count") {
          sql = `SELECT COUNT(*) AS value FROM ${relation}`;
        } else {
          if (!aggregate.column) throw new Error("Aggregate column required");
          const field = form.fields.find((candidate) =>
            candidate.name === aggregate.column
          );
          if (!field) throw new Error("Aggregate field unavailable");
          sql = `SELECT ${aggregate.kind.toUpperCase()}(${
            quoteIdentifier(field.name)
          }) AS value FROM ${relation}`;
        }
        returns = {
          kind: "scalar",
          value_type: scalarFor(
            "integer",
            aggregate.kind === "sum" || aggregate.kind === "avg",
          ),
        };
      }
      const id = `query_${
        form.id.replace(/[^a-zA-Z0-9_-]/g, "_")
      }_${component}`;
      const candidate = draftQuery("inline_sql", { sql }, [], returns);
      const uniqueId = (base: string) => {
        let value = base;
        let suffix = 2;
        while (current.variables.some((node) => node.id === value)) {
          value = `${base}_${suffix++}`;
        }
        return value;
      };
      const resolved = await bindFromPreview(current, {
        ...candidate,
        id: uniqueId(id),
      }, component === "table" ? "table" : "scalar");
      return resolved;
    },
    prepareSavedSql: async (
      {
        entry_id,
        revision_id,
        component,
        arguments: arguments_,
        draft: current,
      },
    ) => {
      const entry = savedSql().find((candidate) =>
        candidate.entry_id === entry_id && candidate.revision_id === revision_id
      );
      if (!entry) throw new Error("Saved SQL revision unavailable");
      if (arguments_.some((argument) => !argument.variable)) {
        throw new Error("Saved SQL arguments must be explicitly bound");
      }
      const args = entry.arguments.map((argument) =>
        arguments_.find((item) => item.name === argument.name) ??
          { name: argument.name, variable: "" }
      );
      const base = draftQuery("saved_sql", {
        entry_id,
        revision_id,
      }, args);
      const withId = (column?: string) => ({
        ...base,
        id: (() => {
          const stem = `query_${entry_id.replace(/[^a-zA-Z0-9_-]/g, "_")}_${
            column ?? (component === "table" ? "table" : "value")
          }`;
          let value = stem;
          let suffix = 2;
          while (current.variables.some((node) => node.id === value)) {
            value = `${stem}_${suffix++}`;
          }
          return value;
        })(),
        query: { ...base.query, ...(column ? { column } : {}) },
      });
      if (component === "table") {
        try {
          return { table: await bindFromPreview(current, withId(), "table") };
        } catch (tableError) {
          try {
            return {
              scalars: [{
                column: "value",
                binding: await bindFromPreview(current, withId(), "scalar"),
              }],
            };
          } catch {
            throw tableError;
          }
        }
      }
      let descriptor: CompositionResolvedBinding;
      try {
        descriptor = await bindFromPreview(current, withId(), "scalar");
      } catch {
        // The Saved SQL revision can return either a scalar or a table. Probe
        // each shape through the same typed preview contract before offering
        // a Metric binding; no shape is inferred from SQL text in the client.
        descriptor = await bindFromPreview(current, withId(), "table");
      }
      if (descriptor.variable.returns.kind !== "table") {
        if (component === "table") throw new Error("Saved SQL is not a table");
        if (descriptor.variable.returns.kind !== "scalar") {
          throw new Error("Saved SQL has no scalar result");
        }
        return {
          scalars: [{ column: "value", binding: descriptor }],
        };
      }
      if (component === "table") {
        return { table: descriptor };
      }
      const columns = descriptor.variable.returns.columns;
      const scalars = await Promise.all(columns.map(async (column) => {
        const scalarQuery = withId(column.name);
        scalarQuery.returns = {
          kind: "scalar",
          value_type: scalarFor(
            column.value_type.scalar_type,
            column.value_type.nullable,
          ),
        };
        return {
          column: column.name,
          binding: await bindFromPreview(current, scalarQuery, "scalar"),
        };
      }));
      return { scalars };
    },
    inspectSavedSql: async ({
      session: _session,
      entry_id,
      revision_id,
      component,
      draft: current,
    }) => {
      const entry = savedSql().find((candidate) =>
        candidate.entry_id === entry_id && candidate.revision_id === revision_id
      );
      if (!entry) throw new Error("Saved SQL revision unavailable");
      const added: CompositionVariableNode[] = [];
      const arguments_ = entry.arguments.map((argument, index) => {
        const stem = `__saved_sql_probe_${index}`;
        let variable = stem;
        let suffix = 2;
        while (
          current.variables.some((node) => node.id === variable) ||
          added.some((node) => node.id === variable)
        ) variable = `${stem}_${suffix++}`;
        const sample: Record<string, unknown> = {
          string: "",
          boolean: false,
          integer: 0,
          float: 0,
          date: "1970-01-01",
          timestamp: "1970-01-01T00:00:00Z",
        };
        added.push({
          id: variable,
          kind: "constant",
          value_type: { scalar_type: argument.type, nullable: false },
          value: {
            kind: argument.type,
            value: sample[argument.type],
          } as CompositionLiteral,
        });
        return { name: argument.name, variable };
      });
      return await provider.prepareSavedSql({
        session: _session,
        entry_id,
        revision_id,
        component,
        arguments: arguments_,
        draft: { ...current, variables: [...current.variables, ...added] },
      });
    },
  };

  const saveRequest = async (
    attempt: PendingCompositionSaveAttempt,
    isRetry: boolean,
  ) => {
    stagePendingCompositionSaveAttempt(attempt, undefined);
    try {
      const saved = await compositionApi.save(
        attempt.spaceId,
        attempt.yaml,
        attempt.idempotencyKey,
        props.saveMode.kind === "update"
          ? {
            compositionId: props.saveMode.compositionId,
            baseRevisionId: props.saveMode.baseRevisionId,
          }
          : undefined,
      );
      if (attempt.routePath !== location.pathname) {
        markPendingCompositionSaveAttemptUncertain(attempt, undefined);
        pendingSave = attempt;
        setSaveRetryAvailable(true);
        return;
      }
      clearPendingCompositionSaveAttempt(attempt, "completed", undefined);
      pendingSave = undefined;
      setSaveRetryAvailable(false);
      navigate(spaceCompositionRevisionPath(
        props.spaceId,
        saved.composition_id,
        saved.revision_id,
      ));
    } catch (error) {
      const outcome = error && typeof error === "object" &&
          "mutationOutcome" in error
        ? (error as { mutationOutcome?: unknown }).mutationOutcome
        : "unknown";
      const status = error && typeof error === "object" && "status" in error
        ? (error as { status?: unknown }).status
        : undefined;
      const retryWasDenied = isRetry && (status === 401 || status === 403);
      if (outcome === "rejected" && !retryWasDenied) {
        clearPendingCompositionSaveAttempt(attempt, "rejected", undefined);
        pendingSave = undefined;
        setSaveRetryAvailable(false);
        setSaveError(compositionSaveErrorMessage(error));
      } else {
        markPendingCompositionSaveAttemptUncertain(attempt, undefined);
        pendingSave = attempt;
        setSaveRetryAvailable(true);
        setSaveError(t("composition.saveFailed"));
      }
    }
  };

  const save = async () => {
    if (saving() || saveRetryAvailable() || !editorValid()) return;
    const current = draft();
    if (!current.name.trim() || current.components.length === 0) {
      setSaveError(t("composition.studioInvalidDocument"));
      return;
    }
    const localDiagnostics = compositionBindingDiagnostics(current);
    if (localDiagnostics.length > 0) {
      setSaveError(t("composition.studioInvalidDocument"));
      return;
    }
    const generation = saveGeneration() + 1;
    setSaveGeneration(generation);
    setSaving(true);
    setSaveError(null);
    try {
      const checked = await previewCandidate(current);
      if (!checked.ok || !checked.plan) {
        throw new Error("Composition preview failed");
      }
      const queryMap = new Map(checked.plan.queries.map((query) => [
        query.variable_id,
        query,
      ]));
      const verified: CompositionGraphDraft = {
        ...current,
        variables: current.variables.map((node) => {
          if (node.kind !== "query") return node;
          const query = queryMap.get(node.id);
          if (!query) throw new Error("Query descriptor missing");
          return {
            ...node,
            returns: query.returns,
            query: {
              ...node.query,
              form_bindings: query.form_bindings,
              schema_fingerprint: query.schema_fingerprint,
            },
          };
        }),
      };
      const canonical = await compositionApi.canonicalizeDocument(
        compositionGraphDocumentDraft(verified),
      );
      if (generation !== saveGeneration()) return;
      const attempt: PendingCompositionSaveAttempt = {
        ...routeIdentity(),
        name: current.name.trim(),
        yaml: canonical.canonical_yaml,
        idempotencyKey: crypto.randomUUID(),
      };
      pendingSave = attempt;
      await saveRequest(attempt, false);
    } catch (error) {
      if (generation === saveGeneration()) {
        setSaveError(compositionCanonicalizeErrorMessage(error));
      }
    } finally {
      if (generation === saveGeneration()) setSaving(false);
    }
  };

  const retrySave = async () => {
    const attempt = pendingSave;
    if (saving() || !saveRetryAvailable() || !attempt) return;
    const generation = saveGeneration() + 1;
    setSaveGeneration(generation);
    setSaving(true);
    setSaveError(null);
    try {
      await saveRequest(attempt, true);
    } finally {
      if (generation === saveGeneration()) setSaving(false);
    }
  };

  const markQueryPreviewsStale = (invalidated: readonly string[]) => {
    if (!invalidated.length) return;
    for (const variableId of invalidated) {
      runGenerations.set(
        variableId,
        (runGenerations.get(variableId) ?? 0) + 1,
      );
    }
    setQueryPreviews((current) => {
      const updated = { ...current };
      for (const variableId of invalidated) {
        updated[variableId] = { status: "idle", isCurrent: false };
      }
      return updated;
    });
    setQueryEvaluations((current) => {
      const updated = { ...current };
      for (const variableId of invalidated) delete updated[variableId];
      return updated;
    });
  };

  const invalidateQueryPreviews = (variableIds: ReadonlySet<string>) => {
    markQueryPreviewsStale(
      dependentCompositionQueryIds(draft(), variableIds),
    );
  };

  const onEditorValidityChange = (
    variableId: string,
    valid: boolean | undefined,
  ) => {
    if (valid === false) invalidateQueryPreviews(new Set([variableId]));
    setInvalidEditorVariables((current) => {
      const next = new Set(current);
      if (valid === false) next.add(variableId);
      else next.delete(variableId);
      return next;
    });
  };

  const onDraftChange = (next: CompositionGraphDraft) => {
    if (saving() || saveRetryAvailable()) return;
    const invalidated = changedCompositionQueryIds(draft(), next);
    markQueryPreviewsStale(invalidated);
    const remainingEditorIds = new Set(
      next.variables
        .filter((node) => node.kind === "constant" || node.kind === "input")
        .map((node) => node.id),
    );
    setInvalidEditorVariables((current) =>
      new Set([...current].filter((id) => remainingEditorIds.has(id)))
    );
    setDraft(next);
    setSaveError(null);
  };

  return (
    <div
      class="compositionGraphStudioRoot"
      aria-busy={saving() ? "true" : "false"}
    >
      <div
        class="compositionGraphStudioEditor"
        inert={saving() || saveRetryAvailable()}
      >
        <header class="screenHead">
          <div class="screenHeadStart">
            <BackLink href={props.backHref} label={props.backLabel} />
            <input
              class="ui-input compositionGraphName"
              aria-label={t("composition.name")}
              value={draft().name}
              onInput={(event) =>
                onDraftChange({
                  ...draft(),
                  name: event.currentTarget.value,
                })}
            />
          </div>
        </header>
        <Show when={formsResource.loading || sqlResource.loading}>
          <LocalBusyIndicator label={t("common.loading")} />
        </Show>
        <CompositionGraphStudio
          draft={draft()}
          forms={forms()}
          savedSql={savedSqlForPicker()}
          savedSqlLoading={sqlResource.loading}
          savedSqlSeedUnavailable={savedSqlSeedUnavailable()}
          initialSavedSqlBinding={props.initialSavedSqlBinding}
          provider={provider}
          session={session}
          getPrincipalGeneration={getPrincipalGeneration}
          isSessionCurrent={isSessionCurrent}
          sqlSchema={sqlSchema()}
          sqlForms={formsResource() ?? []}
          previewForQuery={(variableId) =>
            queryPreviews()[variableId] ?? {
              status: "idle",
              isCurrent: false,
            }}
          onRunQuery={(variableId) => void runQuery(variableId)}
          onNextQuery={(variableId) => {
            const evaluation = queryEvaluations()[variableId];
            if (evaluation?.next) void runQuery(variableId, evaluation.next);
          }}
          onDraftChange={onDraftChange}
          onQueryChange={(variableId, sql) => {
            onDraftChange({
              ...draft(),
              variables: draft().variables.map((node) =>
                node.id === variableId && node.kind === "query" &&
                  node.query.kind === "inline_sql"
                  ? { ...node, query: { ...node.query, sql } }
                  : node
              ),
            });
          }}
          onSave={save}
          saveDisabled={!editorValid() || !draft().name.trim() ||
            draft().components.length === 0 || saveRetryAvailable()}
          saving={saving()}
          onEditorValidityChange={onEditorValidityChange}
        />
      </div>
      <Show when={saveError()}>
        <p class="ui-text-danger" role="alert">{saveError()}</p>
      </Show>
      <Show when={saveRetryAvailable() && !saving()}>
        <button
          class="ui-button ui-button-secondary"
          type="button"
          onClick={() => void retrySave()}
        >
          {t("composition.retry")}
        </button>
      </Show>
    </div>
  );
}
