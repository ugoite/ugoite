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
  SQLWorkspacePlanState,
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
import { authApi, formApi, sqlApi } from "~/lib/ugoite-client";
import type { SqlResultColumnType } from "~/lib/types";
import { buildSqlSchema } from "~/lib/sql";

export type CompositionStudioSaveMode = CompositionSaveTarget;

type CompositionQueryCheck = SQLWorkspacePlanState & {
  draft: CompositionGraphDraft;
  scope: string;
  principal: string;
};

type CompositionInputSnapshot = {
  variables: Record<string, unknown>;
  unset_variables: string[];
};

const inputSnapshotKey = (snapshot: CompositionInputSnapshot) =>
  JSON.stringify({
    variables: Object.entries(snapshot.variables).sort(([left], [right]) =>
      left.localeCompare(right)
    ),
    unset_variables: [...snapshot.unset_variables].sort(),
  });

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
  let getSaveInputSnapshot: () => CompositionInputSnapshot | undefined =
    () => ({ variables: {}, unset_variables: [] });
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
  const editorValid = () => invalidEditorVariables().size === 0;
  const [saveGeneration, setSaveGeneration] = createSignal(0);
  const [queryPreviews, setQueryPreviews] = createSignal<
    Record<string, SQLWorkspacePreview>
  >({});
  const [queryChecks, setQueryChecks] = createSignal<
    Record<string, CompositionQueryCheck>
  >({});
  const [designPreviewGeneration, setDesignPreviewGeneration] = createSignal(0);
  const [queryEvaluations, setQueryEvaluations] = createSignal<
    Record<string, {
      evaluationId: string;
      yaml: string;
      next?: string;
      pageNumber: number;
      inputSnapshotKey: string;
      draft: CompositionGraphDraft;
      pages: Array<{ preview: SQLWorkspacePreview; next?: string }>;
      response: CompositionEvaluationResponse;
    }>
  >({});
  const runGenerations = new Map<string, number>();
  const runControllers = new Map<string, AbortController>();
  const checkGenerations = new Map<string, number>();
  const checkControllers = new Map<string, AbortController>();
  const countControllers = new Map<string, AbortController>();
  onCleanup(() => {
    for (
      const controller of [
        ...runControllers.values(),
        ...checkControllers.values(),
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
          ...checkControllers.values(),
          ...countControllers.values(),
        ]
      ) controller.abort();
      for (const [id, generation] of runGenerations) {
        runGenerations.set(id, generation + 1);
      }
      for (const [id, generation] of checkGenerations) {
        checkGenerations.set(id, generation + 1);
      }
      setQueryPreviews({});
      setQueryChecks({});
      setQueryEvaluations({});
      setInvalidEditorVariables(new Set<string>());
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

  const queryCheckFor = (variableId: string): SQLWorkspacePlanState => {
    const checked = queryChecks()[variableId];
    if (!checked) return { status: "pending", isCurrent: false };
    return {
      status: checked.status,
      isCurrent: checked.isCurrent && checked.draft === draft() &&
        checked.scope === JSON.stringify(routeIdentity()),
      error: checked.error,
      resultColumns: checked.resultColumns,
    };
  };

  const runQuery = async (variableId: string, cursor?: string) => {
    const currentDraft = draft();
    const scope = JSON.stringify(routeIdentity());
    const checked = queryChecks()[variableId];
    if (
      !checked || checked.status !== "success" || !checked.isCurrent ||
      checked.draft !== currentDraft || checked.scope !== scope
    ) return;
    let principal: string;
    try {
      principal = await getPrincipalGeneration();
    } catch {
      setQueryPreviews((current) => ({
        ...current,
        [variableId]: {
          status: "error",
          isCurrent: true,
          error: t("composition.queryFailed"),
        },
      }));
      return;
    }
    if (
      principal !== checked.principal || draft() !== currentDraft ||
      JSON.stringify(routeIdentity()) !== scope ||
      queryChecks()[variableId] !== checked
    ) {
      if (principal !== checked.principal) {
        setQueryChecks((current) => {
          const next = { ...current };
          delete next[variableId];
          return next;
        });
      }
      return;
    }
    const inputSnapshot = getSaveInputSnapshot();
    if (!inputSnapshot) {
      setQueryPreviews((current) => ({
        ...current,
        [variableId]: {
          status: "error",
          isCurrent: true,
          error: t("composition.queryFailed"),
        },
      }));
      return;
    }
    const currentInputKey = inputSnapshotKey(inputSnapshot);
    const generation = (runGenerations.get(variableId) ?? 0) + 1;
    runGenerations.set(variableId, generation);
    runControllers.get(variableId)?.abort();
    const controller = new AbortController();
    runControllers.set(variableId, controller);
    if (!cursor) countControllers.get(variableId)?.abort();
    const spaceId = props.spaceId;
    const previous = queryEvaluations()[variableId];
    const isContinuation = Boolean(
      cursor && previous && previous.draft === currentDraft &&
        previous.inputSnapshotKey === currentInputKey,
    );
    const activeCursor = isContinuation ? cursor : undefined;
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
          compositionGraphAuthoringDocument(currentDraft),
        )).canonical_yaml;
        evaluationId = crypto.randomUUID();
        pageNumber = 1;
      }
      if (
        controller.signal.aborted ||
        runGenerations.get(variableId) !== generation ||
        draft() !== currentDraft || queryChecks()[variableId] !== checked
      ) return;
      const evaluation = await compositionApi.evaluate(
        spaceId,
        {
          evaluation_id: evaluationId,
          target: { kind: "draft", yaml },
          ...(props.saveMode.kind === "update"
            ? {
              base_entry_id: props.saveMode.compositionId,
              base_revision_id: props.saveMode.baseRevisionId,
            }
            : {}),
          variables: inputSnapshot.variables,
          ...(inputSnapshot.unset_variables.length
            ? { unset_variables: inputSnapshot.unset_variables }
            : {}),
          requested_queries: [variableId],
          ...(activeCursor
            ? { page: { variable_id: variableId, cursor: activeCursor } }
            : {}),
        },
        controller.signal,
        currentDraft.variables.flatMap((node) =>
          node.kind === "input" &&
            node.value_type.scalar_type === "integer"
            ? [node.id]
            : []
        ),
      );
      if (
        controller.signal.aborted ||
        runGenerations.get(variableId) !== generation ||
        draft() !== currentDraft || queryChecks()[variableId] !== checked ||
        JSON.stringify(routeIdentity()) !== scope
      ) return;
      const currentPrincipal = await getPrincipalGeneration();
      if (currentPrincipal !== principal) {
        setQueryChecks((current) => {
          const next = { ...current };
          delete next[variableId];
          return next;
        });
        return;
      }
      if (
        controller.signal.aborted ||
        JSON.stringify(routeIdentity()) !== scope ||
        runGenerations.get(variableId) !== generation ||
        draft() !== currentDraft || queryChecks()[variableId] !== checked ||
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
          inputSnapshotKey: currentInputKey,
          draft: currentDraft,
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
      if (!activeCursor) {
        setDesignPreviewGeneration((generation) => generation + 1);
      }
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
    const preview = queryPreviewFor(variableId);
    const inputSnapshot = getSaveInputSnapshot();
    if (
      !evaluated || !inputSnapshot ||
      evaluated.inputSnapshotKey !== inputSnapshotKey(inputSnapshot) ||
      evaluated.draft !== draft() || preview?.status !== "success" ||
      !preview.isCurrent
    ) {
      return;
    }
    countControllers.get(variableId)?.abort();
    const controller = new AbortController();
    countControllers.set(variableId, controller);
    const isCurrent = () => {
      const currentSnapshot = getSaveInputSnapshot();
      return !controller.signal.aborted &&
        queryEvaluations()[variableId]?.evaluationId ===
          evaluated.evaluationId &&
        queryEvaluations()[variableId]?.yaml === evaluated.yaml &&
        currentSnapshot !== undefined &&
        inputSnapshotKey(currentSnapshot) === evaluated.inputSnapshotKey &&
        draft() === evaluated.draft;
    };
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
    const inputSnapshot = getSaveInputSnapshot();
    if (
      !current || !inputSnapshot || current.draft !== draft() ||
      current.inputSnapshotKey !== inputSnapshotKey(inputSnapshot) ||
      current.pageNumber <= 1
    ) return;
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
    requestedQueries?: readonly string[],
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
      requestedQueries,
    );
  };

  const queryPreviewFor = (variableId: string): SQLWorkspacePreview => {
    const preview = queryPreviews()[variableId] ?? {
      status: "idle" as const,
      isCurrent: false,
    };
    const evaluated = queryEvaluations()[variableId];
    const inputSnapshot = getSaveInputSnapshot();
    if (
      evaluated &&
      (!inputSnapshot || evaluated.inputSnapshotKey !==
          inputSnapshotKey(inputSnapshot) ||
        evaluated.draft !== draft())
    ) return { ...preview, isCurrent: false };
    return preview;
  };

  const checkQuery = async (variableId: string) => {
    const candidate = draft();
    const scope = JSON.stringify(routeIdentity());
    const generation = (checkGenerations.get(variableId) ?? 0) + 1;
    checkGenerations.set(variableId, generation);
    checkControllers.get(variableId)?.abort();
    const controller = new AbortController();
    checkControllers.set(variableId, controller);
    const pending: CompositionQueryCheck = {
      status: "pending",
      isCurrent: true,
      draft: candidate,
      scope,
      principal: "",
    };
    setQueryChecks((current) => ({ ...current, [variableId]: pending }));
    const isCurrent = () =>
      !controller.signal.aborted &&
      checkGenerations.get(variableId) === generation &&
      draft() === candidate &&
      JSON.stringify(routeIdentity()) === scope &&
      queryChecks()[variableId] === pending;
    try {
      const principal = await getPrincipalGeneration();
      if (!isCurrent()) return;
      const response = await previewCandidate(candidate, controller.signal, [
        variableId,
      ]);
      if (!isCurrent()) return;
      const currentPrincipal = await getPrincipalGeneration();
      if (!isCurrent()) return;
      if (principal !== currentPrincipal) {
        setQueryChecks((current) => ({
          ...current,
          [variableId]: {
            ...pending,
            status: "error",
            error: t("composition.queryFailed"),
            principal,
          },
        }));
        return;
      }
      const planned = response.plan?.queries.find((query) =>
        query.variable_id === variableId
      );
      if (!response.ok || !planned) {
        const diagnostic = response.diagnostics?.find((entry) =>
          entry.variable_id === variableId
        ) ?? response.diagnostics?.[0];
        setQueryChecks((current) => ({
          ...current,
          [variableId]: {
            ...pending,
            status: "error",
            error: diagnostic?.message || t("composition.queryFailed"),
            principal,
          },
        }));
        return;
      }
      setQueryChecks((current) => ({
        ...current,
        [variableId]: {
          ...pending,
          status: "success",
          principal,
          resultColumns: planned.result_columns.map((column) => ({
            name: column.name,
            type: column.value_type.scalar_type as SqlResultColumnType,
            nullable: column.value_type.nullable,
          })),
        },
      }));
    } catch {
      if (!isCurrent()) return;
      setQueryChecks((current) => ({
        ...current,
        [variableId]: {
          ...pending,
          status: "error",
          error: t("composition.queryFailed"),
        },
      }));
    }
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
    for (const controller of checkControllers.values()) controller.abort();
    for (const [id, generation] of checkGenerations) {
      checkGenerations.set(id, generation + 1);
    }
    setQueryChecks({});
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
          previewGeneration={designPreviewGeneration()}
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
          previewForQuery={queryPreviewFor}
          checkForQuery={queryCheckFor}
          onPlanColumnsForQuery={async (variableId) => {
            const currentDraft = draft();
            const currentSpace = props.spaceId;
            const response = await previewCandidate(currentDraft, undefined, [
              variableId,
            ]);
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
          onCheckQuery={(variableId) => void checkQuery(variableId)}
          onRunQuery={(variableId) => void runQuery(variableId)}
          onPreviousQuery={previousQueryPage}
          onCountQuery={(variableId) => void countQuery(variableId)}
          onNextQuery={(variableId) => {
            const evaluation = queryEvaluations()[variableId];
            if (evaluation?.next) void runQuery(variableId, evaluation.next);
          }}
          onDraftChange={onDraftChange}
          onQueryChange={(variableId, sql, formBindings, queryArguments) => {
            const currentDraft = draft();
            const current = currentDraft.variables.find((node) =>
              node.id === variableId && node.kind === "query"
            );
            if (
              !current || current.kind !== "query" ||
              current.query.kind !== "inline_sql"
            ) return;
            const query = {
              ...current.query,
              sql,
              schema_fingerprint: "",
              ...(formBindings ? { form_bindings: formBindings } : {}),
              ...(queryArguments ? { arguments: queryArguments } : {}),
            };
            if (JSON.stringify(current.query) === JSON.stringify(query)) return;
            onDraftChange({
              ...currentDraft,
              variables: currentDraft.variables.map((node) =>
                node.id === variableId && node.kind === "query"
                  ? { ...node, query }
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
