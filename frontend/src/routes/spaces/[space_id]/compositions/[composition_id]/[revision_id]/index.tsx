import {
  createEffect,
  createResource,
  createSignal,
  onCleanup,
  Show,
  untrack,
} from "solid-js";
import { A, useNavigate, useParams } from "@solidjs/router";
import { CompositionGraphRenderer } from "~/components/CompositionGraphRenderer";
import { IconButton } from "~/components/IconButton";
import { IconLink } from "~/components/IconLink";
import { LocalBusyIndicator } from "~/components/LocalBusyIndicator";
import {
  compositionApi,
  compositionDisplayName,
  type CompositionEvaluationResponse,
} from "~/lib/composition-api";
import {
  type CompositionGraphDraft,
  compositionGraphDraftFromDocument,
  type CompositionLiteral,
} from "~/lib/composition-variable-binding";
import {
  affectedCompositionQueries,
  compositionEvaluationIdentity,
  mergeCompositionEvaluation,
  sameCompositionEvaluationIdentity,
} from "~/lib/composition-evaluation";
import { t } from "~/lib/i18n";
import { parseVariableGraphInput } from "~/lib/ugoite-client";
import { UgoiteApiError } from "~/lib/ugoite-client/protocol";
import { formatUserFacingError } from "~/lib/user-facing-error";
import {
  spaceCompositionEditPath,
  spaceCompositionHistoryPath,
  spaceCompositionRevisionPath,
} from "~/lib/space-path";
import { spaceRoute } from "~/lib/space-shell-route";

export const route = spaceRoute({ navigation: "compositions" });

interface RevisionDocument {
  routeKey: string;
  name: string;
  draft: CompositionGraphDraft;
}

interface EvaluationState {
  status: "idle" | "loading" | "ready" | "error";
  evaluationId?: string;
  response?: Awaited<ReturnType<typeof compositionApi.evaluate>>;
  error?: string;
}

type GraphTablePage = CompositionEvaluationResponse["query_results"][string];
interface GraphTablePageState {
  pages: GraphTablePage[];
  index: number;
}

const literalRawValue = (value: CompositionLiteral | undefined) =>
  !value || value.kind === "null" ? "" : String(value.value ?? "");

