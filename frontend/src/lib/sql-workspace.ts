import { createEffect, createMemo, createSignal, onCleanup } from "solid-js";
import type {
  SQLWorkspaceCount,
  SQLWorkspacePage,
  SQLWorkspacePlanState,
  SQLWorkspacePreview,
  SQLWorkspaceScalar,
  SQLWorkspaceVariable,
} from "~/components/SQLWorkspace";
import { formatUserFacingError } from "~/lib/user-facing-error";
import { t } from "~/lib/i18n";
import type { PlannedSqlColumns } from "~/lib/sql-document-columns";
import { normalizeSqlVariables, parseSqlVariableDefault } from "~/lib/sql";
import type {
  SqlQueryPage,
  SqlQueryPlanRequest,
  SqlVariable,
} from "~/lib/types";
import { sqlApi } from "~/lib/ugoite-client";

export type SqlWorkspaceVariableDefinition = SQLWorkspaceVariable & {
  description?: string;
};

/**
 * Normalize editable SQL while keeping the saved definition for placeholders
 * that remain in the query. Saved SQL owns variable type/default metadata;
 * changing the SQL text alone must not silently replace it with string values.
 */
export async function normalizeSqlWorkspaceVariables(
  sql: string,
  existing: readonly SqlVariable[] = [],
): Promise<{ sql: string; variables: SqlWorkspaceVariableDefinition[] }> {
  const normalized = await normalizeSqlVariables(sql, [...existing]);
  const existingByName = new Map(existing.map((variable) => [
    variable.name,
    variable,
  ]));

  return {
    sql: normalized.sql,
    variables: normalized.variables.map((variable) => {
      const previous = existingByName.get(variable.name);
      return previous
        ? { ...variable, ...previous, name: variable.name }
        : variable;
    }),
  };
}

interface SqlWorkspaceControllerOptions {
  spaceId: () => string;
  sql: () => string;
  variables: () => readonly SqlWorkspaceVariableDefinition[];
  /** Invalid declarations remain editable but cannot plan or execute. */
  definitionError?: () => string | undefined;
  savedSql?: () => { id: string; revisionId: string } | undefined;
  /** Draft saves resolve current Form names; exact revision previews retain bindings. */
  planSavedSql?: () => { id: string; revisionId: string } | undefined;
  /** Definition preflight is independent of transient preview input values. */
  planParameters?: "definition" | "values";
}

type PageState = {
  identity: string;
  page: SQLWorkspacePage;
  next?: string;
};

type PreviewState = {
  identity: string;
  status: SQLWorkspacePreview["status"];
  error?: string;
};

type CountState = SQLWorkspaceCount & { identity: string };

type QueryRequest = {
  sql: string;
  parameters: Record<string, unknown>;
  parameter_types: Record<string, string>;
  include_exact_integer_values: true;
  limit: number;
  continuation?: string;
  saved_sql?: { id: string; revision_id: string };
};

type BuiltRequest = { identity: string; request: QueryRequest };
type PlanState = SQLWorkspacePlanState & { identity: string };

const abortError = (error: unknown): boolean =>
  !!error && typeof error === "object" &&
  (error as { name?: unknown }).name === "AbortError";

const scalarText = (value: SQLWorkspaceScalar | undefined): string =>
  value === undefined ? "" : String(value);

function parseParameter(
  variable: SqlWorkspaceVariableDefinition,
  raw: string,
  useDefault: boolean,
): unknown {
  if (useDefault && variable.default !== undefined) {
    if (
      variable.type === "integer" &&
      !Number.isSafeInteger(variable.default)
    ) {
      throw new Error(t("sqlPage.numberRequired", { name: variable.name }));
    }
    if (
      variable.type === "float" &&
      (typeof variable.default !== "number" ||
        !Number.isFinite(variable.default))
    ) {
      throw new Error(t("sqlPage.numberRequired", { name: variable.name }));
    }
    return variable.default;
  }
  if (raw === "") return null;

  if (variable.type === "boolean") {
    if (raw === "true") return true;
    if (raw === "false") return false;
    throw new Error(t("sqlPage.booleanRequired", { name: variable.name }));
  }
  if (variable.type === "integer" || variable.type === "float") {
    const parsed = parseSqlVariableDefault(variable.type, raw);
    if (!parsed.valid || typeof parsed.value !== "number") {
      throw new Error(t("sqlPage.numberRequired", { name: variable.name }));
    }
    return parsed.value;
  }
  return raw;
}

