import { createSignal, For, Show } from "solid-js";
import { CompositionGraphDesignCanvas } from "~/components/CompositionGraphDesignCanvas";
import { CompositionVariableGraphWorkspace } from "~/components/CompositionVariableGraphWorkspace";
import type { CompositionVariableCreationKind } from "~/components/CompositionVariableCreationForm";
import {
  appendCompositionGraphComponent,
  type CompositionGraphDraft,
  type CompositionQueryArgument,
  type CompositionVariableNode,
} from "~/lib/composition-variable-binding";
import type {
  SQLWorkspacePlanState,
  SQLWorkspacePreview,
} from "~/components/SQLWorkspace";
import type { SqlSchema } from "~/lib/sql";
import type { PlannedSqlColumns } from "~/lib/sql-document-columns";
import type { Form } from "~/lib/types";
import { t } from "~/lib/i18n";

export interface CompositionGraphStudioProps {
  spaceId: string;
  draft: CompositionGraphDraft;
  previewBase?: { entryId: string; revisionId: string };
  previewGeneration?: number;
  getPrincipalGeneration: () => Promise<string>;
  sqlSchema: SqlSchema;
  sqlForms?: readonly Form[];
  previewForQuery?: (variableId: string) => SQLWorkspacePreview;
  checkForQuery?: (variableId: string) => SQLWorkspacePlanState;
  onPlanColumnsForQuery?: (variableId: string) => Promise<PlannedSqlColumns>;
  onDraftChange: (draft: CompositionGraphDraft) => void;
  onQueryChange?: (
    variableId: string,
    sql: string,
    formBindings?: Array<{ relation: string; form_id: string }>,
    queryArguments?: CompositionQueryArgument[],
  ) => void;
  onRunQuery?: (variableId: string) => void;
  onCheckQuery?: (variableId: string) => void;
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
  suggestedName: string;
  returnToDesign?: boolean;
  tableComponentId?: string;
  returnToVariableId?: string;
  argumentName?: string;
  sourceSql?: string;
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

const uniqueName = (base: string, existingNames: readonly string[]) => {
  if (!existingNames.includes(base)) return base;
  let suffix = 2;
  while (existingNames.includes(`${base}_${suffix}`)) suffix++;
  return `${base}_${suffix}`;
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
  let variablePanel: HTMLElement | undefined;
  const requestCreation = (
    kind: CompositionVariableCreationKind,
    origin: VariableCreationOrigin,
    tableComponentId?: string,
  ) => {
    if (pendingCreation()) return;
    const baseName = kind === "constant"
      ? "value"
      : kind === "input"
      ? "input"
      : "query";
    setPendingCreation({
      kind,
      origin,
      previousSelection: selectedVariableId(),
      suggestedName: uniqueName(
        baseName,
        props.draft.variables.map((node) => node.id),
      ),
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
    if (pending.returnToVariableId && pending.argumentName) {
      const target = props.draft.variables.find((node) =>
        node.id === pending.returnToVariableId && node.kind === "query"
      );
      if (
        !target || target.kind !== "query" ||
        target.query.kind !== "inline_sql" ||
        target.query.sql !== pending.sourceSql ||
        target.query.arguments.some((argument) =>
          argument.name === pending.argumentName && !!argument.variable
        )
      ) return false;
      nextDraft = {
        ...nextDraft,
        variables: nextDraft.variables.map((node) =>
          node.id === target.id && node.kind === "query"
            ? {
              ...node,
              query: {
                ...node.query,
                arguments: [
                  ...node.query.arguments.filter((argument) =>
                    argument.name !== pending.argumentName
                  ),
                  { name: pending.argumentName!, variable: id },
                ],
              },
            }
            : node
        ),
      };
    }
    if (pending.origin === "design") {
      if (pending.kind === "input") {
        const componentId = (() => {
          let index = 1;
          while (
            nextDraft.components.some((component) =>
              component.id === `input_${index}`
            )
          ) index++;
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
        while (
          nextDraft.components.some((component) =>
            component.id === `table_${index}`
          )
        ) index++;
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
    setSelectedVariableId(pending.returnToVariableId ?? id);
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
      suggestedName: uniqueName(
        "rows",
        props.draft.variables.map((node) => node.id),
      ),
      returnToDesign: true,
      ...(componentId !== undefined ? { tableComponentId: componentId } : {}),
    });
    setMode("variables");
    setSelectedVariableId(null);
  };

  const requestArgumentVariableCreation = (
    queryId: string,
    argumentName: string,
    sourceSql: string,
  ) => {
    if (pendingCreation()) return;
    const query = props.draft.variables.find((node) =>
      node.id === queryId && node.kind === "query"
    );
    if (
      !query || query.kind !== "query" || query.query.kind !== "inline_sql" ||
      query.query.sql !== sourceSql ||
      query.query.arguments.some((argument) =>
        argument.name === argumentName && !!argument.variable
      )
    ) return;
    setPendingCreation({
      kind: "constant",
      origin: "variables",
      previousSelection: selectedVariableId(),
      suggestedName: uniqueName(
        argumentName,
        props.draft.variables.map((node) => node.id),
      ),
      returnToVariableId: queryId,
      argumentName,
      sourceSql,
    });
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
      const selected = props.draft.variables.find((node) => node.id === id);
      if (selected?.kind === "query") {
        variablePanel?.querySelector<HTMLElement>(
          ".compositionVariableEditor .cm-content[contenteditable='true']",
        )?.focus();
        return;
      }
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
          previewGeneration={props.previewGeneration}
          getPrincipalGeneration={props.getPrincipalGeneration}
          onSaveSnapshotReady={props.onSaveSnapshotReady}
          onDraftChange={props.onDraftChange}
          pendingInputCreation={pendingCreation()?.origin === "design"}
          pendingInputSuggestedName={pendingCreation()?.origin === "design"
            ? pendingCreation()?.suggestedName
            : undefined}
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
          pendingCreationSuggestedName={pendingCreation()?.origin ===
              "variables"
            ? pendingCreation()?.suggestedName
            : undefined}
          onRequestCreation={(kind) => requestCreation(kind, "variables")}
          onRequestArgumentVariableCreation={requestArgumentVariableCreation}
          onCommitCreation={commitCreation}
          onCancelCreation={cancelCreation}
          onFocusCreatedVariable={focusCreatedVariable}
          onRemove={(id) => {
            const next = {
              ...props.draft,
              variables: props.draft.variables.filter((node) => node.id !== id),
            };
            props.onDraftChange(next);
            if (selectedVariableId() === id) setSelectedVariableId(null);
          }}
          sqlSchema={props.sqlSchema}
          sqlForms={props.sqlForms}
          onPlanColumnsForQuery={props.onPlanColumnsForQuery}
          previewForQuery={props.previewForQuery}
          checkForQuery={props.checkForQuery}
          onQueryChange={props.onQueryChange}
          onRunQuery={props.onRunQuery}
          onCheckQuery={props.onCheckQuery}
          onNextQuery={props.onNextQuery}
          onPreviousQuery={props.onPreviousQuery}
          onCountQuery={props.onCountQuery}
          onEditValidityChange={props.onEditorValidityChange}
        />
      </section>
    </>
  );
}
