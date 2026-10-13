import { For, onCleanup, onMount, Show } from "solid-js";
import { Portal } from "solid-js/web";
import {
  DashboardFlowItem,
  flowSourceStatusOwner,
} from "~/components/DashboardFlowRenderer";
import { displayDefaultName } from "~/lib/composition-display-name";
import type {
  CompositionFieldNames,
  CompositionMetricQueryContext,
} from "~/components/CompositionRenderer";
import { IconButton } from "~/components/IconButton";
import { UiIcon } from "~/components/UiIcon";
import {
  addInputComponent,
  addTextDisplay,
  type CompositionDraft,
  type DraftInsertTarget,
  type DraftLayoutItem,
  type DraftLayoutRow,
  moveLayoutItem,
  moveLayoutRow,
} from "~/lib/composition-draft";
import { t } from "~/lib/i18n";
import { compositionVariableDisplayName } from "~/lib/composition-variable-name";
import { safeText } from "~/lib/display-value";
import type {
  CompositionFlowLayoutItem,
  CompositionResolvedComponentBinding,
  CompositionResolvePlan,
  CompositionTextStyle,
  CompositionVariableDefinition,
} from "~/lib/composition-api";
import type { CompositionSourcePageState } from "~/lib/composition-query-handle";

/** Transient canvas selection: component identity, never pixels. */
export const designBlockIdForComponent = (draftId: string): string => draftId;

export interface CompositionDesignCanvasProps {
  draft: CompositionDraft;
  /** Debounced shared preview plan; empty while the preview is pending. */
  plan: Pick<CompositionResolvePlan, "sources" | "component_bindings">;
  /** Transient variable values owned by the preview handle. */
  variableValues: Readonly<Record<string, unknown>>;
  sources: Record<string, CompositionSourcePageState>;
  selectedId: string | null;
  onSelect: (id: string | null) => void;
  /**
   * Soft highlight from Data source selection: block identities using the
   * selected source. Visual only, on token colors; selection stays the
   * single owner via selectedId.
   */
  highlightedIds?: ReadonlySet<string>;
  /** Single draft mutation channel: every canvas op maps onto the document. */
  onDraftChange: (draft: CompositionDraft) => void;
  /** Display insertion opens the shared kind/source picker at a target. */
  onRequestDisplayPicker: (target: DraftInsertTarget) => void;
  fieldNames?: CompositionFieldNames;
  metricQueryContext?: CompositionMetricQueryContext;
  onNext: (sourceId: string) => void;
  onPrevious: (sourceId: string) => void;
  onRetry: (sourceId: string) => void;
  paletteTarget: DraftInsertTarget | null;
  onPaletteTarget: (target: DraftInsertTarget | null) => void;
}

const blockIcon = (
  draft: CompositionDraft,
  row: DraftLayoutRow,
  index: number,
): "canvas-text" | "canvas-metric" | "canvas-table" | "canvas-input" => {
  const item = row.items[index];
  if (draft.inputs.some((input) => input.draftId === item.draftId)) {
    return "canvas-input";
  }
  const display = draft.displays.find((entry) =>
    item.kind === "component" && entry.draftId === item.draftId
  );
  if (!display || display.kind === "metric") return "canvas-metric";
  if (display.kind === "table") return "canvas-table";
  return "canvas-text";
};

const blockName = (
  draft: CompositionDraft,
  item: DraftLayoutItem,
): string => {
  const input = draft.inputs.find((entry) => entry.draftId === item.draftId);
  if (input) {
    return input.label?.trim() ||
      compositionVariableDisplayName(draft.variables, input.variable);
  }
  const display = draft.displays.find((entry) =>
    entry.draftId === item.draftId
  );
  if (!display) return t("composition.studioDisplay");
  if (display.label) return display.label;
  if (display.kind === "text") {
    return display.content.kind === "literal"
      ? display.content.text || t("composition.studioText")
      : compositionVariableDisplayName(
        draft.variables,
        display.content.variable,
      );
  }
  return displayDefaultName(display, draft.sources);
};

/**
 * Block palette as a true modal dialog. The Portal lifts the palette out of
 * the canvas column so it can never visually collide with the inspector;
 * the backdrop blocks canvas selection while open, and dismissal returns
 * focus to the invoking gap control. Data insertion is always available and
 * offers direct Form and Saved SQL selection when no source exists. Input
 * placement creates a component each time, so variables can have multiple
 * independent controls. Text insertion is always available.
 */
