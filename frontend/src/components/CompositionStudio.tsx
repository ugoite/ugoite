import { useLocation, useNavigate } from "@solidjs/router";
import {
  createEffect,
  createMemo,
  createResource,
  createSignal,
  onCleanup,
  Show,
  untrack,
} from "solid-js";
import { BackLink } from "~/components/BackLink";
import { CompositionGraphStudio } from "~/components/CompositionGraphStudio";
import type {
  SQLWorkspaceAction,
  SQLWorkspacePreview,
} from "~/components/SQLWorkspace";
import { LocalBusyIndicator } from "~/components/LocalBusyIndicator";
import {
  compositionApi,
  type CompositionEvaluationResponse,
  type CompositionGraphPreviewResponse,
} from "~/lib/composition-api";
import { stringifyCompositionDocument } from "~/lib/exact-integer";
import {
  CompositionBindingDiagnosticError,
  compositionBindingDiagnostics,
  type CompositionBindingProvider,
  type CompositionBindingSession,
  type CompositionComponent,
  type CompositionFormChoice,
  compositionGraphAuthoringDocument,
  compositionGraphDocumentDraft,
  type CompositionGraphDraft,
  type CompositionLiteral,
  type CompositionQueryDefinition,
  type CompositionQueryReturn,
  type CompositionResolvedBinding,
  type CompositionSavedSqlChoice,
  type CompositionSavedSqlRevisionRef,
  type CompositionScalarDescriptor,
  compositionTablePresentationDiagnostics,
  type CompositionVariableNode,
  createCompositionGraphDraft,
  sameResolvedQueryMeaning,
} from "~/lib/composition-variable-binding";
import { t } from "~/lib/i18n";
import { compositionQueryCountRequest } from "~/lib/composition-query-read";
import {
  compositionCanonicalizeErrorMessage,
  compositionSaveErrorMessage,
} from "~/lib/composition-save-error";
import {
  clearPendingCompositionSaveAttempt,
  type CompositionSaveTarget,
  getPendingCompositionSaveAttempt,
  markPendingCompositionSaveAttemptUncertain,
  type PendingCompositionSaveAttempt,
  sameCompositionSaveIdentity,
  stagePendingCompositionSaveAttempt,
  subscribeToPendingCompositionSaveAttempt,
} from "~/lib/composition-save-attempt";
import {
  changedCompositionQueryIds,
  dependentCompositionQueryIds,
} from "~/lib/composition-evaluation";
import { spaceCompositionRevisionPath } from "~/lib/space-path";
import {
  authApi,
  compositionFormQuerySql,
  formApi,
  sqlApi,
} from "~/lib/ugoite-client";
import type { Form, SqlEntry } from "~/lib/types";
import { buildSqlSchema } from "~/lib/sql";
import type { SqlResultColumnType } from "~/lib/types";

export type CompositionStudioSaveMode = CompositionSaveTarget;

export interface CompositionStudioProps {
  spaceId: string;
  initialDraft?: CompositionGraphDraft;
  initialSavedSqlBinding?: CompositionSavedSqlRevisionRef;
  saveMode: CompositionStudioSaveMode;
  backHref: string;
  backLabel: string;
}

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

const variableSlug = (value: string): string =>
  value.normalize("NFKD").toLowerCase()
    .replace(/\p{M}/gu, "")
    .replace(/[^a-z0-9]+/g, "_")
    .replace(/^_+|_+$/g, "");

const formScalarType = (
  type: string,
): CompositionScalarDescriptor["scalar_type"] | undefined => {
  switch (type) {
    case "string":
    case "boolean":
    case "integer":
    case "float":
    case "decimal":
    case "date":
    case "timestamp":
    case "json":
      return type;
    case "long":
      return "integer";
    case "markdown":
    case "sql":
    case "row_reference":
      return "string";
    case "timestamp_tz":
    case "timestamp_ns":
    case "timestamp_tz_ns":
      return "timestamp";
    case "double":
      return "float";
    default:
      return undefined;
  }
};

