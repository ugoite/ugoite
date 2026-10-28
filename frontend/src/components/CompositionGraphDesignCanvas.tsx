import { createEffect, createSignal, For, Show } from "solid-js";
import { IconButton } from "~/components/IconButton";
import { CompositionGraphRenderer } from "~/components/CompositionGraphRenderer";
import { createCompositionDesignEvaluation } from "~/lib/composition-design-evaluation";
import { UiIcon } from "~/components/UiIcon";
import { CompositionVariableBindingPicker } from "~/components/CompositionVariableBindingPicker";
import {
  appendCompositionGraphComponent,
  type BindingComponentKind,
  type CompositionBindingProvider,
  type CompositionBindingSession,
  type CompositionFormChoice,
  type CompositionGraphDraft,
  compositionNodeReturnsScalar,
  compositionNodeReturnsTable,
  type CompositionSavedSqlChoice,
  type CompositionSavedSqlRevisionRef,
  type CompositionTablePresentation,
  type CompositionTablePresentationColumn,
  removeCompositionGraphComponent,
} from "~/lib/composition-variable-binding";
import { t } from "~/lib/i18n";

export interface CompositionGraphDesignCanvasProps {
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
  onDraftChange: (draft: CompositionGraphDraft) => void;
  onCreateInput?: () => void;
  onEditVariable?: (id: string) => void;
  onSaveSnapshotReady?: (
    getter: () => {
      variables: Record<string, unknown>;
      unset_variables: string[];
    } | undefined,
  ) => void;
}

const nextId = (prefix: string, values: readonly string[]) => {
  let index = 1;
  while (values.includes(`${prefix}_${index}`)) index++;
  return `${prefix}_${index}`;
};

const componentName = (
  draft: CompositionGraphDraft,
  component: CompositionGraphDraft["components"][number],
) => {
  if (component.kind === "text") {
    return component.content.kind === "literal"
      ? component.content.text || t("composition.studioText")
      : component.content.variable;
  }
  return draft.variables.find((variable) => variable.id === component.variable)
    ?.id ?? component.variable;
};

const isEmptyLiteralText = (
  component: CompositionGraphDraft["components"][number],
) =>
  component.kind === "text" && component.content.kind === "literal" &&
  !component.content.text;

type TextComponent = Extract<
  CompositionGraphDraft["components"][number],
  { kind: "text" }
>;

type TableComponent = Extract<
  CompositionGraphDraft["components"][number],
  { kind: "table" }
>;

