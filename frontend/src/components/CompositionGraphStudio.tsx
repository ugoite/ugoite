import { createSignal, For, Show } from "solid-js";
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
  removeUnboundSqlArguments,
} from "~/lib/composition-variable-binding";
import type {
  SQLWorkspaceAction,
  SQLWorkspacePreview,
} from "~/components/SQLWorkspace";
import type { SqlSchema } from "~/lib/sql";
import type { Form } from "~/lib/types";
import { t } from "~/lib/i18n";

export interface CompositionGraphStudioProps {
  draft: CompositionGraphDraft;
  forms: readonly CompositionFormChoice[];
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
  onDraftChange: (draft: CompositionGraphDraft) => void;
  onQueryChange?: (variableId: string, sql: string) => void;
  onRunQuery?: (variableId: string) => void;
  onNextQuery?: (variableId: string) => void;
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
      </nav>
      <section hidden={mode() !== "design"}>
        <CompositionGraphDesignCanvas
          draft={props.draft}
          forms={props.forms}
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
        />
      </section>
      <section hidden={mode() !== "variables"}>
        <CompositionVariableGraphWorkspace
          draft={props.draft}
          selectedVariableId={selectedVariableId()}
          onSelect={setSelectedVariableId}
          onDraftChange={props.onDraftChange}
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
          sqlBodies={props.sqlBodies}
          parameterDeclarations={parameterDeclarations()}
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
          onVariablesRemoved={(variableId, names) => {
            const next = removeUnboundSqlArguments(
              props.draft,
              variableId,
              names,
            );
            if (!next) return false;
            setUnboundArgumentTypes((current) => {
              const queryTypes = { ...current[variableId] };
              for (const name of names) delete queryTypes[name];
              const remaining = { ...current };
              if (Object.keys(queryTypes).length) {
                remaining[variableId] = queryTypes;
              } else {
                delete remaining[variableId];
              }
              return remaining;
            });
            props.onDraftChange(next);
            return true;
          }}
          actionsForQuery={props.actionsForQuery}
          previewForQuery={props.previewForQuery}
          onQueryChange={props.onQueryChange}
          onRunQuery={props.onRunQuery}
          onNextQuery={props.onNextQuery}
          onEditValidityChange={props.onEditorValidityChange}
        />
      </section>
    </>
  );
}