/** Stateless preview/count/paging state owned by one SQL route adapter. */
export function createSqlWorkspaceController(
  options: SqlWorkspaceControllerOptions,
) {
  const [parameterValues, setParameterValues] = createSignal<
    Record<string, SQLWorkspaceScalar | undefined>
  >({});
  const [page, setPage] = createSignal<PageState>();
  const [previewState, setPreviewState] = createSignal<PreviewState>({
    identity: "",
    status: "idle",
  });
  const [countState, setCountState] = createSignal<CountState>({
    identity: "",
    status: "idle",
  });
  const [continuationStack, setContinuationStack] = createSignal<string[]>([]);
  const [currentIdentity, setCurrentIdentity] = createSignal("");
  const [planState, setPlanState] = createSignal<PlanState>({
    identity: "",
    status: "pending",
    isCurrent: false,
  });

  let pageGeneration = 0;
  let countGeneration = 0;
  let pageController: AbortController | undefined;
  let countController: AbortController | undefined;
  let planController: AbortController | undefined;
  let planTimer: ReturnType<typeof setTimeout> | undefined;
  let planGeneration = 0;
  let previousIdentity: string | undefined;

  const requestParts = createMemo(() => {
    const variables = options.variables();
    const values = parameterValues();
    const sql = options.sql().trim();
    const savedSql = options.savedSql?.();
    const definitionError = options.definitionError?.();
    const identity = JSON.stringify({
      spaceId: options.spaceId(),
      sql,
      variables,
      values,
      savedSql,
      definitionError,
    });
    return { identity, sql, variables, values, savedSql, definitionError };
  });

  const planParts = createMemo(
    () => {
      const parts = requestParts();
      const values = options.planParameters === "definition"
        ? {}
        : parts.values;
      const savedSql = options.planSavedSql
        ? options.planSavedSql()
        : parts.savedSql;
      const identity = JSON.stringify({
        spaceId: options.spaceId(),
        sql: parts.sql,
        variables: parts.variables,
        values,
        savedSql,
        definitionError: parts.definitionError,
      });
      return { ...parts, values, savedSql, identity };
    },
    undefined,
    { equals: (previous, next) => previous.identity === next.identity },
  );

  const plan = createMemo<SQLWorkspacePlanState>(() => {
    const identity = planParts().identity;
    const state = planState();
    if (state.identity !== identity) {
      return { status: "pending", isCurrent: false };
    }
    return {
      status: state.status,
      isCurrent: state.isCurrent,
      ...(state.error ? { error: state.error } : {}),
    };
  });

  createEffect(() => {
    const identity = requestParts().identity;
    setCurrentIdentity(identity);
    if (previousIdentity === undefined) {
      previousIdentity = identity;
      return;
    }
    if (identity === previousIdentity) return;

    previousIdentity = identity;
    pageGeneration += 1;
    countGeneration += 1;
    pageController?.abort();
    countController?.abort();
    pageController = undefined;
    countController = undefined;
    setPage(undefined);
    setContinuationStack([]);
    setPreviewState({ identity, status: "idle" });
    setCountState({ identity, status: "idle" });
  });

  createEffect(() => {
    const parts = planParts();
    const generation = ++planGeneration;
    if (planTimer !== undefined) clearTimeout(planTimer);
    planTimer = undefined;
    planController?.abort();
    planController = undefined;
    setPlanState({
      identity: parts.identity,
      status: "pending",
      isCurrent: false,
    });

    planTimer = setTimeout(() => {
      planTimer = undefined;
      let built: BuiltRequest;
      try {
        built = buildRequest(undefined, parts);
      } catch (error) {
        if (
          generation === planGeneration &&
          parts.identity === planParts().identity
        ) {
          setPlanState({
            identity: parts.identity,
            status: "error",
            isCurrent: true,
            error: formatUserFacingError(error, "sqlPage.failedQuery"),
          });
        }
        return;
      }
      if (
        generation !== planGeneration || built.identity !== parts.identity ||
        parts.identity !== planParts().identity
      ) return;

      const controller = new AbortController();
      planController = controller;
      const request: SqlQueryPlanRequest = {
        sql: built.request.sql,
        parameters: built.request.parameters,
        parameter_types: built.request.parameter_types,
        ...(built.request.saved_sql
          ? { saved_sql: built.request.saved_sql }
          : {}),
      };
      void sqlApi.plan(options.spaceId(), request, controller.signal).then(
        (result) => {
          if (
            generation === planGeneration && !controller.signal.aborted &&
            parts.identity === planParts().identity
          ) {
            setPlanState({
              identity: parts.identity,
              status: "success",
              isCurrent: true,
            });
          }
        },
        (error) => {
          if (
            generation === planGeneration && !controller.signal.aborted &&
            parts.identity === planParts().identity && !abortError(error)
          ) {
            setPlanState({
              identity: parts.identity,
              status: "error",
              isCurrent: true,
              error: formatUserFacingError(error, "sqlPage.failedQuery"),
            });
          }
        },
      ).finally(() => {
        if (generation === planGeneration && planController === controller) {
          planController = undefined;
        }
      });
    }, 350);
  });

  const preview = createMemo<SQLWorkspacePreview>(() => {
    const identity = currentIdentity();
    const state = previewState();
    const matching = state.identity === identity;
    const currentPage = page();
    const currentCount = countState();
    const pageIsCurrent = matching && currentPage?.identity === identity &&
      state.status === "success";

    return {
      status: matching ? state.status : "idle",
      isCurrent: Boolean(pageIsCurrent),
      ...(state.error && matching ? { error: state.error } : {}),
      ...(pageIsCurrent && currentPage ? { page: currentPage.page } : {}),
      count: currentCount.identity === identity
        ? {
          status: currentCount.status,
          ...(currentCount.value !== undefined
            ? { value: currentCount.value }
            : {}),
          ...(currentCount.error ? { error: currentCount.error } : {}),
        }
        : { status: "idle" },
    };
  });

  const buildRequest = (
    continuation?: string,
    parts = requestParts(),
  ): BuiltRequest => {
    if (parts.definitionError) throw new Error(parts.definitionError);
    const parameters: Record<string, unknown> = {};
    const parameterTypes: Record<string, string> = {};
    for (const variable of parts.variables) {
      const hasTransientValue = Object.prototype.hasOwnProperty.call(
        parts.values,
        variable.name,
      );
      const value = hasTransientValue ? parts.values[variable.name] : undefined;
      const raw = hasTransientValue
        ? scalarText(value)
        : scalarText(variable.default);
      parameters[variable.name] = parseParameter(
        variable,
        raw,
        !hasTransientValue,
      );
      parameterTypes[variable.name] = variable.type;
    }
    const request: QueryRequest = {
      sql: parts.sql,
      parameters,
      parameter_types: parameterTypes,
      include_exact_integer_values: true,
      limit: 100,
      ...(parts.savedSql
        ? {
          saved_sql: {
            id: parts.savedSql.id,
            revision_id: parts.savedSql.revisionId,
          },
        }
        : {}),
      ...(continuation ? { continuation } : {}),
    };
    return {
      identity: parts.identity,
      request,
    };
  };

  const loadPage = async (
    continuation: string | undefined,
    pageNumber: number,
    stack: string[],
  ) => {
    let built: BuiltRequest;
    try {
      built = buildRequest(continuation);
    } catch (error) {
      setPage(undefined);
      setPreviewState({
        identity: currentIdentity(),
        status: "error",
        error: error instanceof Error
          ? error.message
          : t("sqlPage.failedQuery"),
      });
      return;
    }

    pageController?.abort();
    const controller = new AbortController();
    pageController = controller;
    const generation = ++pageGeneration;
    setContinuationStack(stack);
    setPage(undefined);
    setPreviewState({ identity: built.identity, status: "loading" });
    setCountState({ identity: built.identity, status: "idle" });

    try {
      const result: SqlQueryPage = await sqlApi.query(
        options.spaceId(),
        built.request,
        controller.signal,
      );
      if (
        generation !== pageGeneration || controller.signal.aborted ||
        built.identity !== currentIdentity()
      ) return;

      setPage({
        identity: built.identity,
        next: result.next,
        page: {
          columns: result.columns,
          columnTypes: result.result_schema?.map((column) => column.type),
          rows: result.rows,
          exactIntegerRows: result.exact_integer_rows,
          pageIdentity: JSON.stringify({
            query: built.identity,
            continuation: continuation ?? null,
          }),
          pageNumber,
          canPrevious: pageNumber > 1,
          canNext: result.has_more && Boolean(result.next),
        },
      });
      setPreviewState({ identity: built.identity, status: "success" });
    } catch (error) {
      if (
        generation === pageGeneration && !controller.signal.aborted &&
        !abortError(error) && built.identity === currentIdentity()
      ) {
        setPreviewState({
          identity: built.identity,
          status: "error",
          error: formatUserFacingError(error, "sqlPage.failedQuery"),
        });
      }
    } finally {
      if (generation === pageGeneration) {
        if (pageController === controller) pageController = undefined;
      }
    }
  };

  const run = () => {
    void loadPage(undefined, 1, []);
  };

  const count = async () => {
    const currentPage = page();
    const identity = currentIdentity();
    if (
      !currentPage || currentPage.identity !== identity ||
      previewState().status !== "success"
    ) return;

    let built: BuiltRequest;
    try {
      built = buildRequest();
    } catch (error) {
      setCountState({
        identity,
        status: "error",
        error: error instanceof Error
          ? error.message
          : t("sqlPage.failedCount"),
      });
      return;
    }

    countController?.abort();
    const controller = new AbortController();
    countController = controller;
    const generation = ++countGeneration;
    setCountState({ identity: built.identity, status: "loading" });
    try {
      const value = await sqlApi.count(options.spaceId(), {
        sql: built.request.sql,
        parameters: built.request.parameters,
        parameter_types: built.request.parameter_types,
        ...(built.request.saved_sql
          ? { saved_sql: built.request.saved_sql }
          : {}),
      }, controller.signal);
      if (
        generation === countGeneration && !controller.signal.aborted &&
        built.identity === currentIdentity()
      ) {
        setCountState({
          identity: built.identity,
          status: "success",
          value,
        });
      }
    } catch (error) {
      if (
        generation === countGeneration && !controller.signal.aborted &&
        !abortError(error) && built.identity === currentIdentity()
      ) {
        setCountState({
          identity: built.identity,
          status: "error",
          error: formatUserFacingError(error, "sqlPage.failedCount"),
        });
      }
    } finally {
      if (generation === countGeneration && countController === controller) {
        countController = undefined;
      }
    }
  };

  const next = () => {
    const current = page();
    if (
      !current || current.identity !== currentIdentity() || !current.next ||
      previewState().status === "loading"
    ) return;
    const stack = [...continuationStack(), current.next];
    void loadPage(current.next, stack.length + 1, stack);
  };

  const previous = () => {
    const current = page();
    if (
      !current || current.identity !== currentIdentity() ||
      continuationStack().length === 0 || previewState().status === "loading"
    ) return;
    const stack = continuationStack().slice(0, -1);
    void loadPage(stack.at(-1), stack.length + 1, stack);
  };

  onCleanup(() => {
    pageGeneration += 1;
    countGeneration += 1;
    planGeneration += 1;
    if (planTimer !== undefined) clearTimeout(planTimer);
    planTimer = undefined;
    pageController?.abort();
    countController?.abort();
    planController?.abort();
    pageController = undefined;
    countController = undefined;
    planController = undefined;
  });

  const planColumns = async (): Promise<PlannedSqlColumns> => {
    const parts = planParts();
    const isCurrent = () => parts.identity === planParts().identity;
    const built = buildRequest(undefined, parts);
    const result = await sqlApi.plan(options.spaceId(), {
      sql: built.request.sql,
      parameters: built.request.parameters,
      parameter_types: built.request.parameter_types,
      ...(built.request.saved_sql
        ? { saved_sql: built.request.saved_sql }
        : {}),
    });
    if (!isCurrent()) {
      throw new Error(t("sqlPage.structure.documentEditUnavailable"));
    }
    return {
      columns: result.result_schema.map((column) => column.name),
      isCurrent,
    };
  };

  return {
    parameterValues,
    onParameterChange: (name: string, rawValue: string) => {
      setParameterValues((current) => ({ ...current, [name]: rawValue }));
    },
    preview,
    plan,
    planColumns,
    run,
    count,
    next,
    previous,
  };
}