function PaletteDialog(props: {
  target: DraftInsertTarget;
  draft: CompositionDraft;
  onInsertText: (target: DraftInsertTarget) => void;
  onInsertInput: (target: DraftInsertTarget, variableId: string) => void;
  onRequestDisplayPicker: (target: DraftInsertTarget) => void;
  onClose: () => void;
}) {
  let dialog: HTMLDivElement | undefined;
  let opener: HTMLElement | null = null;

  // Read the insert target once while mounted: component props stay lazy
  // getters over the Show key, which throws once the dialog unmounts.
  const insertTarget = props.target;

  onMount(() => {
    opener = document.activeElement instanceof HTMLElement
      ? document.activeElement
      : null;
    const appRoot = document.getElementById("app");
    appRoot?.setAttribute("inert", "");
    queueMicrotask(() => dialog?.querySelector("button")?.focus());
    onCleanup(() => {
      appRoot?.removeAttribute("inert");
      const target = opener;
      opener = null;
      queueMicrotask(() => {
        if (target?.isConnected) target.focus();
      });
    });
  });

  const variables = () => props.draft.variables;

  const handleKeyDown = (event: KeyboardEvent) => {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      props.onClose();
      return;
    }
    if (event.key === "Tab") {
      const container = event.currentTarget as HTMLElement;
      const focusable = Array.from(
        container.querySelectorAll<HTMLElement>("button:not([disabled])"),
      );
      if (focusable.length === 0) {
        event.preventDefault();
        return;
      }
      const currentIndex = focusable.indexOf(
        document.activeElement as HTMLElement,
      );
      const nextIndex = event.shiftKey
        ? currentIndex <= 0 ? focusable.length - 1 : currentIndex - 1
        : currentIndex < 0 || currentIndex === focusable.length - 1
        ? 0
        : currentIndex + 1;
      event.preventDefault();
      focusable[nextIndex].focus();
    }
  };

  return (
    <Portal>
      <div
        class="ui-backdrop"
        onClick={(event) => {
          if (event.target === event.currentTarget) props.onClose();
        }}
      >
        <div
          ref={dialog}
          class="designPalette"
          role="dialog"
          aria-modal="true"
          aria-label={t("composition.studioAddToDesign")}
          onKeyDown={handleKeyDown}
        >
          <div class="designPaletteEntries">
            <button
              type="button"
              class="designPaletteItem"
              onClick={() => props.onInsertText(insertTarget)}
            >
              <UiIcon name="canvas-text" />
              <span>{t("composition.studioText")}</span>
            </button>
            <button
              type="button"
              class="designPaletteItem"
              onClick={() => {
                props.onClose();
                props.onRequestDisplayPicker(insertTarget);
              }}
            >
              <UiIcon name="columns" />
              <span>{t("composition.studioData")}</span>
            </button>
          </div>
          <Show when={variables().length > 0}>
            <div class="designPaletteParams">
              <span class="ui-label">{t("composition.studioVariables")}</span>
              <For each={variables()}>
                {(variable) => (
                  <button
                    type="button"
                    class="designPaletteItem"
                    aria-label={t("composition.studioPlaceInput", {
                      name: compositionVariableDisplayName(
                        props.draft.variables,
                        variable.id,
                      ),
                    })}
                    onClick={() =>
                      props.onInsertInput(insertTarget, variable.id)}
                  >
                    <UiIcon name="canvas-input" />
                    <span>
                      {compositionVariableDisplayName(
                        props.draft.variables,
                        variable.id,
                      )}
                    </span>
                  </button>
                )}
              </For>
            </div>
          </Show>
        </div>
      </div>
    </Portal>
  );
}

/**
 * Design canvas over the current draft. Blocks render through the shared
 * flow item presenter (saved-Tool look); edit affordances only add
 * selection, insertion gaps with one component palette, and the same
 * keyboard-operable up/down reorder buttons the Display list uses. Removal
 * stays with the selected block Inspector, including its narrow sheet.
 * Selection is transient Work exposed for the RA5 inspector.
 */
