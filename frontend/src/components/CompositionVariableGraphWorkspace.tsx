import type { PlannedSqlColumns } from "~/lib/sql-document-columns";
import {
  batch,
  createEffect,
  createResource,
  createSignal,
  For,
  onCleanup,
  Show,
} from "solid-js";
import { Portal } from "solid-js/web";
import { IconButton } from "~/components/IconButton";
import {
  SQLWorkspace,
  type SQLWorkspaceAction,
  type SQLWorkspacePreview,
} from "~/components/SQLWorkspace";
import { UiIcon } from "~/components/UiIcon";
import {
  type CompositionGraphDraft,
  type CompositionLiteral,
  type CompositionScalarType,
  type CompositionSqlArgumentType,
  compositionVariableKindIcon,
  type CompositionVariableNode,
  renameCompositionVariable,
} from "~/lib/composition-variable-binding";
import { SqlStructureToolbar } from "~/components/SqlStructureToolbar";
import { buildSqlStructureContexts, type SqlSchema } from "~/lib/sql";
import type { Form, SqlVariable } from "~/lib/types";
import { normalizeSqlDocument } from "~/lib/sql";
import { t } from "~/lib/i18n";
import { parseVariableGraphInput } from "~/lib/ugoite-client";
import {
  replaceSqlFormBindingGuard,
  retainReferencedSqlFormBindingGuards,
} from "~/lib/form-binding-guards";

const scalarTypes: CompositionScalarType[] = [
  "string",
  "boolean",
  "integer",
  "float",
  "decimal",
  "date",
  "timestamp",
  "json",
];
export interface CompositionVariableGraphWorkspaceProps {
  draft: CompositionGraphDraft;
  selectedVariableId: string | null;
  onSelect: (id: string) => void;
  onDraftChange: (draft: CompositionGraphDraft) => void;
  onRemove: (id: string) => void;
  sqlSchema: SqlSchema;
  sqlForms?: readonly Form[];
  sqlBodies?: Readonly<Record<string, string>>;
  parameterDeclarations?: Readonly<
    Record<
      string,
      readonly { name: string; type: CompositionSqlArgumentType }[]
    >
  >;
  parameterValues?: Readonly<
    Record<
      string,
      Readonly<Record<string, string | number | boolean | undefined>>
    >
  >;
  actionsForQuery?: (variableId: string) => readonly SQLWorkspaceAction[];
  previewForQuery?: (variableId: string) => SQLWorkspacePreview;
  onPlanColumnsForQuery?: (variableId: string) => Promise<PlannedSqlColumns>;
  onQueryChange?: (
    variableId: string,
    sql: string,
    formBindings?: Array<{ relation: string; form_id: string }>,
  ) => void;
  onRunQuery?: (variableId: string) => void;
  onNextQuery?: (variableId: string) => void;
  onPreviousQuery?: (variableId: string) => void;
  onCountQuery?: (variableId: string) => void;
  onVariablesAdded?: (
    variableId: string,
    variables: readonly { name: string; type: CompositionSqlArgumentType }[],
  ) => boolean | void;
  onVariableRenamed?: (oldId: string, newId: string) => void;
  onArgumentUpdated?: (
    id: string,
    oldName: string,
    argument: SqlVariable,
  ) => boolean | void | Promise<boolean | void>;
  onArgumentRemoved?: (
    id: string,
    name: string,
  ) => boolean | void | Promise<boolean | void>;
  onEditValidityChange?: (
    variableId: string,
    valid: boolean | undefined,
  ) => void;
}
const defaultLiteral = (type: CompositionScalarType): CompositionLiteral => {
  switch (type) {
    case "string":
      return { kind: "string", value: "" };
    case "boolean":
      return { kind: "boolean", value: false };
    case "integer":
      return { kind: "integer", value: "0" };
    case "float":
      return { kind: "float", value: 0 };
    case "decimal":
      return { kind: "decimal", value: "0" };
    case "date":
      return { kind: "date", value: "1970-01-01" };
    case "timestamp":
      return { kind: "timestamp", value: "1970-01-01T00:00:00Z" };
    case "json":
      return { kind: "json", value: null };
  }
};

