import { createSignal, For, Show } from "solid-js";
import { IconButton } from "~/components/IconButton";
import { CompositionGraphRenderer } from "~/components/CompositionGraphRenderer";
import { createCompositionDesignEvaluation } from "~/lib/composition-design-evaluation";
import { UiIcon } from "~/components/UiIcon";
import { CompositionVariableBindingPicker } from "~/components/CompositionVariableBindingPicker";
import { CompositionVariableCreationForm } from "~/components/CompositionVariableCreationForm";
import {
  appendCompositionGraphComponent,
  compositionNodeReturnsScalar,
  compositionNodeReturnsTable,
  type CompositionGraphDraft,
  type CompositionVariableNode,
  removeCompositionGraphComponent,
} from "~/lib/composition-variable-binding";
import { t } from "~/lib/i18n";

export interface CompositionGraphDesignCanvasProps {
  spaceId: string;
  getPrincipalGeneration: () => Promise<string>;
  draft: CompositionGraphDraft;
  previewBase?: { entryId: string; revisionId: string };
  onDraftChange: (draft: CompositionGraphDraft) => void;
  pendingInputCreation?: boolean;
  onRequestCreateInput?: () => void;
  onCommitCreateInput?: (name: string) => boolean;
  onCancelCreateInput?: () => void;
  onFocusCreatedInput?: () => void;
  onRequestCreateTableQuery?: (componentId?: string) => void;
  onEditVariable?: (id: string) => void;
  onSaveSnapshotReady?: (
    getter: () => {
      variables: Record<string, unknown>;
      unset_variables: string[];
    } | undefined,
  ) => void;
}

type TextComponent = Extract<
  CompositionGraphDraft["components"][number],
  { kind: "text" }
>;

const nextId = (prefix: string, values: readonly string[]) => {
  let index = 1;
  while (values.includes(`${prefix}_${index}`)) index++;
  return `${prefix}_${index}`;
};

const displayableVariables = (draft: CompositionGraphDraft) =>
  draft.variables.filter(compositionNodeReturnsScalar);

