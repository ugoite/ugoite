import type { PlannedSqlColumns } from "~/lib/sql-document-columns";
import { batch, createEffect, createSignal, For, Show } from "solid-js";
import { CompositionGraphDesignCanvas } from "~/components/CompositionGraphDesignCanvas";
import { CompositionVariableGraphWorkspace } from "~/components/CompositionVariableGraphWorkspace";
import {
  appendCompositionGraphComponent,
  type CompositionBindingProvider,
  type CompositionBindingSession,
  type CompositionFormChoice,
  type CompositionGraphDraft,
  type CompositionSavedSqlChoice,
  type CompositionSavedSqlRevisionRef,
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
  draft: CompositionGraphDraft;
  previewBase?: { entryId: string; revisionId: string };
  forms: readonly CompositionFormChoice[];
  formsLoading?: boolean;
  savedSql: readonly CompositionSavedSqlChoice[];
  savedSqlLoading?: boolean;
  savedSqlSeedUnavailable?: boolean;
  initialSavedSqlBinding?: CompositionSavedSqlRevisionRef;
  provider: CompositionBindingProvider;
  session: CompositionBindingSession;
  getPrincipalGeneration: () => Promise<string>;
  isSessionCurrent: (
    session: CompositionBindingSession,
  ) => boolean | Promise<boolean>;
  sqlSchema: SqlSchema;
  sqlForms?: readonly Form[];
  sqlBodies?: Readonly<Record<string, string>>;
  actionsForQuery?: (variableId: string) => readonly SQLWorkspaceAction[];
  previewForQuery?: (variableId: string) => SQLWorkspacePreview;
  onPlanColumnsForQuery?: (variableId: string) => Promise<PlannedSqlColumns>;
  onDraftChange: (draft: CompositionGraphDraft) => void;
  onQueryChange?: (variableId: string, sql: string) => void;
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
}

type StudioMode = "design" | "variables";

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
  const [unboundArgumentTypes, setUnboundArgumentTypes] = createSignal<
    Record<string, Record<string, CompositionSqlArgumentType>>
  >({});
  let variablePanel: HTMLElement | undefined;
  let sessionId = props.session.session_id;
  createEffect(() => {
    const next = props.session.session_id;
    if (sessionId === next) return;
    sessionId = next;
    setMode("design");
    setSelectedVariableId(null);
    setUnboundArgumentTypes({});
  });
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
    const scope = props.session.session_id;
    const node = draft.variables.find((node) =>
      node.id === id && node.kind === "query"
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
    if (
      props.draft !== draft || props.session.session_id !== scope ||
      selectedVariableId() !== id
    ) {
      return false;
    }
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
    const scope = props.session.session_id;
    const node = draft.variables.find((node) =>
      node.id === id && node.kind === "query"
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
    if (
      props.draft !== draft || props.session.session_id !== scope ||
      selectedVariableId() !== id
    ) {
      return false;
    }
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

  const createDesignInput = () => {
    const variableId = (() => {
      let index = 1;
      while (
        props.draft.variables.some((node) => node.id === `input_${index}`)
      ) index++;
      return `input_${index}`;
    })();
    const componentId = (() => {
      let index = 1;
      while (
        props.draft.components.some((component) =>
          component.id === `input_${index}`
        )
      ) index++;
      return `input_${index}`;
    })();
    const input: CompositionVariableNode = {
      id: variableId,
      kind: "input",
      value_type: { scalar_type: "string", nullable: false },
      required: true,
    };
    const withVariable = {
      ...props.draft,
      variables: [...props.draft.variables, input],
    };
    const next = appendCompositionGraphComponent(withVariable, {
      id: componentId,
      kind: "input",
      variable: variableId,
    });
    if (!next) return;
    props.onDraftChange(next);
    setSelectedVariableId(variableId);
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
                onClick={() => setMode(entry)}
              >
                {entry === "design"
                  ? t("composition.studioDesign")
                  : t("composition.studioVariables")}
              </button>
            )}
          </For>
        </nav>
        <Show when={props.onSave}>
          <button
            type="button"
            class="btn primary compositionGraphSave"
            disabled={props.saveDisabled || props.saving}
            onClick={() => props.onSave?.()}
          >
            {t("composition.save")}
          </button>
        </Show>
      </div>
      <section hidden={mode() !== "design"}>
        <CompositionGraphDesignCanvas
          draft={props.draft}
          previewBase={props.previewBase}
          forms={props.forms}
          formsLoading={props.formsLoading}
          savedSql={props.savedSql}
          savedSqlLoading={props.savedSqlLoading}
          savedSqlSeedUnavailable={props.savedSqlSeedUnavailable}
          initialSavedSqlBinding={props.initialSavedSqlBinding}
          provider={props.provider}
          session={props.session}
          getPrincipalGeneration={props.getPrincipalGeneration}
          isSessionCurrent={props.isSessionCurrent}
          onDraftChange={props.onDraftChange}
          onCreateInput={createDesignInput}
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
          sqlBodies={props.sqlBodies}
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
