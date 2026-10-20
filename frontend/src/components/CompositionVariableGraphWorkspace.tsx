import { createEffect, createSignal, For, onCleanup, Show } from "solid-js";
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
import type { Form } from "~/lib/types";
import { t } from "~/lib/i18n";
import { parseVariableGraphInput } from "~/lib/ugoite-client";

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
  onQueryChange?: (variableId: string, sql: string) => void;
  onRunQuery?: (variableId: string) => void;
  onNextQuery?: (variableId: string) => void;
  onVariablesAdded?: (
    variableId: string,
    variables: readonly { name: string; type: CompositionSqlArgumentType }[],
  ) => boolean | void;
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
      return { kind: "integer", value: 0 };
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
      props.onDraftChange(next);
      if (props.selectedVariableId === node.id) props.onSelect(raw.trim());
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
                  onInput={(event) =>
                    rename(node(), event.currentTarget.value)}
                />
                <IconButton
                  icon="trash"
                  label={t("composition.studioRemoveVariable", {
                    name: node().id,
                  })}
                  onClick={() => props.onRemove(node().id)}
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
                  const query = node() as Extract<
                    CompositionVariableNode,
                    { kind: "query" }
                  >;
                  const sql = query.query.kind === "inline_sql"
                    ? query.query.sql
                    : props.sqlBodies?.[query.id] ?? "";
                  const declarations =
                    props.parameterDeclarations?.[query.id] ?? [];
                  const [actions, setActions] = createSignal<
                    SQLWorkspaceAction[]
                  >([]);
                  const structureContexts = () =>
                    buildSqlStructureContexts(
                      props.sqlForms ?? [],
                      declarations.map((argument) => ({
                        name: argument.name,
                        type: argument.type,
                      })),
                      `composition-query:${query.id}`,
                    );
                  const candidates = props.draft.variables.filter((entry) =>
                    entry.id !== query.id &&
                    (entry.kind === "constant" || entry.kind === "input" ||
                      entry.kind === "query" && entry.returns.kind === "scalar")
                  );
                  return (
                    <>
                      <SQLWorkspace
                        id={`composition-variable-${query.id}`}
                        sql={sql}
                        {...(query.query.kind === "inline_sql"
                          ? {
                            onSqlChange: (sql: string) =>
                              props.onQueryChange?.(query.id, sql),
                          }
                          : { readOnly: true })}
                        schema={props.sqlSchema}
                        actions={[
                          ...actions(),
                          ...(props.actionsForQuery?.(query.id) ?? []),
                        ]}
                        variables={[]}
                        onPreview={() => props.onRunQuery?.(query.id)}
                        previewDisabled={!props.onRunQuery}
                        preview={props.previewForQuery?.(query.id) ??
                          { status: "idle", isCurrent: false }}
                        onNext={() => props.onNextQuery?.(query.id)}
                      />
                      <Show when={query.query.kind === "inline_sql"}>
                        <SqlStructureToolbar
                          sql={() => sql}
                          contexts={structureContexts}
                          onSqlChange={(nextSql) =>
                            props.onQueryChange?.(query.id, nextSql)}
                          onVariablesAdded={(variables) =>
                            props.onVariablesAdded?.(query.id, variables)}
                          reservedVariableNames={() =>
                            query.query.arguments.map((argument) =>
                              argument.name
                            )}
                          onActionsChange={setActions}
                        />
                      </Show>
                      <Show when={declarations.length}>
                        <section
                          class="compositionVariableArguments"
                          aria-label={t("sqlPage.variables")}
                        >
                          <For each={declarations}>
                            {(argument) => {
                              const bound =
                                query.query.arguments.find((entry) =>
                                  entry.name === argument.name
                                )?.variable ?? "";
                              const typed = candidates.filter((entry) =>
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
                                    value={bound}
                                    onChange={(event) =>
                                      updateArgument(
                                        query,
                                        argument.name,
                                        event.currentTarget.value,
                                      )}
                                  >
                                    <option value="">—</option>
                                    <For each={typed}>
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
