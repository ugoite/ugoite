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
  type SQLWorkspacePlanState,
  type SQLWorkspacePreview,
} from "~/components/SQLWorkspace";
import { UiIcon } from "~/components/UiIcon";
import {
  type CompositionGraphDraft,
  type CompositionLiteral,
  type CompositionQueryArgument,
  type CompositionScalarType,
  type CompositionSqlArgumentType,
  compositionVariableKindIcon,
  type CompositionVariableNode,
  renameCompositionVariable,
} from "~/lib/composition-variable-binding";
import { type SqlSchema } from "~/lib/sql";
import type { PlannedSqlColumns } from "~/lib/sql-document-columns";
import type { Form } from "~/lib/types";
import { normalizeSqlDocument } from "~/lib/sql";
import { SqlFormSourceControl } from "~/components/SqlFormSourceControl";
import { t } from "~/lib/i18n";
import {
  parseCompositionTextTemplate,
  parseVariableGraphInput,
} from "~/lib/ugoite-client";
import {
  replaceSqlFormBindingGuard,
  retainReferencedSqlFormBindingGuards,
} from "~/lib/form-binding-guards";
import {
  CompositionVariableCreationForm,
  type CompositionVariableCreationKind,
} from "~/components/CompositionVariableCreationForm";

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
const sqlArgumentTypes = new Set<CompositionSqlArgumentType>([
  "string",
  "boolean",
  "integer",
  "float",
  "date",
  "timestamp",
]);