export function CompositionDesignCanvas(props: CompositionDesignCanvasProps) {
  const definitionById = () =>
    new Map(
      props.draft.variables.map((variable) => [
        variable.id,
        {
          id: variable.id,
          ...(variable.label ? { label: variable.label } : {}),
          type: variable.type,
          required: variable.required,
          ...(variable.default === undefined
            ? {}
            : { default: variable.default }),
          ...(variable.format ? { format: variable.format } : {}),
        } satisfies CompositionVariableDefinition,
      ]),
    );
  const bindingById = () =>
    new Map(
      props.plan.component_bindings.map((binding) => [
        binding.component_id,
        binding,
      ]),
    );
  const sourceById = () =>
    new Map(props.plan.sources.map((source) => [source.source_id, source]));
  const texts = (): Record<
    string,
    { text: string; style: CompositionTextStyle }
  > => {
    const entries: Record<
      string,
      { text: string; style: CompositionTextStyle }
    > = {};
    for (const display of props.draft.displays) {
      if (display.kind === "text") {
        entries[display.draftId] = {
          text: display.content.kind === "literal"
            ? display.content.text
            : safeText(props.variableValues[display.content.variable]),
          style: display.style,
        };
      }
    }
    return entries;
  };
  const visibleRows = () =>
    props.draft.layoutRows.filter((row) => row.items.length > 0);
  const flowItem = (item: DraftLayoutItem): CompositionFlowLayoutItem => ({
    kind: "component",
    component: item.draftId,
  });
  const flowRows = () =>
    visibleRows().map((row) => ({
      ...row,
      items: row.items.map(flowItem),
    }));
  const statusOwner = () =>
    flowSourceStatusOwner(flowRows(), bindingById(), sourceById());

  const gapKey = (target: DraftInsertTarget): string =>
    `${target.rowId ?? `new:${target.rowIndex}`}:${target.itemIndex}`;
  const paletteKey = () =>
    props.paletteTarget ? gapKey(props.paletteTarget) : null;
  const togglePalette = (target: DraftInsertTarget) => {
    props.onPaletteTarget(
      paletteKey() === gapKey(target) ? null : target,
    );
  };

  const insertText = (target: DraftInsertTarget) => {
    const added = addTextDisplay(props.draft, {}, target);
    if (added.ok && added.draftId) {
      props.onDraftChange(added.draft);
      props.onSelect(designBlockIdForComponent(added.draftId));
      props.onPaletteTarget(null);
    }
  };

  const insertInput = (target: DraftInsertTarget, variableId: string) => {
    const placed = addInputComponent(props.draft, variableId, target);
    if (placed.ok && placed.draftId) {
      props.onDraftChange(placed.draft);
      props.onSelect(designBlockIdForComponent(placed.draftId));
      props.onPaletteTarget(null);
    }
  };

  const moveRow = (rowId: string, direction: "up" | "down") => {
    const moved = moveLayoutRow(props.draft, rowId, direction);
    if (moved.ok) props.onDraftChange(moved.draft);
  };

  const moveItem = (
    rowId: string,
    itemIndex: number,
    direction: "up" | "down",
  ) => {
    const moved = moveLayoutItem(props.draft, rowId, itemIndex, direction);
    if (moved.ok) props.onDraftChange(moved.draft);
  };

  const renderGap = (target: DraftInsertTarget, inline: boolean) => (
    <div class={inline ? "designGap designGap--inline" : "designGap"}>
      <button
        type="button"
        class="designAdd"
        aria-label={t("composition.studioAddToDesign")}
        title={t("composition.studioAddToDesign")}
        aria-expanded={paletteKey() === gapKey(target)}
        onClick={(event) => {
          event.stopPropagation();
          togglePalette(target);
        }}
      >
        <UiIcon name="plus" />
      </button>
    </div>
  );

  const renderBlock = (
    row: DraftLayoutRow,
    item: DraftLayoutItem,
    itemIndex: number,
    rowItemCount: number,
  ) => {
    const blockId = () => designBlockIdForComponent(item.draftId);
    const selected = () => props.selectedId === blockId();
    const highlighted = () => props.highlightedIds?.has(blockId()) ?? false;
    const name = () => blockName(props.draft, item);
    const binding = (): CompositionResolvedComponentBinding | undefined => {
      const itemId = item.draftId;
      const resolved = bindingById().get(itemId);
      if (resolved) return resolved;
      const input = props.draft.inputs.find((entry) =>
        entry.draftId === itemId
      );
      if (!input) return undefined;
      return {
        component_id: itemId,
        kind: "input",
        variable_id: input.variable,
        ...(input.label ? { label: input.label } : {}),
      };
    };
    const inputDisplayName = () => {
      const current = binding();
      if (current?.kind !== "input") return undefined;
      return current.label?.trim() ||
        compositionVariableDisplayName(
          props.draft.variables,
          current.variable_id,
        );
    };
    const ownsSourceStatus = () => {
      const current = binding();
      return current !== undefined && current.kind !== "text" &&
        current.kind !== "input" &&
        statusOwner().get(
            (current as { source_id: string }).source_id,
          ) === current.component_id;
    };
    const toggleSelect = () => {
      props.onSelect(selected() ? null : blockId());
    };
    return (
      <div
        class="designBlock"
        data-block-id={blockId()}
        data-selected={selected() || undefined}
        data-highlighted={highlighted() || undefined}
        onClick={() => props.onSelect(blockId())}
      >
        <div class="designBlockBar">
          <IconButton
            icon={blockIcon(props.draft, row, itemIndex)}
            label={t("composition.studioSelectBlock", { name: name() })}
            active={selected()}
            onClick={(event) => {
              event.stopPropagation();
              toggleSelect();
            }}
          />
          <Show when={selected()}>
            <span class="designBlockActions">
              <button
                type="button"
                class="pill iconpill icononly"
                disabled={itemIndex === 0}
                aria-label={t("composition.studioMoveUp", { name: name() })}
                title={t("composition.studioMoveUp", { name: name() })}
                onClick={(event) => {
                  event.stopPropagation();
                  moveItem(row.id, itemIndex, "up");
                }}
              >
                <span aria-hidden="true">↑</span>
              </button>
              <button
                type="button"
                class="pill iconpill icononly"
                disabled={itemIndex === rowItemCount - 1}
                aria-label={t("composition.studioMoveDown", { name: name() })}
                title={t("composition.studioMoveDown", { name: name() })}
                onClick={(event) => {
                  event.stopPropagation();
                  moveItem(row.id, itemIndex, "down");
                }}
              >
                <span aria-hidden="true">↓</span>
              </button>
            </span>
          </Show>
        </div>
        <div
          class="designBlockContent"
          onClick={(event) => {
            event.stopPropagation();
          }}
        >
          <DashboardFlowItem
            item={flowItem(item)}
            binding={binding()}
            texts={texts()}
            definition={binding()?.kind === "input"
              ? definitionById().get(binding()!.variable_id)
              : undefined}
            inputDisplayName={inputDisplayName()}
            variableValues={props.variableValues}
            sources={props.sources}
            sourceById={sourceById()}
            ownsSourceStatus={ownsSourceStatus()}
            fieldNames={props.fieldNames}
            metricQueryContext={props.metricQueryContext}
            onNext={props.onNext}
            onPrevious={props.onPrevious}
            onRetry={props.onRetry}
          />
        </div>
      </div>
    );
  };

  return (
    <div class="compositionFlow designCanvas">
      <For each={visibleRows()}>
        {(row, rowIndex) => (
          <div class="designRow" data-row-id={row.id}>
            {renderGap(
              { rowId: null, rowIndex: rowIndex(), itemIndex: 0 },
              false,
            )}
            <Show when={visibleRows().length > 1}>
              <div class="designRowBar">
                <button
                  type="button"
                  class="pill iconpill icononly"
                  disabled={rowIndex() === 0}
                  aria-label={t("composition.studioMoveRowUp", {
                    name: rowIndex() + 1,
                  })}
                  title={t("composition.studioMoveRowUp", {
                    name: rowIndex() + 1,
                  })}
                  onClick={() => moveRow(row.id, "up")}
                >
                  <span aria-hidden="true">↑</span>
                </button>
                <button
                  type="button"
                  class="pill iconpill icononly"
                  disabled={rowIndex() === visibleRows().length - 1}
                  aria-label={t("composition.studioMoveRowDown", {
                    name: rowIndex() + 1,
                  })}
                  title={t("composition.studioMoveRowDown", {
                    name: rowIndex() + 1,
                  })}
                  onClick={() => moveRow(row.id, "down")}
                >
                  <span aria-hidden="true">↓</span>
                </button>
              </div>
            </Show>
            <div class="compositionFlowRow">
              <For each={row.items}>
                {(item, itemIndex) => (
                  <>
                    <Show when={itemIndex() > 0}>
                      {renderGap(
                        {
                          rowId: row.id,
                          rowIndex: rowIndex(),
                          itemIndex: itemIndex(),
                        },
                        true,
                      )}
                    </Show>
                    {renderBlock(row, item, itemIndex(), row.items.length)}
                  </>
                )}
              </For>
              {renderGap(
                {
                  rowId: row.id,
                  rowIndex: rowIndex(),
                  itemIndex: row.items.length,
                },
                true,
              )}
            </div>
          </div>
        )}
      </For>
      <Show when={visibleRows().length === 0}>
        {renderGap({ rowId: null, rowIndex: 0, itemIndex: 0 }, false)}
      </Show>
      <Show when={props.paletteTarget}>
        {(target) => (
          <PaletteDialog
            target={target()}
            draft={props.draft}
            onInsertText={insertText}
            onInsertInput={insertInput}
            onRequestDisplayPicker={props.onRequestDisplayPicker}
            onClose={() => props.onPaletteTarget(null)}
          />
        )}
      </Show>
    </div>
  );
}
