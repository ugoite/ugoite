import type { Diagnostic } from "@codemirror/lint";
import { createSignal, createUniqueId, For, Show } from "solid-js";
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
  | "copy"
  | "form"
  | "columns"
  | "filter"
  | "sort"
  | "aggregate"
  | "group_by"
  | "distinct"
  | "row_limit"
  | "structure"
  | "arguments";

/**
 * A capability supplied by the owning SQL adapter. This component only
 * dispatches the operation; it never parses or regenerates SQL itself.
 */
export interface SQLWorkspaceAction {
  kind: SQLWorkspaceActionKind;
  icon:
    | "edit"
    | "forms"
    | "columns"
    | "filter"
    | "sort"
    | "aggregate"
    | "group-by"
    | "distinct"
    | "row-limit"
    | "tree"
    | "appearance"
    | "sql";
  label: string;
  disabled?: boolean;
  disabledReason?: string;
  active?: boolean;
  onActivate: (returnFocus?: HTMLElement | null) => void;
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
}

export interface SQLWorkspaceProps {
  id?: string;
  sql: string;
  onSqlChange?: (sql: string) => void;
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
  /** Hide the default action bar when the host places it outside this workspace. */
  hideActions?: boolean;
  onDiagnostics?: (diagnostics: Diagnostic[]) => void;
  planner?: SQLWorkspacePlanState;
  previewDisabled?: boolean;
  onPreview: () => void;
  preview: SQLWorkspacePreview;
  onCount?: () => void;
  onPrevious?: () => void;
  onNext?: () => void;
}

const formatValue = (value: SQLWorkspaceScalar | undefined): string =>
  value === undefined ? "" : String(value);

export interface SQLWorkspaceActionBarProps {
  actions?: readonly SQLWorkspaceAction[];
}

/** Render the workspace's host-supplied structure actions at the host's chosen position. */
export function SQLWorkspaceActionBar(props: SQLWorkspaceActionBarProps) {
  const actions = () => props.actions ?? [];
  const [structureOpen, setStructureOpen] = createSignal(false);
  const structureMenuId = `sql-workspace-structure-${createUniqueId()}`;
  const structureButton = () =>
    document.querySelector<HTMLButtonElement>(
      `button[aria-controls="${structureMenuId}"]`,
    );

  const structureActions = () =>
    actions().filter((action) =>
      action.kind === "structure" || action.kind === "aggregate" ||
      action.kind === "group_by" || action.kind === "distinct" ||
      action.kind === "row_limit"
    );

  // Keep this action identity stable across menu open/close. The persistent
  // button is also the focus-return target for dialogs opened by nested items.
  const structureAction: SQLWorkspaceAction = {
    kind: "structure",
    icon: "appearance",
    label: t("sqlPage.structure.structure"),
    // Keep the entry point reachable so unavailable operations and their
    // reasons remain inspectable inside the menu.
    onActivate: () => setStructureOpen((current) => !current),
  };

  const visibleActions = () => {
    const current = actions();
    const primary = current.filter((action) =>
      action.kind === "form" || action.kind === "columns" ||
      action.kind === "filter" || action.kind === "sort"
    );
    const structure =
      current.some((action) =>
          action.kind === "structure" || action.kind === "aggregate" ||
          action.kind === "group_by" || action.kind === "distinct" ||
          action.kind === "row_limit"
        )
        ? [structureAction]
        : [];
    const argumentsAction = current.filter((action) =>
      action.kind === "arguments"
    );
    return [
      ...primary,
      ...structure,
      ...argumentsAction,
      ...current.filter((action) => action.kind === "copy"),
    ];
  };

  const actionName = (action: SQLWorkspaceAction) =>
    action.disabled && action.disabledReason
      ? `${action.label}: ${action.disabledReason}`
      : action.label;

  const handleKeyDown = (event: KeyboardEvent) => {
    if (event.key !== "Escape" || !structureOpen()) return;
    event.preventDefault();
    setStructureOpen(false);
    structureButton()?.focus();
  };

  return (
    <Show when={actions().length > 0}>
      <div class="sql-workspace__action-bar" onKeyDown={handleKeyDown}>
        <div
          class="sql-workspace__actions"
          role="group"
          aria-label={t("sqlPage.sql")}
        >
          <For each={visibleActions()}>
            {(action) => {
              const isStructure = action.kind === "structure";
              const visibleAction = action;
              return (
                <IconButton
                  icon={visibleAction.icon}
                  label={actionName(visibleAction)}
                  title={actionName(visibleAction)}
                  disabled={visibleAction.disabled}
                  active={isStructure
                    ? structureOpen() ||
                      structureActions().some((nested) => nested.active)
                    : visibleAction.active}
                  ariaExpanded={isStructure ? structureOpen() : undefined}
                  ariaControls={isStructure ? structureMenuId : undefined}
                  onClick={(event) => {
                    if (isStructure) {
                      visibleAction.onActivate(event.currentTarget);
                    } else {
                      setStructureOpen(false);
                      visibleAction.onActivate(event.currentTarget);
                    }
                  }}
                />
              );
            }}
          </For>
        </div>
        <Show when={structureOpen()}>
          <div
            id={structureMenuId}
            class="sql-workspace__structure-actions"
            role="group"
            aria-label={t("sqlPage.structure.structureOptions")}
          >
            <For each={structureActions()}>
              {(action) => (
                <IconButton
                  icon={action.icon}
                  label={actionName(action)}
                  title={actionName(action)}
                  disabled={action.disabled}
                  active={action.active}
                  onClick={() => {
                    setStructureOpen(false);
                    // The nested action unmounts with the menu. Focus the
                    // persistent opener after closing it and before dispatch,
                    // so any dialog captures a stable return target.
                    structureButton()?.focus();
                    action.onActivate(structureButton() ?? null);
                  }}
                />
              )}
            </For>
          </div>
        </Show>
      </div>
    </Show>
  );
}

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
      <Show when={!props.hideActions}>
        <SQLWorkspaceActionBar actions={actions()} />
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
          onChange={(value) => props.onSqlChange?.(value)}
          schema={props.schema}
          onDiagnostics={props.onDiagnostics}
          additionalDiagnostics={plannerDiagnostics()}
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
