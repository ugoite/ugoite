import {
  createEffect,
  createSignal,
  createUniqueId,
  For,
  Show,
} from "solid-js";
import { IconButton } from "~/components/IconButton";
import { SqlArgumentDefinitionsEditor } from "~/components/SqlArgumentDefinitionsEditor";
import { SqlColumnsEditor } from "~/components/SqlColumnsEditor";
import { SqlFormPicker } from "~/components/SqlFormPicker";
import { SqlDocumentTreeEditor } from "~/components/SqlDocumentTreeEditor";
import { handleDialogKeyDown, useDialogFocus } from "~/components/dialog-focus";
import type { SQLWorkspaceAction } from "~/components/SQLWorkspace";
import type { SqlStructureAdapterContext } from "~/lib/sql";
import type { PlanSqlColumns } from "~/lib/sql-document-columns";
import type { SqlVariableDefaultDraft } from "~/lib/sql";
import { t } from "~/lib/i18n";
import type { SqlVariable } from "~/lib/types";
import type { SqlDocumentNode } from "~/lib/ugoite-client";
import type { SqlStructureVariableType } from "~/lib/ugoite-client";

type StructureAction =
  | "form"
  | "join"
  | "columns"
  | "filter"
  | "sort"
  | "aggregate"
  | "group_by"
  | "having"
  | "distinct"
  | "row_limit"
  | "document";
type ToolbarAction = StructureAction | "arguments";

interface SqlStructureToolbarProps {
  sql: () => string;
  /** Host identity, independent of SQL text, invalidates pending operations. */
  scope?: () => string;
  contexts: () => readonly SqlStructureAdapterContext[];
  onPlanColumns?: PlanSqlColumns;
  onSqlChange?: (sql: string) => void;
  onVariablesAdded?: (
    variables: readonly { name: string; type: SqlStructureVariableType }[],
  ) => boolean | void;
  argumentDefinitions?: () => readonly SqlVariable[];
  onArgumentAdded?: (variable: SqlVariable) => boolean | void;
  onArgumentUpdated?: (
    oldName: string,
    variable: SqlVariable,
  ) => boolean | void | Promise<boolean | void>;
  onArgumentRemoved?: (
    name: string,
  ) => boolean | void | Promise<boolean | void>;
  argumentDefaultsEnabled?: boolean;
  argumentDefaults?: () => Record<string, SqlVariableDefaultDraft>;
  onArgumentDefaultsChange?: (
    defaults: Record<string, SqlVariableDefaultDraft>,
  ) => void;
  /** Names owned by the host, including declarations not referenced in SQL yet. */
  reservedVariableNames?: () => readonly string[];
  onActionsChange: (actions: readonly SQLWorkspaceAction[]) => void;
  disabled?: () => boolean;
  /** Argument discovery can be pending independently of document editing. */
  argumentsDisabled?: () => boolean;
}

const actionNodeKinds: Record<
  StructureAction,
  readonly SqlDocumentNode["kind"][]
> = {
  form: ["table_factor", "select"],
  join: ["relation_tree", "join", "select"],
  columns: ["select_item", "select_list", "select"],
  filter: ["where_clause", "having_clause", "select", "predicate_expression"],
  sort: ["order_by_item", "query"],
  aggregate: ["function_expression", "select_item", "select_list"],
  group_by: ["group_by_item", "select"],
  having: ["having_clause", "select"],
  distinct: ["select"],
  row_limit: ["limit_clause", "offset_clause", "query"],
  document: [],
};

const actionIcons: Record<ToolbarAction, SQLWorkspaceAction["icon"]> = {
  form: "forms",
  join: "group-by",
  columns: "columns",
  filter: "filter",
  sort: "sort",
  aggregate: "aggregate",
  group_by: "group-by",
  having: "aggregate",
  distinct: "distinct",
  row_limit: "row-limit",
  document: "tree",
  arguments: "sql",
};

const isStructuralAction = (action: ToolbarAction): action is StructureAction =>
  action !== "arguments";

