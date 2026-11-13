import type { PlannedSqlColumns } from "~/lib/sql-document-columns";
import { batch, createSignal, For, Show } from "solid-js";
import { CompositionGraphDesignCanvas } from "~/components/CompositionGraphDesignCanvas";
import { CompositionVariableGraphWorkspace } from "~/components/CompositionVariableGraphWorkspace";
import type { CompositionVariableCreationKind } from "~/components/CompositionVariableCreationForm";
import {
  appendCompositionGraphComponent,
  type CompositionGraphDraft,
  type CompositionQueryArgument,
  type CompositionSqlArgumentType,
  type CompositionVariableNode,
} from "~/lib/composition-variable-binding";
import type {
  SQLWorkspaceAction,
  SQLWorkspacePreview,
} from "~/components/SQLWorkspace";
import type { SqlSchema } from "~/lib/sql";
import type { Form, SqlVariable } from "~/lib/types";
import { normalizeSqlVariables } from "~/lib/sql";
import { renameSqlDocumentParameter } from "~/lib/ugoite-client";
import { t } from "~/lib/i18n";

export interface CompositionGraphStudioProps {
  spaceId: string;
  draft: CompositionGraphDraft;
  previewBase?: { entryId: string; revisionId: string };
  getPrincipalGeneration: () => Promise<string>;
  sqlSchema: SqlSchema;
  sqlForms?: readonly Form[];
  actionsForQuery?: (variableId: string) => readonly SQLWorkspaceAction[];
  previewForQuery?: (variableId: string) => SQLWorkspacePreview;
  onPlanColumnsForQuery?: (variableId: string) => Promise<PlannedSqlColumns>;
  onDraftChange: (draft: CompositionGraphDraft) => void;
  onQueryChange?: (
    variableId: string,
    sql: string,
    formBindings?: Array<{ relation: string; form_id: string }>,
    queryArguments?: CompositionQueryArgument[],
  ) => void;
  onRunQuery?: (variableId: string) => void;
  onNextQuery?: (variableId: string) => void;
  onPreviousQuery?: (variableId: string) => void;
  onCountQuery?: (variableId: string) => void;
  onEditorValidityChange?: (
    variableId: string,
    valid: boolean | undefined,
  ) => void;
  onSave?: () => void;
  saveDisabled?: boolean;
  saving?: boolean;
  onSaveSnapshotReady?: (
    getter: () => {
      variables: Record<string, unknown>;
      unset_variables: string[];
    } | undefined,
  ) => void;
}

type StudioMode = "design" | "variables";
type VariableCreationOrigin = StudioMode;
type PendingVariableCreation = {
  kind: CompositionVariableCreationKind;
  origin: VariableCreationOrigin;
  previousSelection: string | null;
  returnToDesign?: boolean;
  tableComponentId?: string;
};

const newVariable = (
  kind: CompositionVariableCreationKind,
  id: string,
): CompositionVariableNode =>
  kind === "constant"
    ? {
      id,
      kind,
      value_type: { scalar_type: "string", nullable: false },
      value: { kind: "string", value: "" },
    }
    : kind === "input"
    ? {
      id,
      kind,
      value_type: { scalar_type: "string", nullable: false },
      required: true,
    }
    : {
      id,
      kind,
      returns: { kind: "table", columns: [] },
      query: { kind: "inline_sql", sql: "SELECT 1", arguments: [] },
    };

const argumentTypes = new Set<CompositionSqlArgumentType>([
  "string",
  "boolean",
  "integer",
  "float",
  "date",
  "timestamp",
]);

const typeOfVariable = (
  node: CompositionGraphDraft["variables"][number],
): CompositionSqlArgumentType | undefined => {
  const valueType = node.kind === "query"
    ? node.returns.kind === "scalar" ? node.returns.value_type : undefined
    : node.value_type;
  return valueType && argumentTypes.has(
      valueType.scalar_type as CompositionSqlArgumentType,
    )
    ? valueType.scalar_type as CompositionSqlArgumentType
    : undefined;
};