function CompositionTableInspector(props: {
  component: () => TableComponent;
  columns: () => Array<{ name: string }>;
  onPresentation: (presentation?: CompositionTablePresentation) => void;
}) {
  const selectedColumns = (): CompositionTablePresentationColumn[] =>
    props.component().presentation?.columns ??
      props.columns().map(({ name }) => ({
        column: name,
        label: name,
      }));
  const selectedLabel = (key: string) =>
    selectedColumns().find((item) => item.column === key)?.label ?? key;
  const selectedAccessibleLabel = (key: string, position: number) => {
    const label = selectedLabel(key);
    const sameLabelCount =
      selectedColumns().filter((item) => selectedLabel(item.column) === label)
        .length;
    return sameLabelCount > 1
      ? t("composition.studioColumnPosition", { label, position })
      : label;
  };
  const selectColumn = (column: string, selected: boolean) => {
    const current = selectedColumns();
    if (selected) {
      if (current.some((item) => item.column === column)) return;
      props.onPresentation({
        columns: [...current, { column, label: column }],
      });
      return;
    }
    if (
      current.length <= 1 &&
      props.columns().some((item) => item.name === column)
    ) return;
    props.onPresentation({
      columns: current.filter((item) => item.column !== column),
    });
  };
  const updateLabel = (column: string, label: string) => {
    props.onPresentation({
      columns: selectedColumns().map((item) =>
        item.column === column ? { ...item, label } : item
      ),
    });
  };
  const moveColumn = (column: string, direction: -1 | 1) => {
    const current = selectedColumns();
    const index = current.findIndex((item) => item.column === column);
    const target = index + direction;
    if (index < 0 || target < 0 || target >= current.length) return;
    const next = [...current];
    [next[index], next[target]] = [next[target], next[index]];
    props.onPresentation({ columns: next });
  };
  const needsShowEveryResultColumn = () => {
    const presentation = props.component().presentation;
    if (!presentation) return false;
    const columns = props.columns();
    const available = new Set(columns.map(({ name }) => name));
    const selected = presentation.columns;
    const selectedKeys = new Set(selected.map(({ column }) => column));
    return available.size !== columns.length ||
      selected.length !== selectedKeys.size ||
      selected.some(({ column }) => !available.has(column)) ||
      columns.some(({ name }) => !selectedKeys.has(name));
  };
  const showEveryResultColumn = () => {
    const columns = props.columns();
    const allKeys = new Set(columns.map(({ name }) => name));
    if (columns.length === 0 || allKeys.size !== columns.length) {
      props.onPresentation();
      return;
    }
    const available = allKeys;
    const seen = new Set<string>();
    const existing = selectedColumns().filter(({ column }) => {
      if (!available.has(column) || seen.has(column)) return false;
      seen.add(column);
      return true;
    });
    const next = [...existing];
    for (const { name } of columns) {
      if (seen.has(name)) continue;
      seen.add(name);
      next.push({ column: name, label: name });
    }
    props.onPresentation({
      columns: next,
    });
  };

  return (
    <section class="compositionGraphTableInspector">
      <h3>{t("composition.studioTableColumns")}</h3>
      <For each={selectedColumns()}>
        {(column, index) => (
          <div class="compositionGraphTableColumnSettings">
            <label>
              <span class="ui-sr-only">
                {t("composition.studioTableColumnLabel", {
                  column: selectedAccessibleLabel(column.column, index() + 1),
                })}
              </span>
              <input
                class="ui-input"
                value={column.label}
                aria-label={t("composition.studioTableColumnLabel", {
                  column: selectedAccessibleLabel(column.column, index() + 1),
                })}
                onInput={(event) =>
                  updateLabel(column.column, event.currentTarget.value)}
              />
            </label>
            <IconButton
              icon="arrow-up"
              label={t("composition.studioMoveColumnEarlier")}
              disabled={index() === 0}
              onClick={() => moveColumn(column.column, -1)}
            />
            <IconButton
              icon="arrow-down"
              label={t("composition.studioMoveColumnLater")}
              disabled={index() === selectedColumns().length - 1}
              onClick={() => moveColumn(column.column, 1)}
            />
            <IconButton
              icon="close"
              label={t("composition.studioRemoveTableColumn", {
                column: selectedAccessibleLabel(column.column, index() + 1),
              })}
              disabled={selectedColumns().length <= 1 &&
                props.columns().some((item) => item.name === column.column)}
              onClick={() => selectColumn(column.column, false)}
            />
          </div>
        )}
      </For>
      <Show
        when={props.columns().some((column) =>
          !selectedColumns().some((item) => item.column === column.name)
        )}
      >
        <details class="compositionGraphTableOptionalColumns">
          <summary>{t("composition.studioAddResultColumns")}</summary>
          <For
            each={props.columns().filter((column) =>
              !selectedColumns().some((item) => item.column === column.name)
            )}
          >
            {(column) => (
              <label class="compositionGraphTableColumnChoice">
                <input
                  type="checkbox"
                  aria-label={t("composition.studioAddResultColumn", {
                    column: column.name,
                  })}
                  onChange={(event) =>
                    selectColumn(column.name, event.currentTarget.checked)}
                />
                <span>{column.name}</span>
              </label>
            )}
          </For>
        </details>
      </Show>
      <Show when={needsShowEveryResultColumn()}>
        <button
          type="button"
          class="compositionGraphVariableChange"
          onClick={showEveryResultColumn}
        >
          {t("composition.studioShowAllColumns")}
        </button>
      </Show>
    </section>
  );
}