export function SqlStructureToolbar(props: SqlStructureToolbarProps) {
  const [activeAction, setActiveAction] = createSignal<ToolbarAction | null>(
    null,
  );
  const [targetNodeKinds, setTargetNodeKinds] = createSignal<
    readonly SqlDocumentNode["kind"][]
  >([]);
  const [dialogReturnFocus, setDialogReturnFocus] = createSignal<
    HTMLElement | null
  >(null);
  const [newArgumentName, setNewArgumentName] = createSignal("");
  const [newArgumentType, setNewArgumentType] = createSignal<
    SqlStructureVariableType
  >("string");
  const [error, setError] = createSignal<string>();
  const dialogId = `sql-structure-title-${createUniqueId()}`;
  let dialog: HTMLDivElement | undefined;

  const formOptions = () => {
    const seen = new Set<string>();
    return props.contexts().flatMap((context) => {
      const relation = context.parser.relation;
      if (!context.relationSelectable || !relation || seen.has(relation)) {
        return [];
      }
      seen.add(relation);
      return [context];
    });
  };

  const argumentVariables = (): SqlVariable[] => {
    const variables = new Map<string, SqlVariable>();
    for (const variable of props.argumentDefinitions?.() ?? []) {
      variables.set(variable.name, variable);
    }
    for (const context of props.argumentDefinitions ? [] : props.contexts()) {
      for (const variable of context.parser.variables ?? []) {
        if (!variables.has(variable.name)) {
          variables.set(variable.name, {
            name: variable.name,
            type: variable.type,
            description: "",
          });
        }
      }
    }
    for (const name of props.reservedVariableNames?.() ?? []) {
      if (!variables.has(name)) {
        variables.set(name, { name, type: "string", description: "" });
      }
    }
    return [...variables.values()];
  };

  const referencedArgumentNames = () => [
    ...new Set(
      props.contexts().flatMap((context) =>
        (context.parser.variables ?? []).map((variable) => variable.name)
      ),
    ),
  ];

  const readOnly = () => Boolean(props.disabled?.() || !props.onSqlChange);
  const argumentReadOnly = () => readOnly() || !!props.argumentsDisabled?.();
  const isDisabled = (action: ToolbarAction) => {
    if (readOnly()) return true;
    if (action === "form" || action === "join") {
      return formOptions().length === 0;
    }
    if (action === "arguments") {
      return argumentReadOnly() ||
        (!props.onArgumentAdded && !props.onVariablesAdded);
    }
    return false;
  };

  const openAction = (
    action: ToolbarAction,
    returnFocus?: HTMLElement | null,
  ) => {
    if (isDisabled(action)) return;
    const focusTarget = returnFocus ?? document.activeElement;
    setDialogReturnFocus(
      focusTarget instanceof HTMLElement ? focusTarget : null,
    );
    setError(undefined);
    setNewArgumentName("");
    setNewArgumentType("string");
    setTargetNodeKinds(
      action === "document"
        ? []
        : isStructuralAction(action)
        ? actionNodeKinds[action]
        : [],
    );
    setActiveAction(action);
  };

  const close = () => setActiveAction(null);
  let previousScope = props.scope?.();
  createEffect(() => {
    const scope = props.scope?.();
    if (scope !== previousScope) {
      previousScope = scope;
      setActiveAction(null);
    }
  });

  useDialogFocus(() => activeAction() !== null, {
    dialog: () => dialog,
    returnFocus: () => dialogReturnFocus(),
    inert: false,
    onClose: close,
  });

  const addArgument = () => {
    if (argumentReadOnly()) return;
    const name = newArgumentName().trim();
    if (!/^[A-Za-z_][A-Za-z0-9_]{0,127}$/.test(name)) return;
    if (argumentVariables().some((variable) => variable.name === name)) return;
    if (props.onArgumentAdded) {
      if (
        props.onArgumentAdded({
          name,
          type: newArgumentType(),
          description: "",
        }) === false
      ) {
        setError(t("sqlPage.structureUnavailable"));
        return;
      }
    } else if (
      props.onVariablesAdded?.([{ name, type: newArgumentType() }]) === false
    ) {
      setError(t("sqlPage.structureUnavailable"));
      return;
    }
    close();
  };

  // Stable action identities preserve the opener DOM through active-state changes.
  const actions = ([
    "form",
    "join",
    "columns",
    "filter",
    "sort",
    "aggregate",
    "group_by",
    "having",
    "distinct",
    "row_limit",
    "document",
    "arguments",
  ] as const).map((action): SQLWorkspaceAction => ({
    kind: action === "join" || action === "having" || action === "document"
      ? "structure"
      : action,
    icon: actionIcons[action],
    get label() {
      return t(`sqlPage.structure.${action}`);
    },
    get disabled() {
      return isDisabled(action);
    },
    get disabledReason() {
      return isDisabled(action)
        ? readOnly()
          ? t("sqlPage.structureReadOnly")
          : t("sqlPage.structureUnavailable")
        : undefined;
    },
    get active() {
      return activeAction() === action ||
        (isStructuralAction(action) && activeAction() === "document" &&
          targetNodeKinds().join() === actionNodeKinds[action].join());
    },
    onActivate: (returnFocus) => openAction(action, returnFocus),
  }));
  createEffect(() => props.onActionsChange(actions));

  return (
    <Show when={activeAction()}>
      {(action) => (
        <div
          class="ui-backdrop sql-structure-backdrop"
          onClick={(event) => {
            if (event.target === event.currentTarget) close();
          }}
        >
          <div
            ref={(element) => dialog = element}
            class="ui-dialog sql-structure-dialog"
            role="dialog"
            aria-modal="true"
            aria-labelledby={dialogId}
            tabIndex={-1}
            onKeyDown={(event) => handleDialogKeyDown(event, dialog, close)}
          >
            <header class="ui-dialog-header">
              <h2 id={dialogId} class="ui-dialog-title">
                {t(`sqlPage.structure.${action()}`)}
              </h2>
              <IconButton
                icon="close"
                label={t("common.close")}
                onClick={close}
              />
            </header>

            <div class="sql-structure-dialog-content">
              <Show when={action() === "arguments"}>
                <Show
                  when={props.argumentDefinitions}
                  fallback={
                    <div class="sql-structure-arguments">
                      <For each={argumentVariables()}>
                        {(variable) => (
                          <div class="sql-structure-argument" role="listitem">
                            <span>{variable.name}</span>
                            <span>{variable.type}</span>
                          </div>
                        )}
                      </For>
                      <label class="ui-field">
                        <span>{t("sqlPage.structure.argumentName")}</span>
                        <input
                          class="ui-input"
                          value={newArgumentName()}
                          onInput={(event) =>
                            setNewArgumentName(event.currentTarget.value)}
                        />
                      </label>
                      <label class="ui-field">
                        <span>{t("sqlPage.structure.argumentType")}</span>
                        <select
                          class="ui-input"
                          value={newArgumentType()}
                          onChange={(event) =>
                            setNewArgumentType(
                              event.currentTarget
                                .value as SqlStructureVariableType,
                            )}
                        >
                          <For
                            each={[
                              "string",
                              "boolean",
                              "integer",
                              "float",
                              "date",
                              "timestamp",
                            ] as const}
                          >
                            {(type) => <option value={type}>{type}</option>}
                          </For>
                        </select>
                      </label>
                    </div>
                  }
                >
                  <SqlArgumentDefinitionsEditor
                    idPrefix="sql-argument-definition"
                    variables={argumentVariables()}
                    referencedNames={referencedArgumentNames()}
                    defaults={props.argumentDefaults?.() ?? {}}
                    allowDefaults={props.argumentDefaultsEnabled}
                    disabled={argumentReadOnly()}
                    onAdd={(variable) =>
                      props.onArgumentAdded
                        ? props.onArgumentAdded(variable)
                        : false}
                    onUpdate={(oldName, variable) =>
                      props.onArgumentUpdated
                        ? props.onArgumentUpdated(oldName, variable)
                        : false}
                    onRemove={(name) =>
                      props.onArgumentRemoved
                        ? props.onArgumentRemoved(name)
                        : false}
                    onDefaultsChange={(defaults) =>
                      props.onArgumentDefaultsChange?.(defaults)}
                  />
                </Show>
              </Show>

              <Show when={action() === "form"}>
                <SqlFormPicker
                  sql={props.sql}
                  scope={props.scope}
                  onSqlChange={(sql) => props.onSqlChange?.(sql)}
                  disabled={props.disabled}
                  forms={() =>
                    formOptions().map((context) => ({
                      relation: context.parser.relation,
                      label: context.label ?? context.parser.relation,
                    }))}
                  onApplied={close}
                  onAdvanced={() => openAction("document", dialogReturnFocus())}
                />
              </Show>
              <Show when={action() === "columns"}>
                <SqlColumnsEditor
                  sql={props.sql}
                  scope={props.scope}
                  contexts={props.contexts}
                  onPlanColumns={props.onPlanColumns}
                  disabled={props.disabled}
                  onSqlChange={(sql) => props.onSqlChange?.(sql)}
                  onAdvanced={() => openAction("document", dialogReturnFocus())}
                />
              </Show>
              <Show
                when={action() !== "arguments" && action() !== "form" &&
                  action() !== "columns"}
              >
                <SqlDocumentTreeEditor
                  sql={props.sql}
                  scope={props.scope}
                  onSqlChange={(sql) => props.onSqlChange?.(sql)}
                  disabled={props.disabled}
                  initialNodeKinds={() => targetNodeKinds()}
                  onPlanColumns={props.onPlanColumns}
                  relationOptions={() =>
                    formOptions().map((context) => ({
                      relation: context.parser.relation,
                      label: context.label ?? context.parser.relation,
                      columns: context.parser.columns.map((column) =>
                        column.name
                      ),
                    }))}
                />
              </Show>
              <Show when={error()}>
                <span class="ui-text-danger" role="alert">{error()}</span>
              </Show>
            </div>

            <footer class="ui-dialog-actions">
              <button
                type="button"
                class="ui-button ui-button-secondary"
                onClick={close}
              >
                {t("common.close")}
              </button>
              <Show
                when={action() === "arguments" && !props.argumentDefinitions}
              >
                <button
                  type="button"
                  class="ui-button ui-button-primary"
                  onClick={addArgument}
                  disabled={!newArgumentName().trim()}
                >
                  {t("sqlPage.structure.addArgument")}
                </button>
              </Show>
            </footer>
          </div>
        </div>
      )}
    </Show>
  );
}
