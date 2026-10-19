import type { Diagnostic } from "@codemirror/lint";
import { createUniqueId, For, Show } from "solid-js";
import type { SqlResultColumnType } from "~/lib/types";
import { t } from "~/lib/i18n";
import { ButtonSpinner } from "./ButtonSpinner";
import { IconButton } from "./IconButton";
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

export type SQLWorkspaceActionKind =
  | "columns"
  | "filter"
  | "sort"
  | "aggregate"
  | "group_by"
  | "distinct"
  | "structure"
  | "variables";

/**
 * A capability supplied by the owning SQL adapter. This component only
 * dispatches the operation; it never parses or regenerates SQL itself.
 */
export interface SQLWorkspaceAction {
  kind: SQLWorkspaceActionKind;
  icon:
    | "columns"
    | "filter"
    | "sort"
    | "aggregate"
    | "group-by"
    | "distinct"
    | "appearance"
    | "sql";
  label: string;
  disabled?: boolean;
  disabledReason?: string;
  active?: boolean;
  onActivate: () => void;
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

export interface SQLWorkspaceProps {
  id?: string;
  sql: string;
  onSqlChange?: (sql: string, userInitiated?: boolean) => void;
  schema: SqlSchema;
  readOnly?: boolean;
  variables?: readonly SQLWorkspaceVariable[];
  /**
   * Present keys are transient Work values. An explicit `undefined` means
   * unset and therefore overrides a declared default.
   */
  parameterValues?: Readonly<Record<string, SQLWorkspaceScalar | undefined>>;
  onParameterChange?: (name: string, rawValue: string) => void;
  parametersReadOnly?: boolean;
  actions?: readonly SQLWorkspaceAction[];
  onDiagnostics?: (diagnostics: Diagnostic[]) => void;
  previewDisabled?: boolean;
  onPreview: () => void;
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
 * the editor emits a direct edit; structured actions are explicit adapter
 * capabilities and never rewrite it from this component.
 */
export function SQLWorkspace(props: SQLWorkspaceProps) {
  const generatedId = createUniqueId();
  const workspaceId = () => props.id ?? `sql-workspace-${generatedId}`;
  const variables = () => props.variables ?? [];
  const actions = () => props.actions ?? [];
  const editorReadOnly = () => props.readOnly || !props.onSqlChange;

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
      <Show when={actions().length > 0}>
        <div
          class="sql-workspace__actions"
          role="group"
          aria-label={t("sqlPage.sql")}
        >
          <For each={actions()}>
            {(action) => {
              const accessibleName = () =>
                action.disabled && action.disabledReason
                  ? `${action.label}: ${action.disabledReason}`
                  : action.label;
              return (
                <IconButton
                  icon={action.icon}
                  label={accessibleName()}
                  title={accessibleName()}
                  disabled={action.disabled}
                  active={action.active}
                  onClick={() => action.onActivate()}
                />
              );
            }}
          </For>
        </div>
      </Show>
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
                      autoComplete="off"
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
          onChange={(value, userInitiated) =>
            props.onSqlChange?.(value, userInitiated)}
          schema={props.schema}
          onDiagnostics={props.onDiagnostics}
          disabled={editorReadOnly()}
        />
      </section>

      <div class="sql-workspace__preview-action">
        <button
          type="button"
          class="btn primary"
          onClick={() => props.onPreview()}
          disabled={props.preview.status === "loading" ||
            props.previewDisabled}
          aria-busy={props.preview.status === "loading" || undefined}
        >
          <Show when={props.preview.status === "loading"}>
            <ButtonSpinner />
          </Show>
          {t("sqlPage.runQuery")}
        </button>
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
        {(page) => (
          <section
            class="sql-workspace__results"
            aria-label={t("sqlPage.results")}
          >
            <h2 class="ui-label">{t("sqlPage.results")}</h2>
            <div class="sql-workspace__result-controls">
              <Show when={page().pageNumber !== undefined}>
                <span>
                  {t("sqlPage.pageNumber", { page: page().pageNumber })}
                </span>
              </Show>
              <Show when={page().canPrevious || page().canNext}>
                <ResultPagination
                  canPrevious={Boolean(page().canPrevious && props.onPrevious)}
                  canNext={Boolean(page().canNext && props.onNext)}
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
              when={page().rows.length > 0}
              fallback={<p>{t("sqlPage.noResults")}</p>}
            >
              <SqlResultTable
                columns={page().columns}
                columnTypes={page().columnTypes}
                rows={page().rows}
                exactIntegerRows={page().exactIntegerRows}
                pageIdentity={page().pageIdentity}
                tableLabel={t("sqlPage.results")}
              />
            </Show>
          </section>
        )}
      </Show>
    </div>
  );
}
