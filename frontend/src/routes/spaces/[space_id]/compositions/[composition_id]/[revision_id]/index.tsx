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
import { compositionApi, compositionDisplayName } from "~/lib/composition-api";
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
  name: string;
  draft: CompositionGraphDraft;
}

interface EvaluationState {
  status: "idle" | "loading" | "ready" | "error";
  evaluationId?: string;
  response?: Awaited<ReturnType<typeof compositionApi.evaluate>>;
  error?: string;
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
    () =>
      params.space_id + "/" + params.composition_id + "/" +
      params.revision_id,
    async () => {
      const raw = await compositionApi.get(
        params.space_id,
        params.composition_id,
        params.revision_id,
      );
      const yaml = raw.fields.spec;
      if (typeof yaml !== "string") throw new Error("Missing Composition spec");
      const linted = await compositionApi.lint(yaml);
      if (!linted.ok || !linted.value) throw new Error("Invalid Composition");
      const draft = compositionGraphDraftFromDocument(linted.value.document);
      return { name: draft.name, draft } satisfies RevisionDocument;
    },
  );

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
    if (!revision()) return;
    if (
      Object.values(inputStates()).some((state) =>
        state === "pending" || state === "invalid"
      )
    ) return;
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
        compositionApi.evaluate(params.space_id, {
          evaluation_id: id,
          target: {
            kind: "saved_revision",
            composition_id: params.composition_id,
            revision_id: params.revision_id,
          },
          variables: values.variables,
          ...(values.unset.length ? { unset_variables: values.unset } : {}),
          ...(requested ? { requested_queries: requested } : {}),
          ...(page ? { page } : {}),
        });
      let response = await request(evaluationId, requestedQueries);
      if (
        generation !== evaluationGeneration ||
        response.evaluation_id !== evaluationId
      ) return;

      let reusablePrevious = previous;
      if (
        !page && requestedQueries !== undefined && previous &&
        !sameCompositionEvaluationIdentity(
          previousIdentity,
          compositionEvaluationIdentity(response),
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
          generation !== evaluationGeneration ||
          response.evaluation_id !== evaluationId
        ) return;
      }

      const responseToShow = mergeCompositionEvaluation(
        reusablePrevious,
        response,
        page ? [page.variable_id] : requestedQueries,
      );
      if (!page) {
        lastEvaluatedInputs = values;
        const computed = new Set(response.computed_query_ids ?? []);
        setBlockedQueryIds((current) =>
          new Set([...current].filter((id) => !computed.has(id)))
        );
      }
      setEvaluation({
        status: response.ok ? "ready" : "error",
        evaluationId,
        response: responseToShow,
        error: response.ok ? undefined : response.diagnostics?.[0]?.code ??
          response.query_diagnostics?.[0]?.code ??
          t("composition.queryFailed"),
      });
    } catch (error) {
      if (generation !== evaluationGeneration) return;
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
    const node = revision()?.draft.variables.find((item) =>
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
    const ready = revision();
    if (!ready) return;
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
    parseGenerations.clear();
    setEvaluation({ status: "loading" });
    lastEvaluatedInputs = undefined;
    void untrack(() => evaluate());
  });
  onCleanup(() => {
    if (debounceTimer) clearTimeout(debounceTimer);
    evaluationGeneration++;
  });

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
        <h1>{compositionDisplayName(revision()?.name)}</h1>
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
      <Show when={revision()}>
        {(document) => (
          <CompositionGraphRenderer
            draft={document().draft}
            evaluation={evaluation().response}
            rawInputs={rawInputs()}
            inputStates={inputStates()}
            blockedQueryIds={blockedQueryIds()}
            onInput={handleInput}
            onNext={(variableId, cursor) =>
              void evaluate({ variable_id: variableId, cursor })}
          />
        )}
      </Show>
    </>
  );
}