const taggedLiteral = (
  type: CompositionScalarType,
  value: unknown,
): CompositionLiteral => ({
  kind: type,
  value,
} as CompositionLiteral);
const textValue = (value: CompositionLiteral | undefined) =>
  !value || value.kind === "null"
    ? ""
    : value.kind === "json"
    ? JSON.stringify(value.value)
    : typeof value.value === "string"
    ? value.value
    : String(value.value);

/** Variable rail and typed editors for the strict graph authoring surface. */
export function CompositionVariableGraphWorkspace(
  props: CompositionVariableGraphWorkspaceProps,
) {
  const [menu, setMenu] = createSignal(false);
  const [mobileOpen, setMobileOpen] = createSignal(false);
  const [rawValues, setRawValues] = createSignal<Record<string, string>>({});
  const [pendingOrInvalidLiterals, setPendingOrInvalidLiterals] = createSignal(
    new Set<string>(),
  );
  const parseGenerations = new Map<string, number>();
  const formGuardGenerations = new Map<string, number>();
  const [formGuardPruningPending, setFormGuardPruningPending] = createSignal<
    Record<string, boolean>
  >({});
  onCleanup(() => formGuardGenerations.clear());
  let mobileDialog: HTMLElement | undefined;
  let mobileOpener: HTMLElement | null = null;
  const selected = () =>
    props.draft.variables.find((node) => node.id === props.selectedVariableId);
  const updateNode = (next: CompositionVariableNode) =>
    props.onDraftChange({
      ...props.draft,
      variables: props.draft.variables.map((node) =>
        node.id === next.id ? next : node
      ),
    });
  const parseLiteral = (
    node: Extract<CompositionVariableNode, { kind: "constant" | "input" }>,
    raw: string,
    target: "value" | "default",
  ) => {
    const generation = (parseGenerations.get(node.id) ?? 0) + 1;
    parseGenerations.set(node.id, generation);
    setRawValues((current) => ({ ...current, [node.id]: raw }));
    setPendingOrInvalidLiterals((current) => new Set(current).add(node.id));
    props.onEditValidityChange?.(node.id, false);
    void parseVariableGraphInput(node.value_type, raw).then((parsed) => {
      if (parseGenerations.get(node.id) !== generation) return;
      const current = props.draft.variables.find((entry) =>
        entry.id === node.id
      );
      if (
        !current || current.kind !== node.kind ||
        JSON.stringify(current.value_type) !== JSON.stringify(node.value_type)
      ) return;
      if (parsed.status === "valid") {
        updateNode(
          target === "value" && current.kind === "constant"
            ? {
              ...current,
              value: taggedLiteral(
                current.value_type.scalar_type,
                parsed.value,
              ),
            }
            : target === "default" && current.kind === "input"
            ? {
              ...current,
              default: taggedLiteral(
                current.value_type.scalar_type,
                parsed.value,
              ),
            }
            : current,
        );
        setPendingOrInvalidLiterals((current) => {
          const next = new Set(current);
          next.delete(node.id);
          return next;
        });
        props.onEditValidityChange?.(node.id, true);
      } else if (parsed.status === "unset" && target === "default") {
        if (current.kind === "input") {
          updateNode({ ...current, default: undefined });
        }
        setPendingOrInvalidLiterals((current) => {
          const next = new Set(current);
          next.delete(node.id);
          return next;
        });
        props.onEditValidityChange?.(node.id, true);
      } else {
        setPendingOrInvalidLiterals((current) => new Set(current).add(node.id));
        props.onEditValidityChange?.(node.id, false);
      }
    }).catch(() => {
      if (parseGenerations.get(node.id) !== generation) return;
      setPendingOrInvalidLiterals((current) => new Set(current).add(node.id));
      props.onEditValidityChange?.(node.id, false);
    });
  };
  const add = (kind: "constant" | "input" | "query") => {
    const stem = kind === "constant" ? "value" : kind;
    let index = 1;
    while (
      props.draft.variables.some((node) => node.id === `${stem}_${index}`)
    ) index++;
    const id = `${stem}_${index}`;
    const node: CompositionVariableNode = kind === "constant"
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
        returns: {
          kind: "scalar",
          value_type: { scalar_type: "integer", nullable: false },
        },
        query: { kind: "inline_sql", sql: "SELECT 1", arguments: [] },
      };
    props.onDraftChange({
      ...props.draft,
      variables: [...props.draft.variables, node],
    });
    props.onSelect(id);
    setMenu(false);
    setMobileOpen(false);
  };
  const closeMobile = () => setMobileOpen(false);
  const removeVariable = (variableId: string) => {
    parseGenerations.set(
      variableId,
      (parseGenerations.get(variableId) ?? 0) + 1,
    );
    setRawValues((current) => {
      const next = { ...current };
      delete next[variableId];
      return next;
    });
    setPendingOrInvalidLiterals((current) => {
      const next = new Set(current);
      next.delete(variableId);
      return next;
    });
    props.onEditValidityChange?.(variableId, undefined);
    props.onRemove(variableId);
  };
  const openMobile = (event: MouseEvent) => {
    mobileOpener = event.currentTarget as HTMLElement;
    setMobileOpen(true);
  };
  createEffect(() => {
    if (!mobileOpen()) return;
    const appRoot = document.getElementById("app");
    appRoot?.setAttribute("inert", "");
    queueMicrotask(() => mobileDialog?.focus());
    onCleanup(() => {
      appRoot?.removeAttribute("inert");
      const target = mobileOpener;
      mobileOpener = null;
      queueMicrotask(() => target?.isConnected && target.focus());
    });
  });
  const rename = (node: CompositionVariableNode, raw: string) => {
    const next = renameCompositionVariable(props.draft, node.id, raw);
    if (next) {
      const newId = raw.trim();
      const hasRaw = Object.hasOwn(rawValues(), node.id);
      const rawValue = rawValues()[node.id];
      const wasPendingOrInvalid = pendingOrInvalidLiterals().has(node.id);
      parseGenerations.set(
        node.id,
        (parseGenerations.get(node.id) ?? 0) + 1,
      );
      setRawValues((current) => {
        const values = { ...current };
        delete values[node.id];
        if (hasRaw) values[newId] = rawValue;
        return values;
      });
      setPendingOrInvalidLiterals((current) => {
        const next = new Set(current);
        next.delete(node.id);
        if (wasPendingOrInvalid && hasRaw) next.add(newId);
        return next;
      });
      props.onEditValidityChange?.(node.id, undefined);
      if (wasPendingOrInvalid && hasRaw) {
        props.onEditValidityChange?.(newId, false);
      }
      batch(() => {
        props.onVariableRenamed?.(node.id, newId);
        props.onDraftChange(next);
        if (props.selectedVariableId === node.id) props.onSelect(raw.trim());
      });
      if (hasRaw && (node.kind === "constant" || node.kind === "input")) {
        queueMicrotask(() =>
          parseLiteral(
            { ...node, id: newId },
            rawValue,
            node.kind === "constant" ? "value" : "default",
          )
        );
      }
    }
  };
  const updateArgument = (
    node: Extract<CompositionVariableNode, { kind: "query" }>,
    name: string,
    variable: string,
  ) => {
    const args = node.query.arguments.some((entry) => entry.name === name)
      ? node.query.arguments.map((entry) =>
        entry.name === name ? { name, variable } : entry
      )
      : [...node.query.arguments, { name, variable }];
    updateNode({ ...node, query: { ...node.query, arguments: args } });
  };
  const railContent = () => (
    <>
      <div class="compositionVariableRailList">
        <For each={props.draft.variables}>
          {(node) => (
            <button
              type="button"
              class="compositionVariableRailItem"
              classList={{
                "compositionVariableRailItem--active":
                  props.selectedVariableId === node.id,
              }}
              aria-current={props.selectedVariableId === node.id
                ? "true"
                : undefined}
              aria-label={node.id}
              onClick={() => {
                props.onSelect(node.id);
                setMobileOpen(false);
              }}
            >
              <UiIcon name={compositionVariableKindIcon(node)} />
              <span>{node.id}</span>
            </button>
          )}
        </For>
      </div>
      <div class="compositionVariableRailAdd">
        <IconButton
          icon="plus"
          label={t("composition.studioAddVariable")}
          onClick={() => setMenu((value) => !value)}
        />
        <Show when={menu()}>
          <div
            class="compositionVariableAddMenu"
            role="group"
            aria-label={t("composition.studioAddVariable")}
          >
            <button type="button" onClick={() => add("constant")}>
              <UiIcon name="appearance" />
              {t("composition.studioValue")}
            </button>
            <button type="button" onClick={() => add("input")}>
              <UiIcon name="canvas-input" />
              {t("composition.studioAddInput")}
            </button>
            <button type="button" onClick={() => add("query")}>
              <UiIcon name="sql" />SQL
            </button>
          </div>
        </Show>
      </div>
    </>
  );
  return (
    <div class="compositionVariableWorkspace">
      <button
        class="compositionVariableMobileOpen"
        type="button"
        aria-label={t("composition.studioShowVariables")}
        aria-expanded={mobileOpen()}
        onClick={openMobile}
      >
        <UiIcon name="menu" />
        <span>{selected()?.id ?? t("composition.studioVariables")}</span>
      </button>
      <aside
        class="compositionVariableRail compositionVariableRail--desktop"
        aria-label={t("composition.studioVariables")}
      >
        {railContent()}
      </aside>
      <Show when={mobileOpen()}>
        <Portal>
          <div class="compositionVariableRailLayer">
            <button
              class="compositionVariableRailBackdrop"
              type="button"
              aria-label={t("common.close")}
              onClick={closeMobile}
            />
            <aside
              ref={mobileDialog}
              class="compositionVariableRail compositionVariableRail--mobile"
              aria-label={t("composition.studioVariables")}
              role="dialog"
              aria-modal="true"
              tabIndex={-1}
              onKeyDown={(event) => {
                if (event.key === "Escape") {
                  event.preventDefault();
                  closeMobile();
                  return;
                }
                if (event.key !== "Tab") return;
                const controls = Array.from(
                  (event.currentTarget as HTMLElement)
                    .querySelectorAll<HTMLElement>(
                      "button:not([disabled]), input:not([disabled]), select:not([disabled])",
                    ),
                );
                if (controls.length === 0) {
                  event.preventDefault();
                  return;
                }
                const index = controls.indexOf(
                  document.activeElement as HTMLElement,
                );
                const next = event.shiftKey
                  ? index <= 0 ? controls.length - 1 : index - 1
                  : index < 0 || index === controls.length - 1
                  ? 0
                  : index + 1;
                event.preventDefault();
                controls[next].focus();
              }}
            >
              {railContent()}
            </aside>
          </div>
        </Portal>
      </Show>
      <main class="compositionVariableEditor">
        <Show
          when={selected()}
          fallback={<div class="compositionVariableEditorEmpty" />}
        >
          {(node) => (
            <>
              <div class="compositionVariableEditorHeader">
                <input
                  class="ui-input compositionVariableName"
                  aria-label={t("composition.studioVariableId")}
                  value={node().id}
                  onInput={(event) => rename(node(), event.currentTarget.value)}
                />
                <IconButton
                  icon="trash"
                  label={t("composition.studioRemoveVariable", {
                    name: node().id,
                  })}
                  onClick={() => removeVariable(node().id)}
                />
              </div>
              <Show when={node().kind === "constant"}>
                {(() => {
                  const value = node() as Extract<
                    CompositionVariableNode,
                    { kind: "constant" }
                  >;
                  return (
                    <div class="compositionVariableScalarEditor">
                      <select
                        class="ui-input"
                        aria-label={t("composition.studioVariableType")}
                        value={value.value_type.scalar_type}
                        onChange={(event) => {
                          const type = event.currentTarget
                            .value as CompositionScalarType;
                          parseGenerations.set(
                            value.id,
                            (parseGenerations.get(value.id) ?? 0) + 1,
                          );
                          setPendingOrInvalidLiterals((current) => {
                            const next = new Set(current);
                            next.delete(value.id);
                            return next;
                          });
                          setRawValues((current) => ({
                            ...current,
                            [value.id]: textValue(defaultLiteral(type)),
                          }));
                          props.onEditValidityChange?.(value.id, true);
                          updateNode({
                            ...value,
                            value_type: {
                              ...value.value_type,
                              scalar_type: type,
                            },
                            value: defaultLiteral(type),
                          });
                        }}
                      >
                        <For each={scalarTypes}>
                          {(type) => <option value={type}>{type}</option>}
                        </For>
                      </select>
                      <input
                        class="ui-input"
                        aria-label={t("composition.studioValue")}
                        value={rawValues()[value.id] ?? textValue(value.value)}
                        onInput={(event) =>
                          parseLiteral(
                            value,
                            event.currentTarget.value,
                            "value",
                          )}
                      />
                      <label class="compositionVariableRequired">
                        <input
                          type="checkbox"
                          checked={value.value_type.nullable}
                          onChange={(event) => {
                            const nullable = event.currentTarget.checked;
                            updateNode({
                              ...value,
                              value_type: {
                                ...value.value_type,
                                nullable,
                              },
                              value: nullable
                                ? { kind: "null" }
                                : defaultLiteral(
                                  value.value_type.scalar_type,
                                ),
                            });
                          }}
                        />
                        {t("composition.variableNullable")}
                      </label>
                    </div>
                  );
                })()}
              </Show>
              <Show when={node().kind === "input"}>
                {(() => {
                  const value = node() as Extract<
                    CompositionVariableNode,
                    { kind: "input" }
                  >;
                  return (
                    <div class="compositionVariableScalarEditor">
                      <select
                        class="ui-input"
                        aria-label={t("composition.studioVariableType")}
                        value={value.value_type.scalar_type}
                        onChange={(event) => {
                          parseGenerations.set(
                            value.id,
                            (parseGenerations.get(value.id) ?? 0) + 1,
                          );
                          setPendingOrInvalidLiterals((current) => {
                            const next = new Set(current);
                            next.delete(value.id);
                            return next;
                          });
                          setRawValues((current) => ({
                            ...current,
                            [value.id]: "",
                          }));
                          updateNode({
                            ...value,
                            value_type: {
                              ...value.value_type,
                              scalar_type: event.currentTarget
                                .value as CompositionScalarType,
                            },
                            default: undefined,
                          });
                          props.onEditValidityChange?.(value.id, true);
                        }}
                      >
                        <For each={scalarTypes}>
                          {(type) => <option value={type}>{type}</option>}
                        </For>
                      </select>
                      <input
                        class="ui-input"
                        aria-label={t("composition.studioVariableDefault")}
                        value={rawValues()[value.id] ??
                          textValue(value.default)}
                        onInput={(event) =>
                          parseLiteral(
                            value,
                            event.currentTarget.value,
                            "default",
                          )}
                      />
                      <label class="compositionVariableRequired">
                        <input
                          type="checkbox"
                          checked={value.value_type.nullable}
                          onChange={(event) =>
                            updateNode({
                              ...value,
                              value_type: {
                                ...value.value_type,
                                nullable: event.currentTarget.checked,
                              },
                              default: undefined,
                            })}
                        />
                        {t("composition.variableNullable")}
                      </label>
                      <label class="compositionVariableRequired">
                        <input
                          type="checkbox"
                          checked={value.required}
                          onChange={(event) =>
                            updateNode({
                              ...value,
                              required: event.currentTarget.checked,
                            })}
                        />
                        {t("composition.studioVariableRequired")}
                      </label>
                    </div>
                  );
                })()}
              </Show>
              <Show when={node().kind === "query"}>
                {(() => {
                  const query = () =>
                    node() as Extract<
                      CompositionVariableNode,
                      { kind: "query" }
                    >;
                  const sql = () => {
                    const definition = query().query;
                    return definition.kind === "inline_sql"
                      ? definition.sql
                      : props.sqlBodies?.[query().id] ?? "";
                  };
                  const isFormGuardPruningPending = () =>
                    formGuardPruningPending()[query().id] ?? false;
                  const updateSqlAndFormGuards = (
                    nextSql: string,
                    selected?: { name: string; form_id: string },
                  ) => {
                    const variableId = query().id;
                    const generation =
                      (formGuardGenerations.get(variableId) ?? 0) + 1;
                    formGuardGenerations.set(variableId, generation);
                    const stillOwnsSql = () =>
                      props.draft.variables.some((entry) =>
                        entry.id === variableId && entry.kind === "query" &&
                        entry.query.kind === "inline_sql" &&
                        entry.query.sql === nextSql
                      );
                    const finishPruning = () => {
                      if (
                        formGuardGenerations.get(variableId) !== generation
                      ) return;
                      setFormGuardPruningPending((current) => ({
                        ...current,
                        [variableId]: false,
                      }));
                    };
                    const guards = query().query.kind === "inline_sql"
                      ? query().query.form_bindings ?? []
                      : [];
                    setFormGuardPruningPending((current) => ({
                      ...current,
                      [variableId]: true,
                    }));
                    const candidates = guards.map((guard) => ({
                      name: guard.relation,
                      formId: guard.form_id,
                    }));
                    const immediateGuards = selected
                      ? [
                        ...guards.filter((guard) =>
                          guard.relation !== selected.name
                        ),
                        {
                          relation: selected.name,
                          form_id: selected.form_id,
                        },
                      ]
                      : guards;
                    // Keep edited SQL and the selected identity in the host
                    // draft together. Plan/run/count stay disabled until Rust
                    // has pruned guards against the parsed reference set.
                    props.onQueryChange?.(
                      variableId,
                      nextSql,
                      immediateGuards,
                    );
                    const pruning = selected
                      ? replaceSqlFormBindingGuard(nextSql, candidates, {
                        name: selected.name,
                        formId: selected.form_id,
                      })
                      : retainReferencedSqlFormBindingGuards(
                        nextSql,
                        candidates,
                      );
                    void pruning.then((retained) => {
                      if (
                        formGuardGenerations.get(variableId) === generation &&
                        stillOwnsSql()
                      ) {
                        props.onQueryChange?.(
                          variableId,
                          nextSql,
                          retained.map((guard) => ({
                            relation: guard.name,
                            form_id: guard.formId,
                          })),
                        );
                      }
                      // Pending belongs to the edited query, not whichever
                      // query happens to be selected when Rust completes.
                      // Clear it even after navigation; only the generation
                      // that set it may clear it.
                      finishPruning();
                    }).catch(() => {
                      finishPruning();
                    });
                  };
                  const declarations = () =>
                    props.parameterDeclarations?.[query().id] ?? [];
                  const [sqlArguments] = createResource(
                    () => ({
                      sql: sql(),
                      declarations: declarations().map((entry) => ({
                        ...entry,
                        description: "",
                      })),
                    }),
                    async (source) =>
                      await normalizeSqlDocument(
                        source.sql,
                        source.declarations,
                      ),
                  );
                  const argumentError = () => {
                    if (sqlArguments.loading || sqlArguments.error) {return t(
                        "composition.queryFailed",
                      );}
                    if (sqlArguments()?.document.status !== "parsed") {
                      return t(
                        "composition.queryFailed",
                      );
                    }
                    const referenced = sqlArguments()?.variables ?? [];
                    const undeclared = referenced.find((argument) =>
                      !declarations().some((entry) =>
                        entry.name === argument.name
                      )
                    );
                    if (undeclared) {
                      return t("sqlPage.undeclaredArgument", {
                        name: undeclared.name,
                      });
                    }
                    if (
                      declarations().some((argument) =>
                        !referenced.some((entry) =>
                          entry.name === argument.name
                        )
                      )
                    ) return t("sqlPage.structure.argumentUnused");
                    for (const argument of declarations()) {
                      const binding = query().query.arguments.find((entry) =>
                        entry.name === argument.name
                      );
                      const bound = props.draft.variables.find((entry) =>
                        entry.id === binding?.variable
                      );
                      const type = bound?.kind === "query"
                        ? bound.returns.kind === "scalar"
                          ? bound.returns.value_type.scalar_type
                          : undefined
                        : bound?.value_type.scalar_type;
                      if (type !== argument.type) {
                        return t(
                          "composition.chooseSqlArgumentVariable",
                          { name: argument.name },
                        );
                      }
                    }
                    return undefined;
                  };
                  createEffect(() => {
                    if (query().query.kind === "inline_sql") {
                      props.onEditValidityChange?.(
                        query().id,
                        !argumentError(),
                      );
                    }
                  });
                  const [actions, setActions] = createSignal<
                    SQLWorkspaceAction[]
                  >([]);
                  const structureContexts = () =>
                    buildSqlStructureContexts(
                      props.sqlForms ?? [],
                      (sqlArguments()?.variables ?? []).map((argument) => ({
                        name: argument.name,
                        type: argument.type,
                      })),
                      `composition-query:${query().id}`,
                    );
                  const candidates = () =>
                    props.draft.variables.filter((entry) =>
                      entry.id !== query().id &&
                      (entry.kind === "constant" || entry.kind === "input" ||
                        entry.kind === "query" &&
                          entry.returns.kind === "scalar")
                    );
                  return (
                    <>
                      <SQLWorkspace
                        id={`composition-variable-${query().id}`}
                        sql={sql()}
                        {...(query().query.kind === "inline_sql"
                          ? {
                            onSqlChange: (sql: string) =>
                              updateSqlAndFormGuards(sql),
                          }
                          : { readOnly: true })}
                        schema={props.sqlSchema}
                        actions={[
                          ...actions(),
                          ...(props.actionsForQuery?.(query().id) ?? []),
                        ]}
                        variables={[]}
                        onPreview={() => props.onRunQuery?.(query().id)}
                        previewDisabled={!props.onRunQuery ||
                          !!argumentError() ||
                          isFormGuardPruningPending()}
                        preview={props.previewForQuery?.(query().id) ??
                          { status: "idle", isCurrent: false }}
                        onNext={() => props.onNextQuery?.(query().id)}
                        onPrevious={() => props.onPreviousQuery?.(query().id)}
                        onCount={!isFormGuardPruningPending() &&
                            query().returns.kind === "table" &&
                            props.onCountQuery
                          ? () => props.onCountQuery?.(query().id)
                          : undefined}
                      />
                      <Show when={query().query.kind === "inline_sql"}>
                        <SqlStructureToolbar
                          onPlanColumns={props.onPlanColumnsForQuery
                            ? () => props.onPlanColumnsForQuery!(query().id)
                            : undefined}
                          formBindingGuards={() =>
                            (query().query.kind === "inline_sql"
                              ? query().query.form_bindings ?? []
                              : []).map((binding) => ({
                                name: binding.relation,
                                formId: binding.form_id,
                              }))}
                          scope={() => `composition-query:${query().id}`}
                          sql={sql}
                          contexts={structureContexts}
                          onSqlChange={(nextSql) =>
                            updateSqlAndFormGuards(nextSql)}
                          onFormBindingSelected={(selection) =>
                            updateSqlAndFormGuards(selection.sql, {
                              name: selection.name,
                              form_id: selection.form_id,
                            })}
                          disabled={() => isFormGuardPruningPending()}
                          onVariablesAdded={(variables) =>
                            props.onVariablesAdded?.(query().id, variables)}
                          reservedVariableNames={() =>
                            query().query.arguments.map((argument) =>
                              argument.name
                            )}
                          argumentDefinitions={() =>
                            declarations().map((entry) => ({
                              ...entry,
                              description: "",
                            }))}
                          scalarVariables={props.draft.variables.flatMap(
                            (node) => {
                              const type = node.kind === "query"
                                ? node.returns.kind === "scalar"
                                  ? node.returns.value_type.scalar_type
                                  : undefined
                                : node.value_type.scalar_type;
                              return type && type !== "json" &&
                                  type !== "decimal"
                                ? [{ id: node.id, name: node.id, type }]
                                : [];
                            },
                          )}
                          argumentBindings={() =>
                            query().query.arguments.map((argument) => ({
                              name: argument.name,
                              variableId: argument.variable,
                            }))}
                          onFilterEdit={(change) => {
                            const current = props.draft.variables.find((node) =>
                              node.id === query().id
                            );
                            if (
                              change.expectedScope !==
                                `composition-query:${query().id}` ||
                              sql() !== change.expectedSql ||
                              current?.kind !== "query" ||
                              current.query.kind !== "inline_sql"
                            ) return false;
                            const currentBindings = current.query.arguments.map(
                              (argument) => ({
                                name: argument.name,
                                variableId: argument.variable,
                              }),
                            );
                            if (
                              JSON.stringify(currentBindings) !==
                                JSON.stringify(
                                  change.expectedBindings ?? currentBindings,
                                )
                            ) return false;
                            const currentArguments = declarations().map((
                              entry,
                            ) => ({
                              ...entry,
                              description: "",
                            }));
                            if (
                              JSON.stringify(currentArguments) !==
                                JSON.stringify(
                                  change.expectedArguments ?? currentArguments,
                                )
                            ) return false;
                            const argumentsNext =
                              (change.resultBindings ?? currentBindings).map((
                                binding,
                              ) => ({
                                name: binding.name,
                                variable: binding.variableId,
                              }));
                            if (
                              change.resultArguments &&
                              change.resultArguments.length !==
                                argumentsNext.length
                            ) return false;
                            const next: CompositionVariableNode = {
                              ...current,
                              query: {
                                ...current.query,
                                sql: change.resultSql,
                                arguments: argumentsNext,
                              },
                            };
                            props.onDraftChange({
                              ...props.draft,
                              variables: props.draft.variables.map((node) =>
                                node.id === current.id ? next : node
                              ),
                            });
                            return true;
                          }}
                          argumentDefaultsEnabled={false}
                          onArgumentAdded={(argument) =>
                            props.onVariablesAdded?.(query().id, [{
                              name: argument.name,
                              type: argument.type as CompositionSqlArgumentType,
                            }])}
                          onArgumentUpdated={(oldName, argument) =>
                            props.onArgumentUpdated?.(
                              query().id,
                              oldName,
                              argument,
                            )}
                          onArgumentRemoved={(name) =>
                            props.onArgumentRemoved?.(query().id, name)}
                          argumentsDisabled={() =>
                            sqlArguments.loading || !!sqlArguments.error}
                          onActionsChange={setActions}
                        />
                      </Show>
                      <Show when={argumentError()}>
                        {(error) => (
                          <p class="ui-text-danger" role="alert">{error()}</p>
                        )}
                      </Show>
                      <Show when={declarations().length}>
                        <section
                          class="compositionVariableArguments"
                          aria-label={t("sqlPage.variables")}
                        >
                          <For each={declarations()}>
                            {(argument) => {
                              const bound = () =>
                                query().query.arguments.find((entry) =>
                                  entry.name === argument.name
                                )?.variable ?? "";
                              const typed = () =>
                                candidates().filter((entry) =>
                                  (entry.kind === "query"
                                    ? entry.returns.kind === "scalar"
                                      ? entry.returns.value_type.scalar_type
                                      : undefined
                                    : entry.value_type.scalar_type) ===
                                    argument.type
                                );
                              return (
                                <label class="compositionVariableArgumentRow">
                                  <span>{argument.name}</span>
                                  <select
                                    class="ui-input"
                                    aria-label={argument.name}
                                    value={bound()}
                                    onChange={(event) =>
                                      updateArgument(
                                        query(),
                                        argument.name,
                                        event.currentTarget.value,
                                      )}
                                  >
                                    <option value="">—</option>
                                    <For each={typed()}>
                                      {(entry) => (
                                        <option value={entry.id}>
                                          {entry.id}
                                        </option>
                                      )}
                                    </For>
                                  </select>
                                </label>
                              );
                            }}
                          </For>
                        </section>
                      </Show>
                    </>
                  );
                })()}
              </Show>
            </>
          )}
        </Show>
      </main>
    </div>
  );
}
