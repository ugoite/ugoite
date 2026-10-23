import { createEffect, createSignal, onCleanup, untrack } from "solid-js";
import {
  compositionApi,
  type CompositionEvaluationResponse,
} from "./composition-api";
import {
  changedCompositionQueryIds,
  dependentCompositionQueryIds,
  unavailableCompositionQueryIds,
} from "./composition-evaluation";
import {
  compositionGraphDocumentDraft,
  type CompositionGraphDraft,
} from "./composition-variable-binding";
import { parseVariableGraphInput } from "./ugoite-client";
import { t } from "./i18n";

/** Disposable Design Work. SQL parsing, binding, authorization and values remain Rust-owned. */
export function createCompositionDesignEvaluation(options: {
  draft: () => CompositionGraphDraft;
  scope: () => string;
  spaceId: () => string;
  principal?: () => Promise<string>;
  base?: () => { entryId: string; revisionId: string } | undefined;
}) {
  const [evaluation, setEvaluation] = createSignal<
    CompositionEvaluationResponse
  >();
  const [rawInputs, setRawInputs] = createSignal<Record<string, string>>({});
  const [inputStates, setInputStates] = createSignal<
    Record<string, "pending" | "invalid" | "missing">
  >({});
  const [blocked, setBlocked] = createSignal<Set<string>>(new Set());
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string>();
  const [pages, setPages] = createSignal<
    Record<
      string,
      {
        values: CompositionEvaluationResponse["query_results"][string][];
        index: number;
      }
    >
  >({});
  let values: Record<string, unknown> = {};
  let unset = new Set<string>();
  let generation = 0;
  let parseGeneration = 0;
  const parses = new Map<string, number>();
  let controller: AbortController | undefined;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let yaml = "";
  let evaluationId = "";
  const pending = new Set<string>();

  const cancel = () => {
    generation++;
    controller?.abort();
    if (timer) clearTimeout(timer);
    timer = undefined;
    setBusy(false);
  };
  const invalidate = (ids: readonly string[]) => {
    ids.forEach((id) => pending.add(id));
    setBlocked((current) => new Set([...current, ...ids]));
    setPages((current) =>
      Object.fromEntries(
        Object.entries(current).filter(([id]) => !ids.includes(id)),
      )
    );
  };
  const run = async (
    page?: { variable_id: string; cursor: string },
    full = false,
  ) => {
    const unavailable = new Set(
      unavailableCompositionQueryIds(
        options.draft(),
        inputStates(),
        values,
        unset,
      ),
    );
    if (
      page &&
      (unavailable.has(page.variable_id) || pending.has(page.variable_id) ||
        [...pending].some((id) => !unavailable.has(id)))
    ) return;
    cancel();
    const scope = options.scope();
    const draft = options.draft();
    const requestGeneration = generation;
    controller = new AbortController();
    const signal = controller.signal;
    const current = () =>
      !signal.aborted && requestGeneration === generation &&
      scope === options.scope();
    const requested = full && unavailable.size === 0
      ? undefined
      : (full
        ? draft.variables.filter((node) => node.kind === "query").map((node) =>
          node.id
        )
        : [...pending]).filter((id) => !unavailable.has(id));
    if (!page && requested?.length === 0 && pending.size === 0) return;
    setBlocked((current) => new Set([...current, ...unavailable]));
    setBusy(true);
    setError(undefined);
    try {
      const principal = await options.principal?.();
      if (!current()) return;
      if (!page) {
        const canonical = (await compositionApi.canonicalizeDocument(
          compositionGraphDocumentDraft(draft),
        )).canonical_yaml;
        if (!current()) return;
        yaml = canonical;
        evaluationId = crypto.randomUUID();
      }
      if (!current()) return;
      const id = evaluationId;
      const base = options.base?.();
      const response = await compositionApi.evaluate(options.spaceId(), {
        evaluation_id: id,
        target: { kind: "draft", yaml },
        ...(base
          ? { base_entry_id: base.entryId, base_revision_id: base.revisionId }
          : {}),
        variables: { ...values },
        ...(unset.size ? { unset_variables: [...unset] } : {}),
        ...(!page && requested ? { requested_queries: requested } : {}),
        ...(page ? { page, requested_queries: [page.variable_id] } : {}),
      }, signal);
      if (!current() || response.evaluation_id !== id) return;
      if (options.principal && principal !== await options.principal()) {
        if (current()) {
          setEvaluation(undefined);
          setPages({});
          setError(t("composition.queryFailed"));
        }
        return;
      }
      if (!current()) return;
      const previous = evaluation();
      const sameAuthority = previous && previous.authorization_fingerprint &&
        previous.publication_fingerprint &&
        previous.authorization_fingerprint ===
          response.authorization_fingerprint &&
        previous.publication_fingerprint === response.publication_fingerprint;
      if (
        previous && response.authorization_fingerprint &&
        response.publication_fingerprint && !sameAuthority && !full
      ) {
        setEvaluation(undefined);
        setPages({});
        invalidate(
          draft.variables.filter((node) => node.kind === "query").map((node) =>
            node.id
          ),
        );
        void run(undefined, true);
        return;
      }
      const invalidated = new Set([
        ...(page ? [page.variable_id] : requested ??
          draft.variables.filter((node) => node.kind === "query").map((
            node,
          ) => node.id)),
        ...response.computed_query_ids,
      ]);
      const results = sameAuthority ? { ...previous.query_results } : {};
      invalidated.forEach((id) => delete results[id]);
      Object.assign(results, response.query_results);
      setEvaluation({
        ...response,
        query_results: results,
        query_diagnostics: [
          ...(sameAuthority ? previous.query_diagnostics ?? [] : []).filter((
            diagnostic,
          ) => !invalidated.has(diagnostic.variable_id)),
          ...(response.query_diagnostics ?? []),
        ],
      });
      setPages((currentPages) => {
        const next = { ...currentPages };
        for (const id of invalidated) {
          const result = results[id];
          if (result?.value.kind !== "table") {
            delete next[id];
            continue;
          }
          const prior = page ? next[id] : undefined;
          const history = prior ? prior.values.slice(0, prior.index + 1) : [];
          history.push(result);
          next[id] = { values: history, index: history.length - 1 };
        }
        return next;
      });
      if (!page) {
        invalidated.forEach((id) => pending.delete(id));
        setBlocked((current) =>
          new Set([...current].filter((id) => !invalidated.has(id)))
        );
      }
      if (!response.ok && !response.query_diagnostics?.length) {
        setEvaluation(undefined);
        setPages({});
        invalidate(
          options.draft().variables.filter((node) => node.kind === "query").map(
            (node) => node.id,
          ),
        );
        setError(t("composition.queryFailed"));
      }
    } catch {
      if (!current()) return;
      setEvaluation(undefined);
      setPages({});
      invalidate(
        options.draft().variables.filter((node) => node.kind === "query").map((
          node,
        ) => node.id),
      );
      setError(t("composition.queryFailed"));
    } finally {
      if (current()) setBusy(false);
    }
  };
  const schedule = () => {
    cancel();
    timer = setTimeout(() => {
      timer = undefined;
      void run();
    }, 250);
  };
  let previousDraft: CompositionGraphDraft | undefined;
  let previousScope: string | undefined;
  createEffect(() => {
    const scope = options.scope();
    const draft = options.draft();
    untrack(() => {
      if (scope !== previousScope) {
        cancel();
        parses.clear();
        parseGeneration++;
        values = {};
        unset = new Set();
        pending.clear();
        setRawInputs({});
        setInputStates({});
        setEvaluation(undefined);
        setPages({});
        setBlocked(new Set());
        previousDraft = undefined;
        previousScope = scope;
      }
      const ids = previousDraft
        ? changedCompositionQueryIds(previousDraft, draft)
        : draft.variables.filter((node) => node.kind === "query").map((node) =>
          node.id
        );
      const inputs = draft.variables.filter((node) => node.kind === "input");
      const removedOrChangedInputs = new Set(
        (previousDraft?.variables ?? []).filter((node) =>
          node.kind === "input" &&
          JSON.stringify(node) !==
            JSON.stringify(inputs.find((next) => next.id === node.id))
        ).map((node) => node.id),
      );
      for (const id of removedOrChangedInputs) {
        delete values[id];
        unset.delete(id);
        parses.delete(id);
      }
      setRawInputs((current) =>
        Object.fromEntries(
          Object.entries(current).filter(([id]) =>
            inputs.some((node) => node.id === id) &&
            !removedOrChangedInputs.has(id)
          ),
        )
      );
      setInputStates((current) =>
        Object.fromEntries(
          Object.entries(current).filter(([id]) =>
            inputs.some((node) => node.id === id) &&
            !removedOrChangedInputs.has(id)
          ),
        )
      );
      previousDraft = draft;
      const queryIds = new Set(
        draft.variables.filter((node) => node.kind === "query").map((node) =>
          node.id
        ),
      );
      for (const id of pending) if (!queryIds.has(id)) pending.delete(id);
      invalidate(ids.filter((id) => queryIds.has(id)));
      if (ids.length) schedule();
    });
  });
  const onInput = (id: string, raw: string) => {
    const node = options.draft().variables.find((node) =>
      node.id === id && node.kind === "input"
    );
    if (!node || node.kind !== "input") return;
    const scope = options.scope();
    const parse = ++parseGeneration;
    parses.set(id, parse);
    cancel();
    delete values[id];
    unset.add(id);
    setRawInputs((current) => ({ ...current, [id]: raw }));
    setInputStates((current) => ({ ...current, [id]: "pending" }));
    invalidate(dependentCompositionQueryIds(options.draft(), new Set([id])));
    const current = () => scope === options.scope() && parses.get(id) === parse;
    void parseVariableGraphInput(node.value_type, raw).then((result) => {
      if (!current()) return;
      if (result.status === "valid") {
        values[id] = result.value;
        unset.delete(id);
      } else {
        delete values[id];
        unset.add(id);
      }
      setInputStates((previous) => {
        const next = { ...previous };
        delete next[id];
        if (result.status === "invalid") next[id] = "invalid";
        else if (result.status === "unset" && node.required) {
          next[id] = "missing";
        }
        return next;
      });
      schedule();
    }).catch(() => {
      if (current()) {
        setInputStates((previous) => ({ ...previous, [id]: "invalid" }));
        schedule();
      }
    });
  };
  const restorePage = (id: string, index: number) => {
    const state = pages()[id];
    const result = state?.values[index];
    if (!result || busy() || blocked().has(id)) return;
    setPages((current) => ({ ...current, [id]: { ...state, index } }));
    setEvaluation((current) =>
      current
        ? {
          ...current,
          query_results: { ...current.query_results, [id]: result },
        }
        : current
    );
  };
  onCleanup(() => {
    cancel();
    parses.clear();
  });
  return {
    evaluation,
    rawInputs,
    inputStates,
    blocked,
    busy,
    error,
    onInput,
    previousIds: () =>
      new Set(
        Object.entries(pages()).filter(([, state]) => state.index > 0).map((
          [id],
        ) => id),
      ),
    onPrevious: (id: string) => restorePage(id, (pages()[id]?.index ?? 0) - 1),
    onNext: (id: string, cursor: string) => {
      if (blocked().has(id)) return;
      const state = pages()[id];
      if (state && state.index + 1 < state.values.length) {
        restorePage(
          id,
          state.index + 1,
        );
      } else void run({ variable_id: id, cursor });
    },
  };
}