/** Keep the editor mounted while a live graph update replaces its definition. */
function CompositionTextInspector(props: {
  component: () => TextComponent;
  variables: () => CompositionGraphDraft["variables"];
  onText: (text: string) => void;
  onStyle: (style: TextComponent["style"]) => void;
  onVariable: (id: string) => void;
}) {
  const strings = () =>
    props.variables().filter((node) =>
      node.kind === "query"
        ? node.returns.kind === "scalar" &&
          node.returns.value_type.scalar_type === "string"
        : node.value_type.scalar_type === "string"
    );
  const text = () => {
    const value = props.component().content;
    return value.kind === "literal" ? value.text : "";
  };
  const variable = () => {
    const value = props.component().content;
    return value.kind === "variable" ? value.variable : "";
  };
  return (
    <div class="compositionGraphTextInspector">
      <Show when={props.component().content.kind === "literal"}>
        <textarea
          class="ui-input"
          aria-label={t("composition.studioText")}
          value={text()}
          onInput={(event) => props.onText(event.currentTarget.value)}
        />
      </Show>
      <select
        class="ui-input"
        aria-label={t("composition.studioTextStyle")}
        value={props.component().style}
        onChange={(event) =>
          props.onStyle(event.currentTarget.value as TextComponent["style"])}
      >
        <For each={["title", "heading", "body", "caption"] as const}>
          {(style) => (
            <option value={style}>
              {t(`composition.studioTextStyle.${style}`)}
            </option>
          )}
        </For>
      </select>
      <Show when={strings().length > 0 || variable()}>
        <select
          class="ui-input"
          aria-label={t("composition.studioVariables")}
          value={variable()}
          onChange={(event) => props.onVariable(event.currentTarget.value)}
        >
          <option value="">{t("composition.studioTextUseLiteral")}</option>
          <For each={strings()}>
            {(node) => <option value={node.id}>{node.id}</option>}
          </For>
        </select>
      </Show>
    </div>
  );
}