export default function CompositionRevisionRoute() {
  const params = useParams<{
    space_id: string;
    composition_id: string;
    revision_id: string;
  }>();
  const navigate = useNavigate();
  const historyHref = () =>
    spaceCompositionHistoryPath(params.space_id, params.composition_id);
  const routeKey = () =>
    JSON.stringify([
      params.space_id,
      params.composition_id,
      params.revision_id,
    ]);

  const [listing] = createResource(
    () => params.space_id,
    (spaceId) => compositionApi.list(spaceId, 100, 0).catch(() => undefined),
  );
  const latestRevisionId = () =>
    listing()?.items.find((item) =>
      item.composition_id === params.composition_id
    )?.revision_id;
  const isHistorical = () =>
    latestRevisionId() !== undefined &&
    latestRevisionId() !== params.revision_id;

  const [restoring, setRestoring] = createSignal(false);
  const [restoreError, setRestoreError] = createSignal<string | null>(null);
  const [restoreConflict, setRestoreConflict] = createSignal(false);
  const [restoreRetryable, setRestoreRetryable] = createSignal(false);
  let restoreKey = "";
  let restoreKeyScope = "";
  const restoreIdempotencyKey = (baseRevisionId: string): string => {
    const scope = params.composition_id + "/" + params.revision_id + "/" +
      baseRevisionId;
    if (restoreKeyScope !== scope || !restoreKey) {
      restoreKeyScope = scope;
      restoreKey = crypto.randomUUID();
    }
    return restoreKey;
  };

  const [revision] = createResource(
    routeKey,
    async (key) => {
      const [spaceId, compositionId, revisionId] = JSON.parse(key) as [
        string,
        string,
        string,
      ];
      const raw = await compositionApi.get(
        spaceId,
        compositionId,
        revisionId,
      );
      const yaml = raw.fields.spec;
      if (typeof yaml !== "string") throw new Error("Missing Composition spec");
      const linted = await compositionApi.lint(yaml);
      if (!linted.ok || !linted.value) throw new Error("Invalid Composition");
      const draft = compositionGraphDraftFromDocument(linted.value.document);
      return {
        routeKey: key,
        name: draft.name,
        draft,
      } satisfies RevisionDocument;
    },
  );
  const currentRevision = () => {
    const document = revision();
    return document?.routeKey === routeKey() ? document : undefined;
  };

  const [rawInputs, setRawInputs] = createSignal<Record<string, string>>({});
  const [inputValues, setInputValues] = createSignal<Record<string, unknown>>(
    {},
  );
  const [unsetInputs, setUnsetInputs] = createSignal<string[]>([]);
  const [inputStates, setInputStates] = createSignal<
    Record<string, "pending" | "invalid" | "missing">
  >({});
  const [blockedQueryIds, setBlockedQueryIds] = createSignal<Set<string>>(
    new Set(),
  );
  const [tablePages, setTablePages] = createSignal<
    Record<string, GraphTablePageState>
  >({});
  const [evaluation, setEvaluation] = createSignal<EvaluationState>({
    status: "idle",
  });
  const parseGenerations = new Map<string, number>();
  let evaluationGeneration = 0;
  let debounceTimer: ReturnType<typeof setTimeout> | undefined;
  let activeEvaluationId = "";
  let lastEvaluatedInputs:
    | { variables: Record<string, unknown>; unset: string[] }
    | undefined;

  const currentInputSnapshot = () => ({
    variables: { ...inputValues() },
    unset: [...unsetInputs()].sort(),
  });

  const changedInputs = (
    previous: typeof lastEvaluatedInputs,
    current: NonNullable<typeof lastEvaluatedInputs>,
  ) => {
    const ids = new Set([
      ...Object.keys(previous?.variables ?? {}),
      ...Object.keys(current.variables),
      ...(previous?.unset ?? []),
      ...current.unset,
    ]);
    const previousUnset = new Set(previous?.unset ?? []);
    const currentUnset = new Set(current.unset);
    return new Set(
      [...ids].filter((id) =>
        previousUnset.has(id) !== currentUnset.has(id) ||
        JSON.stringify(previous?.variables[id]) !==
          JSON.stringify(current.variables[id])
      ),
    );
  };

  const evaluate = async (page?: { variable_id: string; cursor: string }) => {
    if (!currentRevision()) return;
    if (
      Object.values(inputStates()).some((state) =>
        state === "pending" || state === "invalid"
      )
    ) return;
    const routeScope = routeKey();
    const spaceId = params.space_id;
    const compositionId = params.composition_id;
    const revisionId = params.revision_id;
    const generation = ++evaluationGeneration;
    let evaluationId = page ? activeEvaluationId : crypto.randomUUID();
    if (!page) activeEvaluationId = evaluationId;
    const values = currentInputSnapshot();
    const previous = evaluation().response;
    const previousIdentity = compositionEvaluationIdentity(previous);
    const requestedQueries = !page && previous && lastEvaluatedInputs &&
        previousIdentity
      ? [
        ...new Set([
          ...affectedCompositionQueries(
            previous.queries,
            previous.dependencies,
            changedInputs(lastEvaluatedInputs, values),
          ),
          ...blockedQueryIds(),
        ]),
      ]
      : undefined;
    setEvaluation((current) => ({
      ...current,
      status: "loading",
      evaluationId,
      error: undefined,
    }));
    try {
      const request = (id: string, requested?: string[]) =>
        compositionApi.evaluate(spaceId, {
          evaluation_id: id,
          target: {
            kind: "saved_revision",
            composition_id: compositionId,
            revision_id: revisionId,
          },
          variables: values.variables,
          ...(values.unset.length ? { unset_variables: values.unset } : {}),
          ...(requested ? { requested_queries: requested } : {}),
          ...(page ? { page } : {}),
        });
      let response = await request(evaluationId, requestedQueries);
      if (
        routeScope !== routeKey() ||
        generation !== evaluationGeneration ||
        response.evaluation_id !== evaluationId
      ) return;

      let reusablePrevious = previous;
      const responseIdentity = compositionEvaluationIdentity(response);
      if (
        !page && requestedQueries !== undefined && previous &&
        responseIdentity &&
        !sameCompositionEvaluationIdentity(
          previousIdentity,
          responseIdentity,
        )
      ) {
        // ACL, publication, or revision changed. Discard every retained value
        // and ask Core for the entire graph under the new authorization view.
        reusablePrevious = undefined;
        evaluationId = crypto.randomUUID();
        activeEvaluationId = evaluationId;
        setEvaluation({ status: "loading", evaluationId });
        response = await request(evaluationId);
        if (
          routeScope !== routeKey() ||
          generation !== evaluationGeneration ||
          response.evaluation_id !== evaluationId
        ) return;
      }

      const responseToShow = mergeCompositionEvaluation(
        reusablePrevious,
        response,
        page ? [page.variable_id] : requestedQueries,
      );
      if (page) {
        const pageResult = responseToShow.query_results[page.variable_id];
        if (pageResult) {
          setTablePages((current) => {
            const prior = current[page.variable_id];
            const pages = prior
              ? prior.pages.slice(0, prior.index + 1)
              : previous?.query_results[page.variable_id]
              ? [previous.query_results[page.variable_id]]
              : [];
            pages.push(pageResult);
            return {
              ...current,
              [page.variable_id]: { pages, index: pages.length - 1 },
            };
          });
        }
      } else {
        setTablePages((current) => {
          const next = requestedQueries === undefined ? {} : { ...current };
          for (const id of response.computed_query_ids) {
            const result = responseToShow.query_results[id];
            if (result?.value.kind === "table") {
              next[id] = { pages: [result], index: 0 };
            } else {
              delete next[id];
            }
          }
          return next;
        });
      }
      if (!page) {
        lastEvaluatedInputs = values;
        const computed = new Set(response.computed_query_ids ?? []);
        setBlockedQueryIds((current) =>
          new Set([...current].filter((id) => !computed.has(id)))
        );
      }
      const diagnosticsAreComponentScoped = Boolean(
        response.query_diagnostics?.length &&
          response.query_diagnostics.every((diagnostic) =>
            response.component_bindings.some((binding) =>
              binding.variable_id === diagnostic.variable_id
            )
          ),
      );
      setEvaluation({
        status: response.ok ? "ready" : "error",
        evaluationId,
        response: responseToShow,
        error: response.ok || diagnosticsAreComponentScoped
          ? undefined
          : t("composition.queryFailed"),
      });
    } catch (error) {
      if (routeScope !== routeKey() || generation !== evaluationGeneration) {
        return;
      }
      setEvaluation({
        status: "error",
        evaluationId,
        error: formatUserFacingError(
          error,
          "composition.detailFailed",
          "composition.evaluate",
        ),
      });
    }
  };

  const handleInput = (variableId: string, raw: string) => {
    setRawInputs((current) => ({ ...current, [variableId]: raw }));
    const node = currentRevision()?.draft.variables.find((item) =>
      item.id === variableId && item.kind === "input"
    );
    if (!node || node.kind !== "input") return;
    if (debounceTimer) clearTimeout(debounceTimer);
    debounceTimer = undefined;
    // Invalidate an already-running request immediately so a late result for
    // the previous raw input cannot become current while parsing this edit.
    evaluationGeneration++;
    setEvaluation((current) => ({
      ...current,
      status: current.response ? "ready" : "idle",
      error: undefined,
    }));
    const response = evaluation().response;
    if (response) {
      const affected = affectedCompositionQueries(
        response.queries,
        response.dependencies,
        new Set([variableId]),
      );
      setBlockedQueryIds((current) => new Set([...current, ...affected]));
      setTablePages((current) => {
        const next = { ...current };
        for (const id of affected) delete next[id];
        return next;
      });
    }
    setInputStates((current) => ({ ...current, [variableId]: "pending" }));
    const generation = (parseGenerations.get(variableId) ?? 0) + 1;
    parseGenerations.set(variableId, generation);
    void parseVariableGraphInput(node.value_type, raw).then((parsed) => {
      if (parseGenerations.get(variableId) !== generation) return;
      if (parsed.status === "valid") {
        setInputValues((current) => ({
          ...current,
          [variableId]: parsed.value,
        }));
        setUnsetInputs((current) => current.filter((id) => id !== variableId));
        setInputStates((current) => {
          const next = { ...current };
          delete next[variableId];
          return next;
        });
      } else if (parsed.status === "unset") {
        setInputValues((current) => {
          const next = { ...current };
          delete next[variableId];
          return next;
        });
        setUnsetInputs((current) =>
          current.includes(variableId) ? current : [...current, variableId]
        );
        setInputStates((current) => {
          const next = { ...current };
          if (node.required) next[variableId] = "missing";
          else delete next[variableId];
          return next;
        });
      } else {
        setInputStates((current) => ({ ...current, [variableId]: "invalid" }));
        return;
      }
      debounceTimer = setTimeout(() => void evaluate(), 250);
    }).catch(() => {
      if (parseGenerations.get(variableId) !== generation) return;
      setInputStates((current) => ({ ...current, [variableId]: "invalid" }));
    });
  };

  createEffect(() => {
    const currentRouteKey = routeKey();
    const ready = currentRevision();
    if (!ready) {
      evaluationGeneration++;
      if (debounceTimer) clearTimeout(debounceTimer);
      debounceTimer = undefined;
      activeEvaluationId = "";
      lastEvaluatedInputs = undefined;
      setRawInputs({});
      setInputValues({});
      setUnsetInputs([]);
      setInputStates({});
      setBlockedQueryIds(new Set());
      setTablePages({});
      parseGenerations.clear();
      setEvaluation({ status: "loading" });
      return;
    }
    if (ready.routeKey !== currentRouteKey) return;
    setRawInputs(
      Object.fromEntries(
        ready.draft.variables.flatMap((node) =>
          node.kind === "input" && node.default
            ? [[node.id, literalRawValue(node.default)]]
            : []
        ),
      ),
    );
    setInputValues({});
    setUnsetInputs([]);
    setInputStates({});
    setBlockedQueryIds(new Set());
    setTablePages({});
    parseGenerations.clear();
    setEvaluation({ status: "loading" });
    lastEvaluatedInputs = undefined;
    void untrack(() => evaluate());
  });
  onCleanup(() => {
    if (debounceTimer) clearTimeout(debounceTimer);
    evaluationGeneration++;
  });

  const restoreTablePage = (variableId: string, index: number) => {
    const state = tablePages()[variableId];
    const page = state?.pages[index];
    if (!state || !page || index < 0 || index >= state.pages.length) return;
    setTablePages((current) => ({
      ...current,
      [variableId]: { ...state, index },
    }));
    setEvaluation((current) =>
      current.response
        ? {
          ...current,
          response: {
            ...current.response,
            query_results: {
              ...current.response.query_results,
              [variableId]: page,
            },
          },
        }
        : current
    );
  };

  const nextTablePage = (variableId: string, cursor: string) => {
    const state = tablePages()[variableId];
    if (state && state.index + 1 < state.pages.length) {
      restoreTablePage(variableId, state.index + 1);
      return;
    }
    void evaluate({ variable_id: variableId, cursor });
  };

  const previousTablePage = (variableId: string) => {
    const state = tablePages()[variableId];
    if (state) restoreTablePage(variableId, state.index - 1);
  };

  const handleRestore = async () => {
    const base = latestRevisionId();
    if (!base || restoring()) return;
    setRestoring(true);
    setRestoreError(null);
    setRestoreConflict(false);
    setRestoreRetryable(false);
    try {
      const response = await compositionApi.restore(
        params.space_id,
        params.composition_id,
        params.revision_id,
        base,
        restoreIdempotencyKey(base),
      );
      navigate(spaceCompositionRevisionPath(
        params.space_id,
        response.composition_id,
        response.revision_id,
      ));
    } catch (error) {
      if (
        error instanceof UgoiteApiError &&
        (error.status === 409 || error.code === "REVISION_CONFLICT")
      ) {
        setRestoreConflict(true);
        setRestoreError(t("composition.restoreConflict"));
      } else {
        setRestoreError(formatUserFacingError(
          error,
          "composition.restoreFailed",
          "composition.restore",
        ));
        if (
          !(error instanceof UgoiteApiError) ||
          error.mutationOutcome !== "rejected"
        ) setRestoreRetryable(true);
      }
    } finally {
      setRestoring(false);
    }
  };

  return (
    <>
      <div class="flex items-center gap-2">
        <h1>{compositionDisplayName(currentRevision()?.name)}</h1>
        <IconLink
          icon="edit"
          label={t("composition.studioEdit")}
          href={spaceCompositionEditPath(
            params.space_id,
            params.composition_id,
            params.revision_id,
          )}
        />
        <IconLink
          icon="history"
          label={t("composition.history")}
          href={historyHref()}
        />
        <Show when={isHistorical()}>
          <IconButton
            icon="refresh"
            label={t("composition.restore")}
            disabled={restoring()}
            onClick={() => void handleRestore()}
          />
        </Show>
      </div>
      <Show when={restoring()}>
        <LocalBusyIndicator label={t("composition.restore")} />
      </Show>
      <Show when={restoreError()}>
        <p class="ui-text-danger" role="alert">{restoreError()}</p>
      </Show>
      <Show when={restoreConflict()}>
        <A href={historyHref()}>{t("composition.history")}</A>
      </Show>
      <Show when={restoreRetryable() && !restoring()}>
        <button
          class="ui-button ui-button-secondary"
          type="button"
          onClick={() => void handleRestore()}
        >
          {t("composition.retry")}
        </button>
      </Show>
      <Show when={revision.loading}>
        <LocalBusyIndicator label={t("composition.detailLoading")} />
      </Show>
      <Show when={revision.error}>
        <section class="section" role="alert">
          <p class="ui-text-danger">{t("composition.detailFailed")}</p>
          <button
            class="ui-button ui-button-secondary"
            type="button"
            onClick={() => void revision.refetch()}
          >
            {t("composition.retry")}
          </button>
        </section>
      </Show>
      <Show when={evaluation().status === "loading"}>
        <LocalBusyIndicator label={t("composition.queryLoading")} />
      </Show>
      <Show when={evaluation().error}>
        <p class="ui-text-danger" role="alert">{evaluation().error}</p>
      </Show>
      <Show when={currentRevision()}>
        {(document) => (
          <CompositionGraphRenderer
            draft={document().draft}
            evaluation={evaluation().response}
            rawInputs={rawInputs()}
            inputStates={inputStates()}
            blockedQueryIds={blockedQueryIds()}
            previousPageQueryIds={new Set(
              Object.entries(tablePages()).flatMap(([id, state]) =>
                state.index > 0 ? [id] : []
              ),
            )}
            pageBusy={evaluation().status === "loading"}
            onInput={handleInput}
            onNext={nextTablePage}
            onPrevious={previousTablePage}
          />
        )}
      </Show>
    </>
  );
}