const suggestedVariableName = (
  kind: CompositionVariableCreationKind,
  existingNames: readonly string[],
): string => {
  const base = kind === "constant"
    ? "value"
    : kind === "input"
    ? "input"
    : "query";
  if (!existingNames.includes(base)) return base;
  let suffix = 2;
  while (existingNames.includes(`${base}_${suffix}`)) suffix++;
  return `${base}_${suffix}`;
};
export interface CompositionVariableGraphWorkspaceProps {
  draft: CompositionGraphDraft;
  selectedVariableId: string | null;
  onSelect: (id: string) => void;
  onDraftChange: (draft: CompositionGraphDraft) => void;
  pendingCreationKind?: CompositionVariableCreationKind;
  pendingCreationSuggestedName?: string;
  pendingQueryVariableId?: string;
  pendingInputVariableId?: string;
  onUsePendingInput?: () => void;
  onCancelPendingInput?: () => void;
  onUsePendingQuery?: () => void;
  onCancelPendingQuery?: () => void;
  onRequestCreation?: (kind: CompositionVariableCreationKind) => void;
  onRequestArgumentVariableCreation?: (
    queryId: string,
    argumentName: string,
    sourceSql: string,
    kind?: CompositionVariableCreationKind,
  ) => void;
  onCommitCreation?: (name: string) => boolean;
  onCancelCreation?: () => boolean | void;
  onFocusCreatedVariable?: () => void;
  onRemove: (id: string) => void;
  sqlSchema: SqlSchema;
  sqlForms?: readonly Form[];
  onPlanColumnsForQuery?: (variableId: string) => Promise<PlannedSqlColumns>;
  checkForQuery?: (variableId: string) => SQLWorkspacePlanState;
  parameterValues?: Readonly<
    Record<
      string,
      Readonly<Record<string, string | number | boolean | undefined>>
    >
  >;
  previewForQuery?: (variableId: string) => SQLWorkspacePreview;
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
  onVariableRenamed?: (oldId: string, newId: string) => void;
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
  const [argumentCreationMenu, setArgumentCreationMenu] = createSignal<
    string | null
  >(null);
  const [mobileOpen, setMobileOpen] = createSignal(false);
  const [rawValues, setRawValues] = createSignal<Record<string, string>>({});
  const [pendingOrInvalidLiterals, setPendingOrInvalidLiterals] = createSignal(
    new Set<string>(),
  );
  const parseGenerations = new Map<string, number>();
  const renameGenerations = new Map<string, number>();
  const [invalidRenames, setInvalidRenames] = createSignal(new Set<string>());
  const formGuardGenerations = new Map<string, number>();
  const [formGuardPruningPending, setFormGuardPruningPending] = createSignal<
    Record<string, boolean>
  >({});
  onCleanup(() => {
    formGuardGenerations.clear();
    renameGenerations.clear();
  });
  let mobileDialog: HTMLElement | undefined;
  let mobileOpener: HTMLElement | null = null;
  let addVariableButton: HTMLButtonElement | undefined;
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
  const requestCreation = (kind: CompositionVariableCreationKind) => {
    props.onRequestCreation?.(kind);
    setMenu(false);
    setMobileOpen(false);
  };
  const cancelCreation = () => {
    const target = mobileOpener;
    const focusHandled = props.onCancelCreation?.();
    queueMicrotask(() => {
      if (focusHandled) return;
      if (target?.isConnected) target.focus();
      else addVariableButton?.focus();
      if (target === mobileOpener) mobileOpener = null;
    });
  };
  const cancelPendingInput = (variableId: string) => {
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
    props.onCancelPendingInput?.();
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
      if (props.pendingCreationKind) return;
      const target = mobileOpener;
      mobileOpener = null;
      queueMicrotask(() => target?.isConnected && target.focus());
    });
  });
  const rename = (node: CompositionVariableNode, raw: string) => {
    const newId = raw.trim();
    const sourceDraft = props.draft;
    const next = renameCompositionVariable(sourceDraft, node.id, newId);
    if (!next) return;
    const generation = (renameGenerations.get(node.id) ?? 0) + 1;
    renameGenerations.set(node.id, generation);
    setInvalidRenames((current) => new Set(current).add(node.id));
    props.onEditValidityChange?.(node.id, false);
    const rewritten = Promise.all(
      sourceDraft.components.map(async (component) => {
        if (component.kind !== "text") return component;
        const { segments } = await parseCompositionTextTemplate(
          component.content,
        );
        const content = segments.map((segment) =>
          segment.kind === "literal"
            ? segment.text.replaceAll("{{", "\\{{")
            : `{{${segment.variable === node.id ? newId : segment.variable}}}`
        ).join("");
        return { ...component, content };
      }),
    );
    void rewritten.then((components) => {
      if (
        renameGenerations.get(node.id) !== generation ||
        props.draft !== sourceDraft
      ) return;
      const current = sourceDraft.variables.find((entry) =>
        entry.id === node.id
      );
      if (!current) return;
      const renamed = {
        ...next,
        components,
      };
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
      setInvalidRenames((current) => {
        const next = new Set(current);
        next.delete(node.id);
        return next;
      });
      props.onEditValidityChange?.(node.id, undefined);
      batch(() => {
        props.onVariableRenamed?.(node.id, newId);
        props.onDraftChange(renamed);
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
      props.onEditValidityChange?.(
        newId,
        wasPendingOrInvalid && hasRaw ? false : true,
      );
    }).catch(() => {
      if (renameGenerations.get(node.id) !== generation) return;
      setInvalidRenames((current) => new Set(current).add(node.id));
      props.onEditValidityChange?.(node.id, false);
    });
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
              disabled={!!props.pendingQueryVariableId ||
                !!props.pendingInputVariableId}
              onClick={() => {
                if (props.pendingCreationKind) {
                  props.onCancelCreation?.();
                }
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
          disabled={!!props.pendingCreationKind ||
            !!props.pendingQueryVariableId ||
            !!props.pendingInputVariableId}
          ref={(element) => addVariableButton = element}
          onClick={() => setMenu((value) => !value)}
        />
        <Show when={menu()}>
          <div
            class="compositionVariableAddMenu"
            role="group"
            aria-label={t("composition.studioAddVariable")}
          >
            <button type="button" onClick={() => requestCreation("constant")}>
              <UiIcon name="appearance" />
              {t("composition.studioValue")}
            </button>
            <button type="button" onClick={() => requestCreation("input")}>
              <UiIcon name="canvas-input" />
              {t("composition.studioAddInput")}
            </button>
            <button type="button" onClick={() => requestCreation("query")}>
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
        <Show when={props.pendingCreationKind}>
          <CompositionVariableCreationForm
            existingNames={props.draft.variables.map((node) => node.id)}
            suggestedName={props.pendingCreationSuggestedName ??
              suggestedVariableName(
                props.pendingCreationKind!,
                props.draft.variables.map((node) => node.id),
              )}
            onCreate={(name) => props.onCommitCreation?.(name) ?? false}
            onCancel={cancelCreation}
            focusAfterCreate={() => props.onFocusCreatedVariable?.()}
          />
        </Show>
        <Show
          when={!props.pendingCreationKind && selected()}
          fallback={!props.pendingCreationKind
            ? <div class="compositionVariableEditorEmpty" />
            : undefined}
        >
          {(node) => (
            <>
              <div class="compositionVariableEditorHeader">
                <input
                  class="ui-input compositionVariableName"
                  aria-invalid={invalidRenames().has(node().id) || undefined}
                  aria-label={t("composition.studioVariableId")}
                  disabled={!!props.pendingQueryVariableId ||
                    props.pendingInputVariableId === node().id}
                  value={node().id}
                  onInput={(event) => rename(node(), event.currentTarget.value)}
                />
                <IconButton
                  icon="trash"
                  label={t("composition.studioRemoveVariable", {
                    name: node().id,
                  })}
                  disabled={!!props.pendingQueryVariableId ||
                    props.pendingInputVariableId === node().id}
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
                      <Show when={props.pendingInputVariableId === value.id}>
                        <div class="compositionVariableCreationActions">
                          <button
                            type="button"
                            disabled={pendingOrInvalidLiterals().has(value.id)}
                            onClick={() => props.onUsePendingInput?.()}
                          >
                            {t("composition.studioUseQuery")}
                          </button>
                          <button
                            type="button"
                            onClick={() => cancelPendingInput(value.id)}
                          >
                            {t("common.cancel")}
                          </button>
                        </div>
                      </Show>
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
                  const sql = () => query().query.sql;
                  const [shapePending, setShapePending] = createSignal(false);
                  const [shapeError, setShapeError] = createSignal<string>();
                  const setQueryShape = async (shape: "scalar" | "table") => {
                    const source = props.draft;
                    const selected = query();
                    if (selected.returns.kind === shape) return;
                    setShapePending(true);
                    setShapeError(undefined);
                    try {
                      const planned = await props.onPlanColumnsForQuery?.(
                        selected.id,
                      );
                      if (!planned || !(await planned.isCurrent())) {
                        setShapeError(t("composition.queryFailed"));
                        return;
                      }
                      const current = props.draft.variables.find((entry) =>
                        entry.id === selected.id && entry.kind === "query"
                      );
                      if (
                        props.draft !== source || current?.kind !== "query" ||
                        current.query.sql !== selected.query.sql
                      ) return;
                      const columns = planned.result_columns ?? [];
                      if (shape === "scalar") {
                        if (columns.length !== 1) {
                          setShapeError(
                            t("composition.querySingleValueNeedsOneColumn"),
                          );
                          return;
                        }
                        if (columns[0].value_type.scalar_type === "json") {
                          setShapeError(
                            t("composition.querySingleValueUnsupportedType"),
                          );
                          return;
                        }
                        updateNode({
                          ...current,
                          returns: {
                            kind: "scalar",
                            value_type: columns[0].value_type,
                          },
                        });
                      } else {
                        updateNode({
                          ...current,
                          returns: { kind: "table", columns: [...columns] },
                        });
                      }
                    } catch {
                      setShapeError(t("composition.queryFailed"));
                    } finally {
                      setShapePending(false);
                    }
                  };
                  const isFormGuardPruningPending = () =>
                    formGuardPruningPending()[query().id] ?? false;
                  const updateSqlAndFormGuards = (
                    nextSql: string,
                    selected?: { name: string; form_id: string },
                    queryArguments?: CompositionQueryArgument[],
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
                      queryArguments,
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
                          queryArguments,
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
                  const [sqlArguments] = createResource(
                    () => sql(),
                    async (source) => await normalizeSqlDocument(source),
                  );
                  const referencedArguments = () =>
                    sqlArguments()?.variables ?? [];
                  const wouldCreateArgumentCycle = (
                    candidateId: string,
                    targetId: string,
                  ) => {
                    const pending = [candidateId];
                    const visited = new Set<string>();
                    while (pending.length > 0) {
                      const id = pending.pop()!;
                      if (id === targetId) return true;
                      if (visited.has(id)) continue;
                      visited.add(id);
                      const candidate = props.draft.variables.find((entry) =>
                        entry.id === id
                      );
                      if (candidate?.kind === "query") {
                        pending.push(...candidate.query.arguments.map((arg) =>
                          arg.variable
                        ));
                      }
                    }
                    return false;
                  };
                  const bindableVariables = () =>
                    props.draft.variables.filter((entry) => {
                      if (entry.id === query().id) return false;
                      const type = entry.kind === "query"
                        ? entry.returns.kind === "scalar"
                          ? entry.returns.value_type.scalar_type
                          : undefined
                        : entry.value_type.scalar_type;
                      return type !== undefined &&
                        sqlArgumentTypes.has(
                          type as CompositionSqlArgumentType,
                        ) && !wouldCreateArgumentCycle(entry.id, query().id);
                    });
                  const manuallyUnboundArguments = new Set<string>();
                  const setArgumentBinding = (
                    name: string,
                    variable: string,
                  ) => {
                    if (
                      !referencedArguments().some((entry) =>
                        entry.name === name
                      )
                    ) return;
                    const current = query();
                    const argumentsNext = current.query.arguments.filter((
                      entry,
                    ) => entry.name !== name);
                    if (variable) {
                      manuallyUnboundArguments.delete(name);
                      argumentsNext.push({ name, variable });
                    } else {
                      manuallyUnboundArguments.add(name);
                    }
                    updateNode({
                      ...current,
                      query: { ...current.query, arguments: argumentsNext },
                    });
                  };
                  createEffect(() => {
                    if (sqlArguments.loading || sqlArguments.error) return;
                    if (sqlArguments()?.document.status !== "parsed") return;
                    const names = new Set(
                      referencedArguments().map((entry) => entry.name),
                    );
                    for (const name of manuallyUnboundArguments) {
                      if (!names.has(name)) manuallyUnboundArguments.delete(name);
                    }
                    const current = query();
                    const argumentsNext = current.query.arguments.filter((entry) =>
                      names.has(entry.name)
                    );
                    for (const name of names) {
                      if (
                        manuallyUnboundArguments.has(name) ||
                        argumentsNext.some((entry) => entry.name === name)
                      ) continue;
                      const matching = bindableVariables().filter((entry) =>
                        entry.id === name
                      );
                      if (matching.length === 1) {
                        argumentsNext.push({ name, variable: matching[0].id });
                      }
                    }
                    if (
                      JSON.stringify(argumentsNext) !==
                        JSON.stringify(current.query.arguments)
                    ) {
                      updateNode({
                        ...current,
                        query: { ...current.query, arguments: argumentsNext },
                      });
                    }
                  });
                  const argumentError = () => {
                    if (sqlArguments.loading) return undefined;
                    if (sqlArguments.error) {
                      return t("composition.queryFailed");
                    }
                    if (sqlArguments()?.document.status !== "parsed") {
                      return t("composition.queryFailed");
                    }
                    for (const argument of referencedArguments()) {
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
                      if (
                        !type ||
                        !sqlArgumentTypes.has(
                          type as CompositionSqlArgumentType,
                        )
                      ) {
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
                        !sqlArguments.loading && !argumentError(),
                      );
                    }
                  });
                  return (
                    <>
                      <select
                        class="ui-input compositionVariableQueryShape"
                        aria-label={t("composition.queryResultShape")}
                        value={query().returns.kind}
                        disabled={shapePending()}
                        onChange={(event) =>
                          void setQueryShape(
                            event.currentTarget.value as "scalar" | "table",
                          )}
                      >
                        <option value="table">
                          {t("composition.queryResultTable")}
                        </option>
                        <option value="scalar">
                          {t("composition.queryResultSingleValue")}
                        </option>
                      </select>
                      <Show when={shapeError()}>
                        {(message) => (
                          <p class="ui-text-danger" role="alert">{message()}</p>
                        )}
                      </Show>
                      <SQLWorkspace
                        id={`composition-variable-${query().id}`}
                        sql={sql()}
                        onSqlChange={(sql: string) =>
                          updateSqlAndFormGuards(sql)}
                        schema={props.sqlSchema}
                        lintProfile="composition"
                        variables={[]}
                        sqlArgumentVariables={bindableVariables().map((entry) =>
                          entry.id
                        )}
                        onSqlArgumentCompletion={(name, nextSql) => {
                          manuallyUnboundArguments.delete(name);
                          const argumentsNext = query().query.arguments.filter(
                            (entry) => entry.name !== name,
                          );
                          const matches = bindableVariables().filter((entry) =>
                            entry.id === name
                          );
                          if (matches.length === 1) {
                            argumentsNext.push({
                              name,
                              variable: matches[0].id,
                            });
                          }
                          updateSqlAndFormGuards(
                            nextSql,
                            undefined,
                            argumentsNext,
                          );
                        }}
                        planner={props.checkForQuery?.(query().id)}
                        onCheck={props.onCheckQuery
                          ? () => props.onCheckQuery?.(query().id)
                          : undefined}
                        onPreview={() => props.onRunQuery?.(query().id)}
                        previewDisabled={!props.onRunQuery ||
                          sqlArguments.loading ||
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
                        onAccept={props.pendingQueryVariableId === query().id
                          ? () => props.onUsePendingQuery?.()
                          : undefined}
                        acceptLabel={t("composition.studioUseQuery")}
                        acceptDisabled={
                          query().returns.kind !== "table" ||
                          props.checkForQuery?.(query().id)?.status !==
                            "success" ||
                          !props.checkForQuery?.(query().id)?.isCurrent ||
                          !(props.checkForQuery?.(query().id)?.resultColumns
                            ?.length)
                        }
                        onCancel={props.pendingQueryVariableId === query().id
                          ? () => props.onCancelPendingQuery?.()
                          : undefined}
                        hideRun={props.pendingQueryVariableId === query().id}
                      />
                      <SqlFormSourceControl
                        sql={sql}
                        scope={() => `composition-query:${query().id}`}
                        forms={() => props.sqlForms ?? []}
                        formBindingGuards={() =>
                          query().query.form_bindings?.map((binding) => ({
                            name: binding.relation,
                            formId: binding.form_id,
                          })) ?? []}
                        onFormBindingSelected={(selection) =>
                          updateSqlAndFormGuards(selection.sql, {
                            name: selection.name,
                            form_id: selection.form_id,
                          })}
                        disabled={isFormGuardPruningPending}
                      />
                      <Show when={argumentError()}>
                        {(error) => (
                          <p class="ui-text-danger" role="alert">{error()}</p>
                        )}
                      </Show>
                      <Show when={referencedArguments().length}>
                        <section
                          class="compositionVariableArguments"
                          aria-label={t("composition.sqlArgumentBindings")}
                        >
                          <For each={referencedArguments()}>
                            {(argument) => {
                              const bound = () =>
                                query().query.arguments.find((entry) =>
                                  entry.name === argument.name
                                )?.variable ?? "";
                              return (
                                <div class="compositionVariableArgumentRow">
                                  <span>{argument.name}</span>
                                  <select
                                    class="ui-input"
                                    aria-label={t(
                                      "composition.chooseSqlArgumentVariable",
                                      { name: argument.name },
                                    )}
                                    value={bound()}
                                    onChange={(event) =>
                                      setArgumentBinding(
                                        argument.name,
                                        event.currentTarget.value,
                                      )}
                                  >
                                    <option value="">—</option>
                                    <For each={bindableVariables()}>
                                      {(entry) => (
                                        <option value={entry.id}>
                                          {entry.id}
                                        </option>
                                      )}
                                    </For>
                                  </select>
                                  <Show
                                    when={!bound() && props
                                      .onRequestArgumentVariableCreation}
                                  >
                                    <Show
                                      when={argumentCreationMenu() ===
                                        argument.name}
                                      fallback={
                                        <IconButton
                                          icon="plus"
                                          label={t(
                                            "composition.createSqlArgumentVariable",
                                            { name: argument.name },
                                          )}
                                          disabled={!!bound()}
                                          onClick={() =>
                                            setArgumentCreationMenu(
                                              argument.name,
                                            )}
                                        />
                                      }
                                    >
                                      <div
                                        class="compositionSqlArgumentCreateMenu"
                                        role="group"
                                        aria-label={t(
                                          "composition.createSqlArgumentVariable",
                                          { name: argument.name },
                                        )}
                                      >
                                        <button
                                          type="button"
                                          onClick={() => {
                                            setArgumentCreationMenu(null);
                                            props
                                              .onRequestArgumentVariableCreation?.(
                                                query().id,
                                                argument.name,
                                                sql(),
                                                "constant",
                                              );
                                          }}
                                        >
                                          {t("composition.studioValue")}
                                        </button>
                                        <button
                                          type="button"
                                          onClick={() => {
                                            setArgumentCreationMenu(null);
                                            props
                                              .onRequestArgumentVariableCreation?.(
                                                query().id,
                                                argument.name,
                                                sql(),
                                                "input",
                                              );
                                          }}
                                        >
                                          {t("composition.studioAddInput")}
                                        </button>
                                      </div>
                                    </Show>
                                  </Show>
                                </div>
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
