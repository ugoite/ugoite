import type { Diagnostic } from "@codemirror/lint";
import { createUniqueId, For, Show } from "solid-js";
import type { SqlResultColumnType } from "~/lib/types";
import { t } from "~/lib/i18n";
import { ButtonSpinner } from "./ButtonSpinner";
import { ResultPagination } from "./ResultPagination";
import { SqlQueryEditor } from "./SqlQueryEditor";
import { SqlResultTable } from "./SqlResultTable";
import type { SqlSchema } from "~/lib/sql";

export type SQLWorkspaceScalar = string | number | boolean;

/** A SQL parameter declaration projected from its owning adapter. */
export interface SQLWorkspaceVariable {
  name: string;
  type: string;
  required?: boolean;
  default?: SQLWorkspaceScalar;
}

export interface SQLWorkspacePage {
  columns: readonly string[];
  columnTypes?: readonly SqlResultColumnType[];
  rows: readonly unknown[];
  exactIntegerRows?: readonly (readonly (string | null)[])[];
  pageIdentity: string;
  pageNumber?: number;
  canPrevious?: boolean;
  canNext?: boolean;
}

export interface SQLWorkspaceCount {
  status: "idle" | "loading" | "success" | "error";
  value?: number;
  error?: string;
}

export interface SQLWorkspacePreview {
  status: "idle" | "loading" | "success" | "error";
  /** The owner confirms this page matches the current SQL and parameters. */
  isCurrent: boolean;
  page?: SQLWorkspacePage;
  error?: string;
  count?: SQLWorkspaceCount;
}

export interface SQLWorkspacePlanState {
  status: "pending" | "success" | "error";
  /** The result matches the current SQL, parameters, revision, and Space. */
  isCurrent: boolean;
  error?: string;
  resultColumns?: readonly {
    name: string;
    type: SqlResultColumnType;
    nullable: boolean;
  }[];
}

export interface SQLWorkspaceProps {
  id?: string;
  sql: string;
  onSqlChange?: (sql: string) => void;
  schema: SqlSchema;
  lintProfile?: "standard" | "composition";
  readOnly?: boolean;
  variables?: readonly SQLWorkspaceVariable[];
  /**
   * Present keys are transient Work values. An explicit `undefined` means
   * unset and therefore overrides a declared default.
   */
  parameterValues?: Readonly<Record<string, SQLWorkspaceScalar | undefined>>;
  onParameterChange?: (name: string, rawValue: string) => void;
  parametersReadOnly?: boolean;
  onDiagnostics?: (diagnostics: Diagnostic[]) => void;
  planner?: SQLWorkspacePlanState;
  onCheck?: () => void;
  sqlArgumentVariables?: readonly string[];
  onSqlArgumentCompletion?: (name: string, sql: string) => void;
  previewDisabled?: boolean;
  onPreview: () => void;
  hideRun?: boolean;
  onAccept?: () => void;
  acceptLabel?: string;
  acceptDisabled?: boolean;
  onCancel?: () => void;
  preview: SQLWorkspacePreview;
  onCount?: () => void;
  onPrevious?: () => void;
  onNext?: () => void;
}

const formatValue = (value: SQLWorkspaceScalar | undefined): string =>
  value === undefined ? "" : String(value);

/**
 * Shared SQL editing and preview presentation. The owning route or
 * Composition source supplies persistence, query execution, typed SQL schema,
 * parameters, count, and continuation callbacks. SQL text changes only when
 * the editor emits a direct edit.
 */