export function CompositionGraphDesignCanvas(
  props: CompositionGraphDesignCanvasProps,
) {
  const preview = createCompositionDesignEvaluation({
    draft: () => props.draft,
    scope: () => JSON.stringify({ spaceId: props.spaceId, base: props.previewBase }),
    spaceId: () => props.spaceId,
    principal: props.getPrincipalGeneration,
    base: () => props.previewBase,
  });
  props.onSaveSnapshotReady?.(preview.saveSnapshot);
  const [pickerComponentId, setPickerComponentId] = createSignal<
    string | null | undefined
  >();
  const [selectedId, setSelectedId] = createSignal<string | null>(null);
  const selected = () =>
    props.draft.components.find((component) => component.id === selectedId());
  let addButton: HTMLButtonElement | undefined;
  let addInputButton: HTMLButtonElement | undefined;
  let bindingOpener: HTMLElement | null = null;

  const focusComponent = (id: string) => {
    queueMicrotask(() => {
      document.getElementById(`composition-text-editor-${id}`)?.focus();
    });
  };
  const openTablePicker = (componentId?: string) => {
    const tables = props.draft.variables.filter(compositionNodeReturnsTable);
    if (tables.length === 0) {
      props.onRequestCreateTableQuery?.(componentId);
      return;
    }
    bindingOpener = document.activeElement instanceof HTMLElement
      ? document.activeElement
      : null;
    setPickerComponentId(componentId ?? null);
  };
  const createTableQuery = () => {
    const componentId = pickerComponentId();
    setPickerComponentId(undefined);
    props.onRequestCreateTableQuery?.(componentId ?? undefined);
  };
  const insertText = () => {
    const id = nextId("text", props.draft.components.map((item) => item.id));
    const next = appendCompositionGraphComponent(props.draft, {
      id,
      kind: "text",
      content: "",
      style: "body",
    });
    if (!next) return;
    props.onDraftChange(next);
    setSelectedId(id);
    focusComponent(id);
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
    setSelectedId(id);
  };
  const updateText = (id: string, content: string) => {
    props.onDraftChange({
      ...props.draft,
      components: props.draft.components.map((component) =>
        component.id === id && component.kind === "text"
          ? { ...component, content }
          : component
      ),
    });
  };
  const updateTextStyle = (
    id: string,
    style: TextComponent["style"],
  ) => {
    props.onDraftChange({
      ...props.draft,
      components: props.draft.components.map((component) =>
        component.id === id && component.kind === "text"
          ? { ...component, style }
          : component
      ),
    });
  };
  const insertTextVariable = (
    id: string,
    variableId: string,
    editor: HTMLTextAreaElement | undefined,
  ) => {
    const component = props.draft.components.find((item) =>
      item.id === id && item.kind === "text"
    );
    if (!component || component.kind !== "text") return;
    const start = editor?.selectionStart ?? component.content.length;
    const end = editor?.selectionEnd ?? start;
    const marker = `{{${variableId}}}`;
    const content = component.content.slice(0, start) + marker +
      component.content.slice(end);
    updateText(id, content);
    setTimeout(() => {
      const currentEditor = document.getElementById(
        `composition-text-editor-${id}`,
      );
      if (!(currentEditor instanceof HTMLTextAreaElement)) return;
      currentEditor.focus();
      currentEditor.setSelectionRange(
        start + marker.length,
        start + marker.length,
      );
    }, 0);
  };

  const orderedIds = () => props.draft.layout.rows.flatMap((row) =>
    row.items.map((item) => item.component)
  );
  const moveSelected = (direction: -1 | 1) => {
    const ids = orderedIds();
    const id = selectedId();
    const index = ids.indexOf(id ?? "");
    const target = ids[index + direction];
    if (index < 0 || !target) return;
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
              ? { ...item, component: id! }
              : item
          ),
        })),
      },
    });
    queueMicrotask(() =>
      document.querySelector<HTMLElement>(
        ".compositionGraphDesignValue--selected textarea, .compositionGraphDesignValue--selected [data-component-select]",
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
  };
  const removeSelected = () => {
    const id = selectedId();
    if (!id) return;
    const ids = orderedIds();
    const index = ids.indexOf(id);
    const adjacent = ids[index + 1] ?? ids[index - 1];
    const next = removeCompositionGraphComponent(props.draft, id);
    if (!next) return;
    props.onDraftChange(next);
    setSelectedId(adjacent ?? null);
    queueMicrotask(() => {
      if (adjacent) {
        document.querySelector<HTMLElement>(
          ".compositionGraphDesignValue--selected textarea, .compositionGraphDesignValue--selected [data-component-select]",
        )?.focus();
      } else addButton?.focus();
    });
  };
  const componentName = (component: CompositionGraphDraft["components"][number]) =>
    component.kind === "text"
      ? component.content || t("composition.studioText")
      : props.draft.variables.find((node) => node.id === component.variable)
        ?.id ?? component.variable;
  const componentActions = (
    component: CompositionGraphDraft["components"][number],
  ) => (
    <div class="compositionGraphComponentActions" role="group" aria-label={componentName(component)}>
      <Show when={component.kind === "table"}>
        <IconButton
          icon="edit"
          label={t("composition.studioChangeVariable", {
            name: componentName(component),
          })}
          onClick={() => openTablePicker(component.id)}
        />
      </Show>
      <Show when={component.kind === "input"}>
        <IconButton
          icon="edit"
          label={t("composition.studioChangeVariable", {
            name: componentName(component),
          })}
          onClick={() => component.kind === "input" &&
            props.onEditVariable?.(component.variable)}
        />
      </Show>
      <IconButton
        icon="arrow-up"
        label={t("composition.studioMoveEarlier")}
        disabled={orderedIds().indexOf(component.id) <= 0}
        onClick={() => moveSelected(-1)}
      />
      <IconButton
        icon="arrow-down"
        label={t("composition.studioMoveLater")}
        disabled={orderedIds().indexOf(component.id) >= orderedIds().length - 1}
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
          name: componentName(component),
        })}
        onClick={removeSelected}
      />
    </div>
  );

  return (
    <div class="compositionGraphDesigner">
      <section
        class="compositionGraphCanvas"
        aria-label={t("composition.studioDesign")}
      >
        <div class="compositionGraphPalette" role="group" aria-label={t("composition.studioAddToDesign")}>
          <button
            type="button"
            ref={(element) => addButton = element}
            onClick={() => openTablePicker()}
          >
            <UiIcon name="canvas-table" />
            {t("composition.studioTable")}
          </button>
          <Show
            when={props.pendingInputCreation}
            fallback={
              <Show
                when={props.draft.variables.some((node) => node.kind === "input")}
                fallback={
                  <button
                    type="button"
                    ref={(element) => addInputButton = element}
                    onClick={() => props.onRequestCreateInput?.()}
                  >
                    <UiIcon name="canvas-input" />
                    {t("composition.studioAddInput")}
                  </button>
                }
              >
                <div class="compositionGraphInputChoice">
                  <label>
                    <UiIcon name="canvas-input" />
                    <select
                      aria-label={t("composition.studioAddInput")}
                      value=""
                      onChange={(event) => event.currentTarget.value && insertInput(event.currentTarget.value)}
                    >
                      <option value="">{t("composition.studioAddInput")}</option>
                      <For each={props.draft.variables.filter((node) => node.kind === "input")}>
                        {(node) => <option value={node.id}>{node.id}</option>}
                      </For>
                    </select>
                  </label>
                  <IconButton
                    icon="plus"
                    label={t("composition.studioCreateInput")}
                    ref={(element) => addInputButton = element}
                    onClick={() => props.onRequestCreateInput?.()}
                  />
                </div>
              </Show>
            }
          >
            <CompositionVariableCreationForm
              existingNames={props.draft.variables.map((node) => node.id)}
              onCreate={(name) => props.onCommitCreateInput?.(name) ?? false}
              onCancel={() => {
                props.onCancelCreateInput?.();
                queueMicrotask(() => addInputButton?.focus());
              }}
              focusAfterCreate={() => props.onFocusCreatedInput?.()}
            />
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
          decorateComponent={(entry, value) => {
            const component = () => entry();
            const isSelected = () => selectedId() === component().id;
            const select = () => setSelectedId(component().id);
            return (
              <div
                class="compositionGraphValue compositionGraphDesignValue"
                classList={{ "compositionGraphDesignValue--selected": isSelected() }}
                data-component-id={component().id}
                data-component-select={component().kind !== "text" ? "true" : undefined}
                role={component().kind !== "text" ? "group" : undefined}
                tabIndex={component().kind !== "text" ? 0 : undefined}
                aria-label={component().kind !== "text" ? componentName(component()) : undefined}
                onClick={() => component().kind !== "text" && select()}
                onFocus={() => component().kind !== "text" && select()}
              >
                <Show
                  when={component().kind === "text" && isSelected()}
                  fallback={component().kind === "text"
                    ? (
                      <button
                        type="button"
                        class="compositionGraphTextSelect"
                        data-component-select="true"
                        aria-label={t("composition.studioEditText")}
                        onClick={select}
                      >
                        <span class={`compositionGraphText compositionGraphText--${component().kind === "text" ? component().style : "body"}`}>
                          {value}
                        </span>
                        <UiIcon name="edit" />
                      </button>
                    )
                    : value}
                >
                  {(() => {
                    const text = component() as TextComponent;
                    let editor: HTMLTextAreaElement | undefined;
                    return (
                      <div class="compositionGraphTextEditing">
                        <textarea
                          ref={(element) => editor = element}
                          id={`composition-text-editor-${text.id}`}
                          class="ui-input compositionGraphTextEditor"
                          aria-label={t("composition.studioText")}
                          value={text.content}
                          onFocus={select}
                          onInput={(event) => updateText(text.id, event.currentTarget.value)}
                        />
                        <div class="compositionGraphTextTools">
                          <select
                            class="ui-input compositionGraphTextStyle"
                            aria-label={t("composition.studioTextStyle")}
                            value={text.style}
                            onChange={(event) => updateTextStyle(text.id, event.currentTarget.value as TextComponent["style"])}
                          >
                            <For each={["title", "heading", "body", "caption"] as const}>
                              {(style) => <option value={style}>{t(`composition.studioTextStyle.${style}`)}</option>}
                            </For>
                          </select>
                          <select
                            class="ui-input compositionGraphTextVariable"
                            aria-label={t("composition.studioInsertVariable")}
                            value=""
                            onChange={(event) => event.currentTarget.value && insertTextVariable(text.id, event.currentTarget.value, editor)}
                          >
                            <option value="">{t("composition.studioInsertVariable")}</option>
                            <For each={displayableVariables(props.draft)}>
                              {(node) => <option value={node.id}>{node.id}</option>}
                            </For>
                          </select>
                        </div>
                        {componentActions(text)}
                      </div>
                    );
                  })()}
                </Show>
                <Show when={component().kind !== "text" && isSelected()}>
                  {componentActions(component())}
                </Show>
              </div>
            );
          }}
        />
      </section>
      <Show when={pickerComponentId() !== undefined}>
        <CompositionVariableBindingPicker
          draft={props.draft}
          componentId={pickerComponentId() ?? undefined}
          onCreateQuery={createTableQuery}
          onCommit={(draft) => {
            props.onDraftChange(draft);
            setSelectedId(
              (pickerComponentId() || undefined) ??
                draft.components.at(-1)?.id ?? null,
            );
          }}
          onClose={() => {
            setPickerComponentId(undefined);
            queueMicrotask(() => {
              const target = bindingOpener;
              bindingOpener = null;
              if (target?.isConnected) target.focus();
              else addButton?.focus();
            });
          }}
        />
      </Show>
    </div>
  );
}
