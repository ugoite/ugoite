import { createEffect, createSignal, For, Show } from "solid-js";
import { IconButton } from "~/components/IconButton";
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
  type CompositionVariableNode,
  removeCompositionGraphComponent,
} from "~/lib/composition-variable-binding";
import { t } from "~/lib/i18n";

export interface CompositionGraphDesignCanvasProps {
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
  onDraftChange: (draft: CompositionGraphDraft) => void;
  onCreateInput?: () => void;
}

const nextId = (prefix: string, values: readonly string[]) => {
  let index = 1;
  while (values.includes(`${prefix}_${index}`)) index++;
  return `${prefix}_${index}`;
};

const variableFor = (
  draft: CompositionGraphDraft,
  component: CompositionGraphDraft["components"][number],
): CompositionVariableNode | undefined => {
  if (component.kind === "text") {
    const content = component.content;
    return content.kind === "variable"
      ? draft.variables.find((variable) => variable.id === content.variable)
      : undefined;
  }
  return draft.variables.find((variable) => variable.id === component.variable);
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

export function CompositionGraphDesignCanvas(
  props: CompositionGraphDesignCanvasProps,
) {
  const [paletteOpen, setPaletteOpen] = createSignal(false);
  const [pickerKind, setPickerKind] = createSignal<BindingComponentKind | null>(
    props.initialSavedSqlBinding ? "table" : null,
  );
  const [selectedId, setSelectedId] = createSignal<string | null>(null);
  let sessionId = props.session.session_id;
  createEffect(() => {
    const next = props.session.session_id;
    if (sessionId === next) return;
    sessionId = next;
    setPaletteOpen(false);
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
    setPaletteOpen(false);
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
    setPaletteOpen(false);
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
    setPaletteOpen(false);
  };

  const removeSelected = () => {
    const id = selectedId();
    if (!id) return;
    const next = removeCompositionGraphComponent(props.draft, id);
    if (!next) return;
    props.onDraftChange(next);
    setSelectedId(null);
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
    <div class="compositionGraphDesigner">
      <section
        class="compositionGraphCanvas"
        aria-label={t("composition.studioDesign")}
      >
        <header class="compositionGraphCanvasHeader">
          <IconButton
            icon="plus"
            label={t("composition.studioAddToDesign")}
            ref={(element) => addButton = element}
            onClick={() => setPaletteOpen((open) => !open)}
          />
          <Show when={paletteOpen()}>
            <div
              class="compositionGraphPalette"
              role="group"
              aria-label={t("composition.studioAddToDesign")}
            >
              <button type="button" onClick={() => openBinding("table")}>
                <UiIcon name="canvas-table" />
                {t("composition.studioTable")}
              </button>
              <button type="button" onClick={() => openBinding("metric")}>
                <UiIcon name="canvas-metric" />
                {t("composition.studioMetric")}
              </button>
              <Show
                when={props.draft.variables.some((node) =>
                  node.kind === "input"
                )}
                fallback={
                  <button
                    type="button"
                    onClick={() => {
                      props.onCreateInput?.();
                      setPaletteOpen(false);
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
                    onChange={(event) => event.currentTarget.value &&
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
          </Show>
        </header>
        <For each={props.draft.layout.rows}>
          {(row) => (
            <div class="compositionGraphCanvasRow">
              <For each={row.items}>
                {(item) => {
                  const component = () =>
                    props.draft.components.find((entry) =>
                      entry.id === item.component
                    );
                  return (
                    <Show when={component()}>
                      {(entry) => {
                        const value = () => variableFor(props.draft, entry());
                        const kind = () => entry().kind;
                        const icon = () =>
                          kind() === "table"
                            ? "canvas-table"
                            : kind() === "metric"
                            ? "canvas-metric"
                            : kind() === "input"
                            ? "canvas-input"
                            : "canvas-text";
                        return (
                          <button
                            type="button"
                            class="compositionGraphBlock"
                            classList={{
                              "compositionGraphBlock--selected":
                                selectedId() === entry().id,
                            }}
                            aria-label={`${kind()}, ${
                              componentName(props.draft, entry())
                            }`}
                            aria-pressed={selectedId() === entry().id}
                            data-component-id={entry().id}
                            onClick={() => setSelectedId(entry().id)}
                          >
                            <UiIcon name={icon()} />
                            <span>{componentName(props.draft, entry())}</span>
                            <Show
                              when={value()?.kind === "query" &&
                                compositionNodeReturnsTable(value()!)}
                            >
                              <span
                                class="compositionGraphShape"
                                aria-hidden="true"
                              >
                                ▦
                              </span>
                            </Show>
                            <Show
                              when={value()?.kind === "query" &&
                                compositionNodeReturnsScalar(value()!)}
                            >
                              <span
                                class="compositionGraphShape"
                                aria-hidden="true"
                              >
                                ◉
                              </span>
                            </Show>
                          </button>
                        );
                      }}
                    </Show>
                  );
                }}
              </For>
              <Show
                when={row.items.length === 0 &&
                  props.draft.components.length === 0}
              >
                <div class="compositionGraphCanvasEmpty" aria-hidden="true">
                  <UiIcon name="columns" />
                </div>
              </Show>
            </div>
          )}
        </For>
      </section>

      <aside
        class="compositionGraphInspector"
        aria-label={t("composition.studioInspector")}
      >
        <Show
          when={selected()}
          fallback={<div class="compositionGraphInspectorEmpty" />}
        >
          {(component) => (
            <>
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
              <Show when={component().kind === "input"}>
                <div class="compositionGraphVariableBinding">
                  <UiIcon name="canvas-input" />
                  <span>
                    {props.draft.variables.find((node) =>
                      node.id === (component() as Extract<
                        CompositionGraphDraft["components"][number],
                        { kind: "input" }
                      >).variable
                    )?.id}
                  </span>
                </div>
              </Show>
              <Show when={component().kind === "text"}>
                {(() => {
                  const item = component() as Extract<
                    CompositionGraphDraft["components"][number],
                    { kind: "text" }
                  >;
                  return (
                    <div class="compositionGraphTextInspector">
                      <textarea
                        class="ui-input"
                        aria-label={t("composition.studioText")}
                        value={item.content.kind === "literal"
                          ? item.content.text
                          : ""}
                        onInput={(event) =>
                          updateText(event.currentTarget.value)}
                      />
                      <select
                        class="ui-input"
                        aria-label={t("composition.studioTextStyle")}
                        value={item.style}
                        onChange={(event) =>
                          updateTextStyle(
                            event.currentTarget.value as typeof item.style,
                          )}
                      >
                        <For
                          each={[
                            "title",
                            "heading",
                            "body",
                            "caption",
                          ] as const}
                        >
                          {(style) => (
                            <option value={style}>
                              {t(`composition.studioTextStyle.${style}`)}
                            </option>
                          )}
                        </For>
                      </select>
                      <select
                        class="ui-input"
                        aria-label={t("composition.studioVariables")}
                        value={item.content.kind === "variable"
                          ? item.content.variable
                          : ""}
                        onChange={(event) =>
                          updateTextVariable(event.currentTarget.value)}
                      >
                        <option value="">
                          {t("composition.studioTextUseLiteral")}
                        </option>
                        <For
                          each={props.draft.variables.filter((node) =>
                            node.kind === "constant" &&
                              node.value_type.scalar_type === "string" ||
                            node.kind === "input" &&
                              node.value_type.scalar_type === "string" ||
                            node.kind === "query" &&
                              node.returns.kind === "scalar" &&
                              node.returns.value_type.scalar_type === "string"
                          )}
                        >
                          {(node) => <option value={node.id}>{node.id}</option>}
                        </For>
                      </select>
                    </div>
                  );
                })()}
              </Show>
              <div class="compositionGraphInspectorActions">
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

      <Show when={pickerKind()}>
        {(kind) => (
          <CompositionVariableBindingPicker
            kind={kind()}
            draft={props.draft}
            forms={props.forms}
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