/** Single strict graph-based Studio surface shared by create and edit routes. */
export function CompositionGraphStudio(props: CompositionGraphStudioProps) {
  const [mode, setMode] = createSignal<StudioMode>("design");
  const [selectedVariableId, setSelectedVariableId] = createSignal<
    string | null
  >(
    null,
  );
  const [pendingCreation, setPendingCreation] = createSignal<
    PendingVariableCreation | null
  >(null);
  const [unboundArgumentTypes, setUnboundArgumentTypes] = createSignal<
    Record<string, Record<string, CompositionSqlArgumentType>>
  >({});
  let variablePanel: HTMLElement | undefined;

  const parameterDeclarations = () =>
    Object.fromEntries(
      props.draft.variables.filter((node) => node.kind === "query").map((
        node,
      ) => [
        node.id,
        node.query.arguments.flatMap((argument) => {
          const variable = props.draft.variables.find((entry) =>
            entry.id === argument.variable
          );
          const type = (variable && typeOfVariable(variable)) ||
            unboundArgumentTypes()[node.id]?.[argument.name];
          return type ? [{ name: argument.name, type }] : [];
        }),
      ]),
    );

  const updateArgumentDefinition = async (
    id: string,
    oldName: string,
    argument: SqlVariable,
  ) => {
    if (
      !argumentTypes.has(argument.type as CompositionSqlArgumentType) ||
      argument.default !== undefined
    ) return false;
    const draft = props.draft;
    const node = draft.variables.find((entry) =>
      entry.id === id && entry.kind === "query"
    );
    if (
      !node || node.kind !== "query" || node.query.kind !== "inline_sql" ||
      !node.query.arguments.some((entry) => entry.name === oldName) ||
      node.query.arguments.some((entry) =>
        entry.name === argument.name && entry.name !== oldName
      )
    ) return false;
    let sql = node.query.sql;
    if (oldName !== argument.name) {
      try {
        sql = (await renameSqlDocumentParameter(
          (await normalizeSqlVariables(sql)).sql,
          oldName,
          argument.name,
        )).source;
      } catch {
        return false;
      }
    }
    if (props.draft !== draft || selectedVariableId() !== id) return false;
    const args = node.query.arguments.map((entry) => {
      if (entry.name !== oldName) return entry;
      const bound = draft.variables.find((variable) =>
        variable.id === entry.variable
      );
      return {
        name: argument.name,
        variable: bound && typeOfVariable(bound) === argument.type
          ? entry.variable
          : "",
      };
    });
    batch(() => {
      setUnboundArgumentTypes((current) => {
        const next = { ...current[id] };
        delete next[oldName];
        return {
          ...current,
          [id]: {
            ...next,
            [argument.name]: argument.type as CompositionSqlArgumentType,
          },
        };
      });
      props.onDraftChange({
        ...draft,
        variables: draft.variables.map((entry) =>
          entry.id === id
            ? { ...node, query: { ...node.query, sql, arguments: args } }
            : entry
        ),
      });
    });
    return true;
  };

  const removeArgumentDefinition = async (id: string, name: string) => {
    const draft = props.draft;
    const node = draft.variables.find((entry) =>
      entry.id === id && entry.kind === "query"
    );
    if (!node || node.kind !== "query" || node.query.kind !== "inline_sql") {
      return false;
    }
    try {
      if (
        (await normalizeSqlVariables(node.query.sql)).variables.some((entry) =>
          entry.name === name
        )
      ) return false;
    } catch {
      return false;
    }
    if (props.draft !== draft || selectedVariableId() !== id) return false;
    batch(() => {
      setUnboundArgumentTypes((current) => {
        const next = { ...current[id] };
        delete next[name];
        return { ...current, [id]: next };
      });
      props.onDraftChange({
        ...draft,
        variables: draft.variables.map((entry) =>
          entry.id === id
            ? {
              ...node,
              query: {
                ...node.query,
                arguments: node.query.arguments.filter((entry) =>
                  entry.name !== name
                ),
              },
            }
            : entry
        ),
      });
    });
    return true;
  };
  const requestCreation = (
    kind: CompositionVariableCreationKind,
    origin: VariableCreationOrigin,
    tableComponentId?: string,
  ) => {
    if (pendingCreation()) return;
    setPendingCreation({
      kind,
      origin,
      previousSelection: selectedVariableId(),
      ...(tableComponentId !== undefined
        ? { returnToDesign: true, tableComponentId }
        : {}),
    });
    if (origin === "variables") setSelectedVariableId(null);
  };

  const cancelCreation = () => {
    const pending = pendingCreation();
    if (!pending) return;
    setPendingCreation(null);
    if (pending.returnToDesign) setMode("design");
    if (pending.origin === "variables") {
      setSelectedVariableId(
        pending.previousSelection &&
          props.draft.variables.some((node) =>
            node.id === pending.previousSelection
          )
          ? pending.previousSelection
          : null,
      );
    }
  };

  const commitCreation = (rawName: string): boolean => {
    const pending = pendingCreation();
    const id = rawName.trim();
    if (
      !pending || !id ||
      props.draft.variables.some((node) => node.id === id)
    ) return false;
    const variable = newVariable(pending.kind, id);
    let nextDraft: CompositionGraphDraft = {
      ...props.draft,
      variables: [...props.draft.variables, variable],
    };
    if (pending.origin === "design") {
      if (pending.kind === "input") {
        const componentId = (() => {
          let index = 1;
          while (nextDraft.components.some((component) =>
            component.id === `input_${index}`
          )) index++;
          return `input_${index}`;
        })();
        const withComponent = appendCompositionGraphComponent(nextDraft, {
          id: componentId,
          kind: "input",
          variable: id,
        });
        if (!withComponent) return false;
        nextDraft = withComponent;
      }
    }
    if (pending.returnToDesign) {
      const tableComponentId = pending.tableComponentId;
      if (tableComponentId) {
        const existing = nextDraft.components.find((component) =>
          component.id === tableComponentId && component.kind === "table"
        );
        if (!existing) return false;
        nextDraft = {
          ...nextDraft,
          components: nextDraft.components.map((component) =>
            component.id === tableComponentId && component.kind === "table"
              ? { ...component, variable: id }
              : component
          ),
        };
      } else {
        let index = 1;
        while (nextDraft.components.some((component) =>
          component.id === `table_${index}`
        )) index++;
        const withComponent = appendCompositionGraphComponent(nextDraft, {
          id: `table_${index}`,
          kind: "table",
          variable: id,
        });
        if (!withComponent) return false;
        nextDraft = withComponent;
      }
    }
    props.onDraftChange(nextDraft);
    setSelectedVariableId(id);
    if (pending.returnToDesign) setMode("design");
    setPendingCreation(null);
    return true;
  };

  const requestTableQuery = (componentId?: string) => {
    if (pendingCreation()) return;
    setPendingCreation({
      kind: "query",
      origin: "variables",
      previousSelection: selectedVariableId(),
      returnToDesign: true,
      ...(componentId !== undefined ? { tableComponentId: componentId } : {}),
    });
    setMode("variables");
    setSelectedVariableId(null);
  };

  const changeMode = (entry: StudioMode) => {
    if (pendingCreation() && pendingCreation()?.origin !== entry) return;
    setMode(entry);
  };

  const pendingForSave = () => !!pendingCreation();
  const focusCreatedVariable = () => {
    const id = selectedVariableId();
    if (!id) return;
    if (mode() === "variables") {
      variablePanel?.querySelector<HTMLInputElement>(
        ".compositionVariableName",
      )?.focus();
      return;
    }
    const selectedComponent = document.querySelector<HTMLElement>(
      ".compositionGraphDesignValue--selected[data-component-select='true']",
    );
    if (selectedComponent) {
      selectedComponent.focus();
      return;
    }
    const input = Array.from(
      document.querySelectorAll<HTMLInputElement>(
        ".compositionGraphInput input",
      ),
    ).find((candidate) => candidate.getAttribute("aria-label") === id);
    input?.focus();
  };

  return (
    <>
      <div class="compositionGraphToolbar">
        <nav
          class="studioMode"
          role="tablist"
          aria-label={t("composition.studioMode")}
        >
          <For each={["design", "variables"] as const}>
            {(entry) => (
              <button
                type="button"
                role="tab"
                class="studioModeOption"
                classList={{ "studioModeOption--active": mode() === entry }}
                aria-selected={mode() === entry}
                disabled={!!pendingCreation() &&
                  pendingCreation()?.origin !== entry}
                aria-describedby={pendingCreation() &&
                    pendingCreation()?.origin !== entry
                  ? "compositionPendingModeNotice"
                  : undefined}
                onClick={() => changeMode(entry)}
              >
                {entry === "design"
                  ? t("composition.studioDesign")
                  : t("composition.studioVariables")}
              </button>
            )}
          </For>
        </nav>
        <Show when={pendingCreation()}>
          <span
            id="compositionPendingModeNotice"
            class="ui-sr-only"
            role="status"
          >
            {t("composition.finishOrCancelVariableCreation")}
          </span>
        </Show>
        <Show when={props.onSave}>
          <button
            type="button"
            class="btn primary compositionGraphSave"
            disabled={props.saveDisabled || props.saving || pendingForSave()}
            title={pendingForSave()
              ? t("composition.studioSaveNeedsVariableName")
              : undefined}
            aria-describedby={pendingForSave()
              ? "composition-pending-variable-save"
              : undefined}
            onClick={() => props.onSave?.()}
          >
            {t("composition.save")}
          </button>
          <Show when={pendingForSave()}>
            <span id="composition-pending-variable-save" class="ui-sr-only">
              {t("composition.studioSaveNeedsVariableName")}
            </span>
          </Show>
        </Show>
      </div>
      <section hidden={mode() !== "design"}>
        <CompositionGraphDesignCanvas
          spaceId={props.spaceId}
          draft={props.draft}
          previewBase={props.previewBase}
          getPrincipalGeneration={props.getPrincipalGeneration}
          onSaveSnapshotReady={props.onSaveSnapshotReady}
          onDraftChange={props.onDraftChange}
          pendingInputCreation={pendingCreation()?.origin === "design"}
          onRequestCreateInput={() => requestCreation("input", "design")}
          onCommitCreateInput={commitCreation}
          onCancelCreateInput={cancelCreation}
          onFocusCreatedInput={focusCreatedVariable}
          onRequestCreateTableQuery={requestTableQuery}
          onEditVariable={(id) => {
            setSelectedVariableId(id);
            setMode("variables");
            queueMicrotask(() =>
              variablePanel?.querySelector<HTMLInputElement>(
                ".compositionVariableName",
              )?.focus()
            );
          }}
        />
      </section>
      <section ref={variablePanel} hidden={mode() !== "variables"}>
        <CompositionVariableGraphWorkspace
          draft={props.draft}
          selectedVariableId={selectedVariableId()}
          onSelect={setSelectedVariableId}
          onDraftChange={props.onDraftChange}
          pendingCreationKind={pendingCreation()?.origin === "variables"
            ? pendingCreation()?.kind
            : undefined}
          onRequestCreation={(kind) => requestCreation(kind, "variables")}
          onCommitCreation={commitCreation}
          onCancelCreation={cancelCreation}
          onFocusCreatedVariable={focusCreatedVariable}
          onRemove={(id) => {
            setUnboundArgumentTypes((current) =>
              Object.fromEntries(
                Object.entries(current).filter(([key]) => key !== id),
              )
            );
            const next = {
              ...props.draft,
              variables: props.draft.variables.filter((node) => node.id !== id),
            };
            props.onDraftChange(next);
            if (selectedVariableId() === id) setSelectedVariableId(null);
          }}
          sqlSchema={props.sqlSchema}
          sqlForms={props.sqlForms}
          parameterDeclarations={parameterDeclarations()}
          onVariableRenamed={(oldId, newId) => {
            setUnboundArgumentTypes((current) => {
              const next = { ...current };
              delete next[oldId];
              if (current[oldId]) next[newId] = current[oldId];
              return next;
            });
          }}
          onArgumentUpdated={updateArgumentDefinition}
          onArgumentRemoved={removeArgumentDefinition}
          onVariablesAdded={(variableId, added) => {
            const query = props.draft.variables.find((node) =>
              node.id === variableId && node.kind === "query"
            );
            if (!query || query.query.kind !== "inline_sql") return false;
            const known = new Set(
              query.query.arguments.map((entry) => entry.name),
            );
            const args = [
              ...query.query.arguments,
              ...added.filter((entry) => !known.has(entry.name)).map((
                entry,
              ) => ({
                name: entry.name,
                variable: "",
              })),
            ];
            setUnboundArgumentTypes((current) => ({
              ...current,
              [variableId]: {
                ...current[variableId],
                ...Object.fromEntries(added.map(({ name, type }) => [
                  name,
                  type,
                ])),
              },
            }));
            props.onDraftChange({
              ...props.draft,
              variables: props.draft.variables.map((node) =>
                node.id === variableId && node.kind === "query"
                  ? { ...node, query: { ...node.query, arguments: args } }
                  : node
              ),
            });
            return true;
          }}
          actionsForQuery={props.actionsForQuery}
          previewForQuery={props.previewForQuery}
          onPlanColumnsForQuery={props.onPlanColumnsForQuery}
          onQueryChange={props.onQueryChange}
          onRunQuery={props.onRunQuery}
          onNextQuery={props.onNextQuery}
          onPreviousQuery={props.onPreviousQuery}
          onCountQuery={props.onCountQuery}
          onEditValidityChange={props.onEditorValidityChange}
        />
      </section>
    </>
  );
}