export function CompositionGraphDesignCanvas(
  props: CompositionGraphDesignCanvasProps,
) {
  const preview = createCompositionDesignEvaluation({
    draft: () => props.draft,
    scope: () => JSON.stringify(props.session),
    spaceId: () => props.session.space_id,
    principal: props.getPrincipalGeneration,
    base: () => props.previewBase,
  });
  props.onSaveSnapshotReady?.(preview.saveSnapshot);
  const [pickerKind, setPickerKind] = createSignal<BindingComponentKind | null>(
    props.initialSavedSqlBinding ? "table" : null,
  );
  const [selectedId, setSelectedId] = createSignal<string | null>(null);
  let sessionId = props.session.session_id;
  createEffect(() => {
    const next = props.session.session_id;
    if (sessionId === next) return;
    sessionId = next;
    setSelectedId(null);
    setPickerKind(props.initialSavedSqlBinding ? "table" : null);
  });
  let addButton: HTMLButtonElement | undefined;
  let bindingOpener: HTMLElement | null = null;
  const savedSqlChoices = () => {
    const exact = props.initialSavedSqlBinding;
    return exact
      ? props.savedSql.filter((entry) =>
        entry.entry_id === exact.entry_id &&
        entry.revision_id === exact.revision_id
      )
      : props.savedSql;
  };
  const savedSqlSeedUnavailable = () =>
    Boolean(
      props.savedSqlSeedUnavailable ||
        props.initialSavedSqlBinding && !props.savedSqlLoading &&
          savedSqlChoices().length === 0,
    );
  const selected = () =>
    props.draft.components.find((component) => component.id === selectedId());

  const openBinding = (kind: BindingComponentKind, componentId?: string) => {
    bindingOpener = document.activeElement instanceof HTMLElement
      ? document.activeElement
      : null;
    setSelectedId(componentId ?? null);
    setPickerKind(kind);
  };

  const insertText = () => {
    const id = nextId("text", props.draft.components.map((item) => item.id));
    const next = appendCompositionGraphComponent(props.draft, {
      id,
      kind: "text",
      content: { kind: "literal", text: "" },
      style: "body",
    });
    if (!next) return;
    props.onDraftChange(next);
  };

  const insertInput = (variableId: string) => {
    const id = nextId("input", props.draft.components.map((item) => item.id));
    const next = appendCompositionGraphComponent(props.draft, {
      id,
      kind: "input",
      variable: variableId,
    });
    if (!next) return;
    props.onDraftChange(next);
  };

  const orderedIds = () =>
    props.draft.layout.rows.flatMap((row) =>
      row.items.map((item) => item.component)
    );
  const moveSelected = (direction: -1 | 1) => {
    const ids = orderedIds();
    const index = ids.indexOf(selectedId() ?? "");
    const target = ids[index + direction];
    if (index < 0 || !target) return;
    const id = ids[index];
    props.onDraftChange({
      ...props.draft,
      layout: {
        ...props.draft.layout,
        rows: props.draft.layout.rows.map((row) => ({
          ...row,
          items: row.items.map((item) =>
            item.component === id
              ? { ...item, component: target }
              : item.component === target
              ? { ...item, component: id }
              : item
          ),
        })),
      },
    });
    queueMicrotask(() =>
      document.querySelector<HTMLElement>(
        ".compositionGraphDesignValue--selected button",
      )?.focus()
    );
  };
  const moveToNewRow = () => {
    const id = selectedId();
    if (!id) return;
    const rows = props.draft.layout.rows.map((row) => ({
      ...row,
      items: row.items.filter((item) => item.component !== id),
    })).filter((row) => row.items.length > 0);
    rows.push({
      id: nextId("row", props.draft.layout.rows.map((row) => row.id)),
      items: [{ kind: "component", component: id }],
    });
    props.onDraftChange({ ...props.draft, layout: { kind: "flow", rows } });
    queueMicrotask(() =>
      document.querySelector<HTMLElement>(
        ".compositionGraphDesignValue--selected button",
      )?.focus()
    );
  };
  const removeSelected = () => {
    const id = selectedId();
    if (!id) return;
    const next = removeCompositionGraphComponent(props.draft, id);
    if (!next) return;
    props.onDraftChange(next);
    const ids = orderedIds();
    const index = ids.indexOf(id);
    const adjacent = ids[index + 1] ?? ids[index - 1];
    setSelectedId(adjacent ?? null);
    queueMicrotask(() => {
      if (adjacent) {
        document.querySelector<HTMLElement>(
          ".compositionGraphDesignValue--selected button",
        )?.focus();
      } else addButton?.focus();
    });
  };

  const updateText = (value: string) => {
    const id = selectedId();
    if (!id) return;
    props.onDraftChange({
      ...props.draft,
      components: props.draft.components.map((component) =>
        component.id === id && component.kind === "text" &&
          component.content.kind === "literal"
          ? { ...component, content: { kind: "literal", text: value } }
          : component
      ),
    });
  };

  const updateTablePresentation = (
    componentId: string,
    presentation?: CompositionTablePresentation,
  ) => {
    props.onDraftChange({
      ...props.draft,
      components: props.draft.components.map((component) => {
        if (component.id !== componentId || component.kind !== "table") {
          return component;
        }
        const { presentation: _previous, ...withoutPresentation } = component;
        return presentation
          ? { ...withoutPresentation, presentation }
          : withoutPresentation;
      }),
    });
  };

  const updateTextStyle = (style: "title" | "heading" | "body" | "caption") => {
    const id = selectedId();
    if (!id) return;
    props.onDraftChange({
      ...props.draft,
      components: props.draft.components.map((component) =>
        component.id === id && component.kind === "text"
          ? { ...component, style }
          : component
      ),
    });
  };

  const updateTextVariable = (variable: string) => {
    const id = selectedId();
    if (!id) return;
    props.onDraftChange({
      ...props.draft,
      components: props.draft.components.map((component) =>
        component.id === id && component.kind === "text"
          ? {
            ...component,
            content: variable
              ? { kind: "variable", variable }
              : { kind: "literal", text: "" },
          }
          : component
      ),
    });
  };

  return (
    <div
      class="compositionGraphDesigner"
      classList={{ "compositionGraphDesigner--selected": !!selected() }}
    >
      <section
        class="compositionGraphCanvas"
        aria-label={t("composition.studioDesign")}
      >
        <div
          class="compositionGraphPalette"
          role="group"
          aria-label={t("composition.studioAddToDesign")}
        >
          <button
            type="button"
            ref={(element) => addButton = element}
            onClick={() => openBinding("table")}
          >
            <UiIcon name="canvas-table" />
            {t("composition.studioTable")}
          </button>
          <button type="button" onClick={() => openBinding("metric")}>
            <UiIcon name="canvas-metric" />
            {t("composition.studioMetric")}
          </button>
          <Show
            when={props.draft.variables.some((node) => node.kind === "input")}
            fallback={
              <button
                type="button"
                onClick={() => {
                  props.onCreateInput?.();
                }}
              >
                <UiIcon name="canvas-input" />
                {t("composition.studioAddInput")}
              </button>
            }
          >
            <label class="compositionGraphInputChoice">
              <UiIcon name="canvas-input" />
              <span>{t("composition.studioAddInput")}</span>
              <select
                aria-label={t("composition.studioAddInput")}
                value=""
                onChange={(event) =>
                  event.currentTarget.value &&
                  insertInput(event.currentTarget.value)}
              >
                <option value="">—</option>
                <For
                  each={props.draft.variables.filter((node) =>
                    node.kind === "input"
                  )}
                >
                  {(node) => <option value={node.id}>{node.id}</option>}
                </For>
              </select>
            </label>
          </Show>
          <button type="button" onClick={insertText}>
            <UiIcon name="canvas-text" />
            {t("composition.studioText")}
          </button>
        </div>
        <Show when={preview.error()}>
          {(error) => <p role="alert" class="ui-text-danger">{error()}</p>}
        </Show>
        <CompositionGraphRenderer
          draft={props.draft}
          evaluation={preview.evaluation()}
          rawInputs={preview.rawInputs()}
          inputStates={preview.inputStates()}
          blockedQueryIds={preview.blocked()}
          previousPageQueryIds={preview.previousIds()}
          pageBusy={preview.busy()}
          onInput={preview.onInput}
          onNext={preview.onNext}
          onPrevious={preview.onPrevious}
          decorateComponent={(entry, value) => (
            <div
              class="compositionGraphValue compositionGraphDesignValue"
              classList={{
                "compositionGraphDesignValue--selected":
                  selectedId() === entry().id,
              }}
              data-component-id={entry().id}
            >
              <Show
                when={isEmptyLiteralText(entry())}
                fallback={
                  <button
                    type="button"
                    class="compositionGraphSelect"
                    aria-label={`${entry().kind}, ${
                      componentName(props.draft, entry())
                    }`}
                    aria-pressed={selectedId() === entry().id}
                    onClick={() => setSelectedId(entry().id)}
                  >
                    <UiIcon name="edit" />
                  </button>
                }
              >
                <button
                  type="button"
                  class="compositionGraphEmptyText"
                  aria-pressed={selectedId() === entry().id}
                  onClick={() => setSelectedId(entry().id)}
                >
                  {t("composition.studioTextUseLiteral")}
                </button>
              </Show>
              {value}
            </div>
          )}
        />
      </section>

      <Show when={selected()}>
        <aside
          class="compositionGraphInspector"
          aria-label={t("composition.studioInspector")}
        >
          <Show
            when={selected()}
          >
            {(component) => (
              <>
                <div class="compositionGraphInspectorHeader">
                  <IconButton
                    icon="close"
                    label={t("common.close")}
                    onClick={() => {
                      setSelectedId(null);
                      queueMicrotask(() => addButton?.focus());
                    }}
                  />
                </div>
                <Show
                  when={component().kind === "table" ||
                    component().kind === "metric"}
                >
                  {(() => {
                    const item = component() as Extract<
                      CompositionGraphDraft["components"][number],
                      { kind: "table" | "metric" }
                    >;
                    const variable = () =>
                      props.draft.variables.find((node) =>
                        node.id === item.variable
                      );
                    return (
                      <div class="compositionGraphVariableBinding">
                        <span>{t("composition.studioVariables")}</span>
                        <button
                          type="button"
                          class="compositionGraphVariableChange"
                          aria-label={t("composition.studioChangeVariable", {
                            name: variable()?.id ?? item.variable,
                          })}
                          onClick={() => openBinding(item.kind, item.id)}
                        >
                          <span>{variable()?.id ?? item.variable}</span>
                          <UiIcon name="chevron-right" />
                        </button>
                      </div>
                    );
                  })()}
                </Show>
                <Show when={component().kind === "table"}>
                  {(() => {
                    const table = () => component() as TableComponent;
                    const variable = () =>
                      props.draft.variables.find((node) =>
                        node.id === table().variable
                      );
                    const columns = () => {
                      const node = variable();
                      return node?.kind === "query" &&
                          node.returns.kind === "table"
                        ? node.returns.columns
                        : [];
                    };
                    return (
                      <CompositionTableInspector
                        component={table}
                        columns={columns}
                        onPresentation={(presentation) =>
                          updateTablePresentation(table().id, presentation)}
                      />
                    );
                  })()}
                </Show>
                <Show when={component().kind === "input"}>
                  <button
                    type="button"
                    class="compositionGraphVariableChange"
                    onClick={() => {
                      const item = component();
                      if (item.kind === "input") {
                        props.onEditVariable?.(
                          item.variable,
                        );
                      }
                    }}
                  >
                    <UiIcon name="canvas-input" />
                    <span>{componentName(props.draft, component())}</span>
                    <UiIcon name="chevron-right" />
                  </button>
                </Show>
                <Show when={component().kind === "text"}>
                  <CompositionTextInspector
                    component={() => component() as TextComponent}
                    variables={() => props.draft.variables}
                    onText={updateText}
                    onStyle={updateTextStyle}
                    onVariable={updateTextVariable}
                  />
                </Show>
                <div class="compositionGraphInspectorActions">
                  <IconButton
                    icon="arrow-up"
                    label={t("composition.studioMoveEarlier")}
                    disabled={orderedIds().indexOf(selectedId() ?? "") <= 0}
                    onClick={() => moveSelected(-1)}
                  />
                  <IconButton
                    icon="arrow-down"
                    label={t("composition.studioMoveLater")}
                    disabled={orderedIds().indexOf(selectedId() ?? "") >=
                      orderedIds().length - 1}
                    onClick={() => moveSelected(1)}
                  />
                  <IconButton
                    icon="plus"
                    label={t("composition.studioMoveToNewRow")}
                    onClick={moveToNewRow}
                  />
                  <IconButton
                    icon="trash"
                    label={t("composition.studioRemoveDisplay", {
                      name: selected()
                        ? componentName(props.draft, selected()!)
                        : "",
                    })}
                    onClick={removeSelected}
                  />
                </div>
              </>
            )}
          </Show>
        </aside>
      </Show>

      <Show when={pickerKind()}>
        {(kind) => (
          <CompositionVariableBindingPicker
            kind={kind()}
            draft={props.draft}
            forms={props.forms}
            formsLoading={props.formsLoading}
            savedSql={savedSqlChoices()}
            savedSqlLoading={props.savedSqlLoading}
            savedSqlSeedUnavailable={savedSqlSeedUnavailable()}
            initialTab={props.initialSavedSqlBinding ? "saved_sql" : undefined}
            provider={props.provider}
            session={props.session}
            getPrincipalGeneration={props.getPrincipalGeneration}
            isSessionCurrent={props.isSessionCurrent}
            componentId={selectedId() ?? undefined}
            onCommit={props.onDraftChange}
            onClose={() => {
              setPickerKind(null);
              queueMicrotask(() => {
                const target = bindingOpener;
                bindingOpener = null;
                if (target?.isConnected) target.focus();
                else addButton?.focus();
              });
            }}
          />
        )}
      </Show>
    </div>
  );
}

export const compositionGraphComponentCanBind = (
  draft: CompositionGraphDraft,
  kind: BindingComponentKind,
) =>
  draft.variables.some((node) =>
    kind === "table"
      ? compositionNodeReturnsTable(node)
      : compositionNodeReturnsScalar(node)
  );
