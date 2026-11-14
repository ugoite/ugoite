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
  compositionBindingDiagnostics,
  compositionGraphAuthoringDocument,
  compositionGraphDocumentDraft,
  type CompositionGraphDraft,
  type CompositionLiteral,
  type CompositionVariableNode,
  createCompositionGraphDraft,
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
  formApi,
  sqlApi,
} from "~/lib/ugoite-client";
import type { SqlResultColumnType } from "~/lib/types";
import { buildSqlSchema } from "~/lib/sql";

export type CompositionStudioSaveMode = CompositionSaveTarget;

export interface CompositionStudioProps {
  spaceId: string;
  initialDraft?: CompositionGraphDraft;
  saveMode: CompositionStudioSaveMode;
  backHref: string;
  backLabel: string;
}

const inputDefaultValue = (node: CompositionVariableNode): unknown => {
  if (node.kind === "input" && node.default) {
    return node.default.kind === "null" ? null : node.default.value;
  }
  return undefined;
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
    invalidEditorVariables().size === 0;
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

  const getPrincipalGeneration = async () => {
    const authSession = await authApi.getSession();
    if (!authSession.authenticated) return "anonymous";
    const accountId = authSession.account?.account_id.trim();
    if (!accountId) throw new Error("Authenticated account is unavailable");
    return `account:${accountId}`;
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
        <Show when={formsResource.loading}>
          <LocalBusyIndicator label={t("common.loading")} />
        </Show>
        <CompositionGraphStudio
          spaceId={props.spaceId}
          draft={draft()}
          previewBase={props.saveMode.kind === "update"
            ? {
              entryId: props.saveMode.compositionId,
              revisionId: props.saveMode.baseRevisionId,
            }
            : undefined}
          getPrincipalGeneration={getPrincipalGeneration}
          onSaveSnapshotReady={(getter) => {
            getSaveInputSnapshot = getter;
          }}
          sqlSchema={sqlSchema()}
          sqlForms={formsResource() ?? []}
          previewForQuery={(variableId) =>
            queryPreviews()[variableId] ?? {
              status: "idle",
              isCurrent: false,
            }}
          onPlanColumnsForQuery={async (variableId) => {
            const currentDraft = draft();
            const currentSpace = props.spaceId;
            const response = await previewCandidate(currentDraft);
            const query = response.plan?.queries.find((query) =>
              query.variable_id === variableId
            );
            if (!response.ok || !query) {
              throw new Error(t("composition.queryFailed"));
            }
            return {
              columns: query.result_columns.map((column) => column.name),
              result_columns: query.result_columns,
              isCurrent: () =>
                draft() === currentDraft && props.spaceId === currentSpace,
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
          onQueryChange={(variableId, sql, formBindings, queryArguments) => {
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
                      ...(queryArguments ? { arguments: queryArguments } : {}),
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