const formChoice = (form: Form): CompositionFormChoice | undefined => {
  if (!form.id || !form.sql_relation?.trim()) return undefined;
  return {
    id: form.id,
    name: form.name,
    relation: form.sql_relation,
    fields: Object.entries(form.fields ?? {}).map(([name, field]) => ({
      name: field.label?.trim() || name,
      column: field.sql_column?.trim() || name,
      type: field.type ? formScalarType(field.type) ?? field.type : "other",
    })),
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
  const routeIdentity = () => ({
    spaceId: props.spaceId,
    routePath: location.pathname,
    saveTarget: props.saveMode.kind === "update"
      ? { ...props.saveMode }
      : { kind: "create" as const },
  });
  const initialPending = getPendingCompositionSaveAttempt(routeIdentity());
  let pendingSave: PendingCompositionSaveAttempt | undefined = initialPending
    ?.attempt;
  const [saving, setSaving] = createSignal(
    initialPending?.state === "in_flight",
  );
  const [saveRetryAvailable, setSaveRetryAvailable] = createSignal(
    initialPending?.state === "uncertain",
  );
  let getSaveInputSnapshot: () =>
    | { variables: Record<string, unknown>; unset_variables: string[] }
    | undefined = () => ({ variables: {}, unset_variables: [] });
  createEffect(() => {
    const identity = routeIdentity();
    const stored = getPendingCompositionSaveAttempt(identity);
    pendingSave = stored?.attempt;
    setSaving(stored?.state === "in_flight");
    setSaveRetryAvailable(stored?.state === "uncertain");
    const unsubscribe = subscribeToPendingCompositionSaveAttempt(
      identity,
      (event) => {
        if (event.type === "pending") {
          pendingSave = event.stored.attempt;
          setSaving(event.stored.state === "in_flight");
          setSaveRetryAvailable(event.stored.state === "uncertain");
        } else if (pendingSave?.idempotencyKey === event.idempotencyKey) {
          pendingSave = undefined;
          setSaving(false);
          setSaveRetryAvailable(false);
        }
      },
    );
    onCleanup(unsubscribe);
  });
  const [saveError, setSaveError] = createSignal<string | null>(null);
  const [invalidEditorVariables, setInvalidEditorVariables] = createSignal(
    new Set<string>(),
  );
  const editorValid = () =>
    invalidEditorVariables().size === 0 &&
    compositionTablePresentationDiagnostics(draft()).length === 0;
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
      pages: Array<{ preview: SQLWorkspacePreview; next?: string }>;
      response: CompositionEvaluationResponse;
    }>
  >({});
  const runGenerations = new Map<string, number>();
  const runControllers = new Map<string, AbortController>();
  const countControllers = new Map<string, AbortController>();
  onCleanup(() => {
    for (
      const controller of [
        ...runControllers.values(),
        ...countControllers.values(),
      ]
    ) controller.abort();
    for (const [id, generation] of runGenerations) {
      runGenerations.set(id, generation + 1);
    }
  });
  let editorScope: string | undefined;
  createEffect(() => {
    const scope = JSON.stringify(routeIdentity());
    if (editorScope === undefined) {
      editorScope = scope;
      return;
    }
    if (editorScope === scope) return;
    editorScope = scope;
    untrack(() => {
      for (
        const controller of [
          ...runControllers.values(),
          ...countControllers.values(),
        ]
      ) controller.abort();
      for (const [id, generation] of runGenerations) {
        runGenerations.set(id, generation + 1);
      }
      setQueryPreviews({});
      setQueryEvaluations({});
      setInvalidEditorVariables(new Set());
      setSaveError(null);
      setDraft(
        props.saveMode.kind === "create"
          ? createCompositionGraphDraft()
          : props.initialDraft ?? createCompositionGraphDraft(),
      );
    });
  });
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
    if (!evaluated) {
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
        canPrevious: pageNumber > 1,
        canNext: evaluated.has_more && Boolean(evaluated.next),
      },
    };
  };

  const runQuery = async (variableId: string, cursor?: string) => {
    const generation = (runGenerations.get(variableId) ?? 0) + 1;
    runGenerations.set(variableId, generation);
    runControllers.get(variableId)?.abort();
    const controller = new AbortController();
    runControllers.set(variableId, controller);
    if (!cursor) countControllers.get(variableId)?.abort();
    const spaceId = props.spaceId;
    const scope = JSON.stringify(routeIdentity());
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
          compositionGraphAuthoringDocument(draft()),
        )).canonical_yaml;
        evaluationId = crypto.randomUUID();
        pageNumber = 1;
      }
      if (
        controller.signal.aborted ||
        runGenerations.get(variableId) !== generation
      ) return;
      const evaluation = await compositionApi.evaluate(spaceId, {
        evaluation_id: evaluationId,
        target: { kind: "draft", yaml },
        ...(props.saveMode.kind === "update"
          ? {
            base_entry_id: props.saveMode.compositionId,
            base_revision_id: props.saveMode.baseRevisionId,
          }
          : {}),
        variables: {},
        requested_queries: [variableId],
        ...(cursor ? { page: { variable_id: variableId, cursor } } : {}),
      }, controller.signal);
      if (
        controller.signal.aborted ||
        JSON.stringify(routeIdentity()) !== scope ||
        runGenerations.get(variableId) !== generation ||
        evaluation.evaluation_id !== evaluationId
      ) return;
      const evaluated = evaluation.query_results[variableId];
      const preview = workspacePage(evaluation, variableId, pageNumber);
      setQueryEvaluations((current) => ({
        ...current,
        [variableId]: {
          evaluationId,
          yaml,
          next: evaluated?.next,
          pageNumber,
          response: evaluation,
          pages: [
            ...(isContinuation
              ? previous?.pages.slice(0, pageNumber - 1) ?? []
              : []),
            { preview, next: evaluated?.next },
          ],
        },
      }));
      setQueryPreviews((current) => ({ ...current, [variableId]: preview }));
    } catch {
      if (
        !controller.signal.aborted &&
        runGenerations.get(variableId) === generation
      ) {
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
  const countQuery = async (variableId: string) => {
    const evaluated = queryEvaluations()[variableId];
    const preview = queryPreviews()[variableId];
    if (!evaluated || preview?.status !== "success" || !preview.isCurrent) {
      return;
    }
    countControllers.get(variableId)?.abort();
    const controller = new AbortController();
    countControllers.set(variableId, controller);
    const isCurrent = () =>
      !controller.signal.aborted &&
      queryEvaluations()[variableId]?.evaluationId === evaluated.evaluationId &&
      queryEvaluations()[variableId]?.yaml === evaluated.yaml;
    const updateCount = (count: SQLWorkspacePreview["count"]) => {
      if (isCurrent()) {
        setQueryPreviews((all) => ({
          ...all,
          [variableId]: { ...all[variableId], count },
        }));
      }
    };
    updateCount({ status: "loading" });
    try {
      const request = compositionQueryCountRequest(
        draft(),
        evaluated.response,
        variableId,
      );
      const value = await sqlApi.count(
        props.spaceId,
        request,
        controller.signal,
      );
      updateCount({ status: "success", value });
    } catch {
      updateCount({ status: "error", error: t("sqlPage.failedCount") });
    }
  };

  const previousQueryPage = (variableId: string) => {
    const current = queryEvaluations()[variableId];
    if (!current || current.pageNumber <= 1) return;
    const pageNumber = current.pageNumber - 1;
    const page = current.pages[pageNumber - 1];
    if (!page) return;
    runGenerations.set(variableId, (runGenerations.get(variableId) ?? 0) + 1);
    setQueryEvaluations((all) => ({
      ...all,
      [variableId]: { ...current, pageNumber, next: page.next },
    }));
    setQueryPreviews((all) => ({ ...all, [variableId]: page.preview }));
  };

  const session = createMemo<CompositionBindingSession>(() => {
    const identity = routeIdentity();
    return {
      session_id: crypto.randomUUID(),
      space_id: identity.spaceId,
      document_revision: props.saveMode.kind === "update"
        ? props.saveMode.baseRevisionId
        : null,
      principal_generation: "not-captured",
    };
  });
  const getPrincipalGeneration = async () => {
    const authSession = await authApi.getSession();
    if (!authSession.authenticated) return "anonymous";
    const accountId = authSession.account?.account_id.trim();
    if (!accountId) throw new Error("Authenticated account is unavailable");
    return `account:${accountId}`;
  };
  const isSessionCurrent = async (value: CompositionBindingSession) => {
    if (
      value.session_id !== session().session_id ||
      value.space_id !== props.spaceId ||
      value.document_revision !== session().document_revision
    ) return false;
    try {
      return value.principal_generation === await getPrincipalGeneration();
    } catch {
      return false;
    }
  };

  const previewCandidate = async (
    candidate: CompositionGraphDraft,
    signal?: AbortSignal,
  ): Promise<CompositionGraphPreviewResponse> => {
    const document = compositionGraphAuthoringDocument(candidate);
    return await compositionApi.previewGraph(
      props.spaceId,
      stringifyCompositionDocument(document),
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
      signal,
      candidate.variables.flatMap((node) =>
        node.kind === "input" &&
          node.value_type.scalar_type === "integer"
          ? [node.id]
          : []
      ),
    );
  };

  // Exact Saved SQL source is returned by the authorized graph resolver.
  // Do not consult sql.get (the moving head) to populate a pinned editor.
  const savedQueryIdentity = createMemo(() => {
    const current = draft();
    const queries = current.variables.filter((
      node,
    ): node is Extract<CompositionVariableNode, { kind: "query" }> =>
      node.kind === "query" && node.query.kind === "saved_sql"
    );
    if (!queries.length) return undefined;
    return JSON.stringify({
      spaceId: props.spaceId,
      base: props.saveMode,
      queries: queries.map((node) => ({ id: node.id, query: node.query })),
      argumentTypes: queries.flatMap((node) =>
        node.query.arguments.map((argument) => {
          const variable = current.variables.find((entry) =>
            entry.id === argument.variable
          );
          return variable?.kind === "query"
            ? variable.returns
            : variable?.value_type;
        })
      ),
    });
  });
  let metadataController: AbortController | undefined;
  onCleanup(() => metadataController?.abort());
  const [savedQueryMetadata] = createResource(
    savedQueryIdentity,
    async (identity) => {
      metadataController?.abort();
      const controller = new AbortController();
      metadataController = controller;
      const response = await previewCandidate(draft(), controller.signal);
      if (!response.ok || !response.plan) {
        throw new Error(
          t("composition.queryFailed"),
        );
      }
      return {
        identity,
        queries: response.plan.queries,
        bodies: Object.fromEntries(
          response.plan.queries.flatMap((query) =>
            query.sql_text === undefined
              ? []
              : [[query.variable_id, query.sql_text]]
          ),
        ),
      };
    },
  );
  const sqlBodies = () => {
    if (savedQueryMetadata.error) return {};
    const metadata = savedQueryMetadata();
    return metadata?.identity === savedQueryIdentity()
      ? metadata?.bodies ?? {}
      : {};
  };
  const actionsForQuery = (variableId: string): SQLWorkspaceAction[] => {
    const node = draft().variables.find((entry) => entry.id === variableId);
    if (node?.kind !== "query" || node.query.kind !== "saved_sql") return [];
    const metadata = savedQueryMetadata.error
      ? undefined
      : savedQueryMetadata();
    const query = metadata?.identity === savedQueryIdentity()
      ? metadata?.queries.find((entry) => entry.variable_id === variableId)
      : undefined;
    return [{
      kind: "copy",
      icon: "edit",
      label: t("composition.editInlineCopy"),
      disabled: !query?.portable_sql,
      disabledReason: !query?.portable_sql
        ? t("composition.queryFailed")
        : undefined,
      onActivate: () => {
        if (
          !query?.portable_sql || metadata?.identity !== savedQueryIdentity()
        ) return;
        onDraftChange({
          ...draft(),
          variables: draft().variables.map((entry) =>
            entry.id === variableId && entry.kind === "query"
              ? {
                ...entry,
                query: {
                  kind: "inline_sql",
                  sql: query.portable_sql!,
                  arguments: entry.query.arguments,
                },
              }
              : entry
          ),
        });
      },
    }];
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
    if (!response.ok || !planned) {
      const diagnostic = response.diagnostics?.find((candidate) =>
        candidate.variable_id === candidateNode.id &&
        (candidate.code === "query_unavailable" ||
          candidate.code === "query_schema_changed")
      );
      if (
        diagnostic?.code === "query_unavailable" ||
        diagnostic?.code === "query_schema_changed"
      ) {
        throw new CompositionBindingDiagnosticError(diagnostic.code);
      }
      throw new Error("query preview rejected");
    }
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
      if (component === "metric" && !aggregate) {
        throw new Error("Aggregate selection required");
      }
      if (
        aggregate && aggregate.kind !== "count" &&
        !form.fields.some((field) => field.column === aggregate.column)
      ) {
        throw new Error("Aggregate field unavailable");
      }
      const { sql } = await compositionFormQuerySql(
        form.name,
        component === "table"
          ? undefined
          : aggregate!.kind === "count"
          ? { kind: "count" }
          : { kind: aggregate!.kind, column: aggregate!.column! },
      );
      const returns: CompositionQueryReturn = component === "table"
        ? { kind: "table", columns: [] }
        : {
          kind: "scalar",
          value_type: scalarFor(
            "integer",
            aggregate!.kind === "sum" || aggregate!.kind === "avg",
          ),
        };
      const formSlug = variableSlug(form.name);
      const formPrefix = /^[a-z]/.test(formSlug)
        ? formSlug
        : formSlug
        ? `form_${formSlug}`
        : "form";
      const aggregateField = component === "metric" && aggregate?.column
        ? form.fields.find((field) => field.column === aggregate.column)
        : undefined;
      const aggregateFieldSlug = aggregateField
        ? variableSlug(aggregateField.name)
        : "";
      const variableSuffix = component === "table" ? "rows" : aggregate!.kind +
        (aggregateFieldSlug ? `_${aggregateFieldSlug}` : "");
      const id = `${formPrefix}_${variableSuffix}`;
      const candidate = draftQuery("inline_sql", { sql }, [], returns);
      candidate.kind === "query" &&
        (candidate.query.form_bindings = [{
          relation: form.name,
          form_id: form.id,
        }]);
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
      if (component === "table") {
        resolved.tablePresentation = {
          columns: form.fields.length > 0
            ? form.fields.map((field) => ({
              column: field.column,
              label: field.name,
            }))
            : [{
              column: "_ugoite_id",
              label: t("composition.studioEntryIdColumn"),
            }],
        };
      }
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
          const name = variableSlug(entry.name);
          const prefix = name && /^[a-z]/.test(name)
            ? name
            : name
            ? `query_${name}`
            : "query";
          const stem = `${prefix}_${
            column
              ? variableSlug(column) || "value"
              : component === "table"
              ? "table"
              : "value"
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
        attempt.saveTarget.kind === "update"
          ? {
            compositionId: attempt.saveTarget.compositionId,
            baseRevisionId: attempt.saveTarget.baseRevisionId,
            variables: attempt.variables ?? {},
            ...(attempt.integerVariableIds?.length
              ? { integerVariableIds: attempt.integerVariableIds }
              : {}),
            unset_variables: attempt.unset_variables ?? [],
          }
          : {
            variables: attempt.variables ?? {},
            ...(attempt.integerVariableIds?.length
              ? { integerVariableIds: attempt.integerVariableIds }
              : {}),
            unset_variables: attempt.unset_variables ?? [],
          },
      );
      if (!sameCompositionSaveIdentity(attempt, routeIdentity())) {
        clearPendingCompositionSaveAttempt(attempt, "completed", undefined);
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
        if (sameCompositionSaveIdentity(attempt, routeIdentity())) {
          pendingSave = undefined;
          setSaveRetryAvailable(false);
          setSaveError(compositionSaveErrorMessage(error));
        }
      } else {
        markPendingCompositionSaveAttemptUncertain(attempt, undefined);
        if (sameCompositionSaveIdentity(attempt, routeIdentity())) {
          pendingSave = attempt;
          setSaveRetryAvailable(true);
          setSaveError(t("composition.saveFailed"));
        }
      }
    }
  };

  const save = async () => {
    if (saving() || saveRetryAvailable() || pendingSave || !editorValid()) {
      return;
    }
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
    const inputSnapshot = getSaveInputSnapshot();
    if (!inputSnapshot) {
      setSaveError(t("composition.queryFailed"));
      return;
    }
    const saveIdentity = routeIdentity();
    const generation = saveGeneration() + 1;
    setSaveGeneration(generation);
    setSaving(true);
    setSaveError(null);
    let attempt: PendingCompositionSaveAttempt = {
      ...saveIdentity,
      name: current.name.trim(),
      yaml: "",
      variables: { ...inputSnapshot.variables },
      integerVariableIds: current.variables.flatMap((node) =>
        node.kind === "input" &&
          node.value_type.scalar_type === "integer"
          ? [node.id]
          : []
      ),
      unset_variables: [...inputSnapshot.unset_variables],
      idempotencyKey: crypto.randomUUID(),
    };
    pendingSave = attempt;
    stagePendingCompositionSaveAttempt(attempt, undefined);
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
      if (compositionTablePresentationDiagnostics(verified).length > 0) {
        setDraft(verified);
        clearPendingCompositionSaveAttempt(attempt, "rejected", undefined);
        pendingSave = undefined;
        setSaveRetryAvailable(false);
        setSaveError(t("composition.studioTableViewChanged"));
        return;
      }
      const canonical = await compositionApi.canonicalizeDocument(
        compositionGraphDocumentDraft(verified),
      );
      if (generation !== saveGeneration()) return;
      attempt = { ...attempt, yaml: canonical.canonical_yaml };
      pendingSave = attempt;
      await saveRequest(attempt, false);
    } catch (error) {
      clearPendingCompositionSaveAttempt(attempt, "rejected", undefined);
      if (sameCompositionSaveIdentity(attempt, routeIdentity())) {
        pendingSave = undefined;
        setSaveRetryAvailable(false);
      }
      if (
        generation === saveGeneration() &&
        sameCompositionSaveIdentity(attempt, routeIdentity())
      ) {
        setSaveError(compositionCanonicalizeErrorMessage(error));
      }
    } finally {
      if (
        generation === saveGeneration() &&
        sameCompositionSaveIdentity(attempt, routeIdentity())
      ) setSaving(false);
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
      runControllers.get(variableId)?.abort();
      countControllers.get(variableId)?.abort();
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
              placeholder={t("composition.namePlaceholder")}
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
          previewBase={props.saveMode.kind === "update"
            ? {
              entryId: props.saveMode.compositionId,
              revisionId: props.saveMode.baseRevisionId,
            }
            : undefined}
          forms={forms()}
          formsLoading={formsResource.loading}
          savedSql={savedSqlForPicker()}
          savedSqlLoading={sqlResource.loading}
          savedSqlSeedUnavailable={savedSqlSeedUnavailable()}
          initialSavedSqlBinding={props.initialSavedSqlBinding}
          provider={provider}
          session={session()}
          getPrincipalGeneration={getPrincipalGeneration}
          isSessionCurrent={isSessionCurrent}
          onSaveSnapshotReady={(getter) => {
            getSaveInputSnapshot = getter;
          }}
          sqlSchema={sqlSchema()}
          sqlForms={formsResource() ?? []}
          sqlBodies={sqlBodies()}
          actionsForQuery={actionsForQuery}
          previewForQuery={(variableId) =>
            queryPreviews()[variableId] ?? {
              status: "idle",
              isCurrent: false,
            }}
          onPlanColumnsForQuery={async (variableId) => {
            const currentDraft = draft();
            const currentSession = session();
            const currentSpace = props.spaceId;
            const response = await previewCandidate(currentDraft);
            const query = response.plan?.queries.find((query) =>
              query.variable_id === variableId
            );
            if (!response.ok || !query) {
              throw new Error(
                t("composition.queryFailed"),
              );
            }
            return {
              columns: query.result_columns.map((column) => column.name),
              isCurrent: () =>
                draft() === currentDraft && props.spaceId === currentSpace &&
                isSessionCurrent(currentSession),
            };
          }}
          onRunQuery={(variableId) => void runQuery(variableId)}
          onPreviousQuery={previousQueryPage}
          onCountQuery={(variableId) => void countQuery(variableId)}
          onNextQuery={(variableId) => {
            const evaluation = queryEvaluations()[variableId];
            if (evaluation?.next) void runQuery(variableId, evaluation.next);
          }}
          onDraftChange={onDraftChange}
          onQueryChange={(variableId, sql, formBindings) => {
            onDraftChange({
              ...draft(),
              variables: draft().variables.map((node) =>
                node.id === variableId && node.kind === "query" &&
                  node.query.kind === "inline_sql"
                  ? {
                    ...node,
                    query: {
                      ...node.query,
                      sql,
                      schema_fingerprint: "",
                      ...(formBindings ? { form_bindings: formBindings } : {}),
                    },
                  }
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