export function SQLWorkspace(props: SQLWorkspaceProps) {
  const generatedId = createUniqueId();
  const workspaceId = () => props.id ?? `sql-workspace-${generatedId}`;
  const variables = () => props.variables ?? [];
  const editorReadOnly = () => props.readOnly || !props.onSqlChange;
  const plannerDiagnostics = (): Diagnostic[] => {
    const planner = props.planner;
    if (
      planner?.status !== "error" || !planner.isCurrent || !planner.error
    ) return [];
    return [{
      from: 0,
      to: props.sql.length,
      severity: "error",
      message: planner.error,
    }];
  };
  const previewDisabledReason = () => {
    const planner = props.planner;
    if (!planner || planner.status === "success" && planner.isCurrent) {
      return undefined;
    }
    if (planner.status === "error" && planner.isCurrent && planner.error) {
      return planner.error;
    }
    return t("sqlPage.planRequiredForRun");
  };

  const parameterValue = (
    name: string,
    fallback: SQLWorkspaceScalar | undefined,
  ) => {
    const values = props.parameterValues;
    if (values && Object.prototype.hasOwnProperty.call(values, name)) {
      return formatValue(values[name]);
    }
    return formatValue(fallback);
  };

  return (
    <div class="sql-workspace">
      <Show when={variables().length > 0}>
        <section
          class="sql-workspace__variables"
          aria-label={t("sqlPage.variables")}
        >
          <h2 class="ui-label">{t("sqlPage.variables")}</h2>
          <div class="sql-workspace__variable-grid">
            <For each={variables()}>
              {(variable, index) => {
                const inputId = () => `${workspaceId()}-variable-${index()}`;
                return (
                  <div class="sql-workspace__variable">
                    <label class="ui-label" for={inputId()}>
                      {variable.name}
                    </label>
                    <input
                      id={inputId()}
                      class="ui-input"
                      type="text"
                      inputMode={variable.type === "integer"
                        ? "numeric"
                        : variable.type === "float"
                        ? "decimal"
                        : "text"}
                      autocomplete="off"
                      spellcheck={false}
                      required={variable.required}
                      aria-required={variable.required || undefined}
                      value={parameterValue(variable.name, variable.default)}
                      readOnly={props.parametersReadOnly ||
                        !props.onParameterChange}
                      onInput={(event) =>
                        props.onParameterChange?.(
                          variable.name,
                          event.currentTarget.value,
                        )}
                    />
                  </div>
                );
              }}
            </For>
          </div>
        </section>
      </Show>

      <section
        class="sql-workspace__editor"
        aria-label={t("sqlPage.sql")}
      >
        <h2 class="ui-label">{t("sqlPage.sql")}</h2>
        <SqlQueryEditor
          id={`${workspaceId()}-editor`}
          ariaLabel={t("sqlPage.sql")}
          wrapLines
          value={props.sql}
          onChange={(value) => props.onSqlChange?.(value)}
          schema={props.schema}
          lintProfile={props.lintProfile}
          onDiagnostics={props.onDiagnostics}
          sqlArgumentVariables={props.sqlArgumentVariables}
          onSqlArgumentCompletion={props.onSqlArgumentCompletion}
          additionalDiagnostics={plannerDiagnostics()}
          disabled={editorReadOnly()}
        />
      </section>

      <div class="sql-workspace__preview-action">
        <Show when={props.onCheck}>
          <button
            type="button"
            class="btn"
            onClick={() => props.onCheck?.()}
            disabled={props.planner?.status === "pending" &&
                props.planner.isCurrent || props.previewDisabled}
            aria-busy={props.planner?.status === "pending" &&
                props.planner.isCurrent || undefined}
          >
            <Show when={props.planner?.status === "pending" &&
              props.planner.isCurrent}>
              <ButtonSpinner />
            </Show>
            {t("sqlPage.checkSql")}
          </button>
        </Show>
        <Show when={!props.hideRun}>
          <button
            type="button"
            class="btn primary"
            onClick={() => props.onPreview()}
            disabled={props.preview.status === "loading" ||
              props.previewDisabled ||
              Boolean(previewDisabledReason())}
            title={previewDisabledReason()}
            aria-describedby={previewDisabledReason()
              ? `${workspaceId()}-run-plan-reason`
              : undefined}
            aria-busy={props.preview.status === "loading" || undefined}
          >
            <Show when={props.preview.status === "loading"}>
              <ButtonSpinner />
            </Show>
            {t("sqlPage.runQuery")}
          </button>
        </Show>
        <Show when={props.onAccept}>
          <button
            type="button"
            class="btn primary"
            onClick={() => props.onAccept?.()}
            disabled={props.acceptDisabled}
          >
            {props.acceptLabel}
          </button>
        </Show>
        <Show when={props.onCancel}>
          <button
            type="button"
            class="btn"
            onClick={() => props.onCancel?.()}
          >
            {t("common.cancel")}
          </button>
        </Show>
        <Show when={props.planner?.status === "success" &&
          props.planner.isCurrent &&
          props.planner.resultColumns?.length}>
          <span class="sql-workspace__check-columns" role="status">
            <For each={props.planner?.resultColumns ?? []}>
              {(column, index) => (
                <span>
                  {index() > 0 ? ", " : ""}{column.name}: {column.type}{column.nullable ? "?" : ""}
                </span>
              )}
            </For>
          </span>
        </Show>
        <Show when={props.planner?.status === "error" &&
          props.planner.isCurrent && props.planner.error}>
          <span class="sql-workspace__error" role="status">
            {props.planner?.error}
          </span>
        </Show>
        <Show when={previewDisabledReason()}>
          {(reason) => (
            <span
              id={`${workspaceId()}-run-plan-reason`}
              class="sr-only"
            >
              {reason()}
            </span>
          )}
        </Show>
      </div>

      <Show when={props.preview.status === "loading"}>
        <p class="sql-workspace__status" role="status">
          {t("sqlPage.loadingResults")}
        </p>
      </Show>
      <Show when={props.preview.status === "error"}>
        <p class="sql-workspace__error" role="alert">
          {props.preview.error || t("sqlPage.failedQuery")}
        </p>
      </Show>

      <Show
        when={props.preview.status === "success" && props.preview.isCurrent
          ? props.preview.page
          : null}
      >
        {(page) => {
          const currentPage = page();
          const pageNumber = currentPage.pageNumber;
          return (
            <section
              class="sql-workspace__results"
              aria-label={t("sqlPage.results")}
            >
            <h2 class="ui-label">{t("sqlPage.results")}</h2>
            <div class="sql-workspace__result-controls">
              {pageNumber === undefined
                ? null
                : <span>{t("sqlPage.pageNumber", { page: pageNumber })}</span>}
              <Show when={currentPage.canPrevious || currentPage.canNext}>
                <ResultPagination
                  canPrevious={Boolean(currentPage.canPrevious && props.onPrevious)}
                  canNext={Boolean(currentPage.canNext && props.onNext)}
                  busy={props.preview.status === "loading"}
                  previousLabel={t("common.previous")}
                  nextLabel={t("common.next")}
                  ariaLabel={t("sqlPage.results")}
                  onPrevious={() => props.onPrevious?.()}
                  onNext={() => props.onNext?.()}
                />
              </Show>
              <Show when={props.onCount !== undefined}>
                <button
                  type="button"
                  class="btn secondary"
                  onClick={() => props.onCount?.()}
                  disabled={props.preview.count?.status === "loading"}
                  aria-busy={props.preview.count?.status === "loading" ||
                    undefined}
                >
                  <Show when={props.preview.count?.status === "loading"}>
                    <ButtonSpinner />
                  </Show>
                  {props.preview.count?.status === "loading"
                    ? t("sqlPage.counting")
                    : t("sqlPage.loadCount")}
                </button>
              </Show>
              <Show when={props.preview.count?.status === "success"}>
                <span>
                  {t("sqlPage.resultCount", {
                    count: props.preview.count?.value ?? 0,
                  })}
                </span>
              </Show>
            </div>
            <Show when={props.preview.count?.status === "error"}>
              <p class="sql-workspace__error" role="alert">
                {props.preview.count?.error || t("sqlPage.failedCount")}
              </p>
            </Show>
            <Show
              when={currentPage.rows.length > 0}
              fallback={<p>{t("sqlPage.noResults")}</p>}
            >
              <SqlResultTable
                columns={currentPage.columns}
                columnTypes={currentPage.columnTypes}
                rows={currentPage.rows}
                exactIntegerRows={currentPage.exactIntegerRows}
                pageIdentity={currentPage.pageIdentity}
                tableLabel={t("sqlPage.results")}
              />
            </Show>
          </section>
          );
        }}
      </Show>
    </div>
  );
}
