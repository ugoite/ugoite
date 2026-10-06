import { For, Show } from "solid-js";
import {
  DashboardFlowItem,
  flowSourceStatusOwner,
} from "~/components/DashboardFlowRenderer";
import { displayDefaultName } from "~/components/CompositionDisplayList";
import type { CompositionFieldNames } from "~/components/CompositionRenderer";
import { IconButton } from "~/components/IconButton";
import { UiIcon } from "~/components/UiIcon";
import {
  addTextDisplay,
  type CompositionDraft,
  type DraftInsertTarget,
  type DraftLayoutItem,
  type DraftLayoutRow,
  moveLayoutItem,
  moveLayoutRow,
  placeParameterControl,
  removeDisplay,
  unplacedParameters,
  unplaceParameterControl,
} from "~/lib/composition-draft";
import { t } from "~/lib/i18n";
import type {
  CompositionFlowLayoutItem,
  CompositionParameterDefinition,
  CompositionResolvePlan,
  CompositionTextStyle,
} from "~/lib/composition-api";
import type { CompositionSourcePageState } from "~/lib/composition-query-handle";

/** Transient canvas selection: component/parameter identity, never pixels. */
export const designBlockIdForComponent = (draftId: string): string => draftId;

export const designBlockIdForParameter = (parameterId: string): string =>
  `param:${parameterId}`;

export interface CompositionDesignCanvasProps {
  draft: CompositionDraft;
  /** Debounced shared preview plan; empty while the preview is pending. */
  plan: Pick<CompositionResolvePlan, "sources" | "component_bindings">;
  /** Transient parameter values owned by the preview handle. */
  parameterValues: Readonly<Record<string, unknown>>;
  sources: Record<string, CompositionSourcePageState>;
  selectedId: string | null;
  onSelect: (id: string | null) => void;
  /** Single draft mutation channel: every canvas op maps onto the document. */
  onDraftChange: (draft: CompositionDraft) => void;
  /** Metric/table insertion reuses the existing display picker at a target. */
  onRequestDisplayPicker: (target: DraftInsertTarget) => void;
  onParameterChange: (parameterId: string, value: unknown | undefined) => void;
  fieldNames?: CompositionFieldNames;
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
  if (item.kind === "parameter") return "canvas-input";
  const display = draft.displays.find((entry) =>
    item.kind === "component" && entry.draftId === item.draftId
  );
  if (!display || display.kind === "metric") return "canvas-metric";
  if (display.kind === "table") return "canvas-table";
  return "canvas-text";
};

const blockName = (
  draft: CompositionDraft,
  definitions: ReadonlyMap<string, CompositionParameterDefinition>,
  item: DraftLayoutItem,
): string => {
  if (item.kind === "parameter") {
    return definitions.get(item.parameterId)?.label ?? item.parameterId;
  }
  const display = draft.displays.find((entry) =>
    entry.draftId === item.draftId
  );
  if (!display) return item.draftId;
  if (display.label) return display.label;
  if (display.kind === "text") return display.text || display.draftId;
  return displayDefaultName(display, draft.sources);
};

/**
 * Design canvas over the current draft. Blocks render through the shared
 * flow item presenter (saved-Tool look); edit affordances only add
 * selection, insertion gaps with one component palette, and the same
 * keyboard-operable up/down reorder buttons the Display list uses.
 * Selection is transient Work exposed for the RA5 inspector.
 */
export function CompositionDesignCanvas(props: CompositionDesignCanvasProps) {
  const definitionById = () =>
    new Map(
      props.draft.parameters.map((parameter) => [
        parameter.id,
        {
          id: parameter.id,
          ...(parameter.label ? { label: parameter.label } : {}),
          type: parameter.type,
          required: parameter.required,
          ...(parameter.default === undefined
            ? {}
            : { default: parameter.default }),
          ...(parameter.format ? { format: parameter.format } : {}),
        } satisfies CompositionParameterDefinition,
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
        entries[display.draftId] = { text: display.text, style: display.style };
      }
    }
    return entries;
  };
  const visibleRows = () =>
    props.draft.layoutRows.filter((row) => row.items.length > 0);
  const statusOwner = () =>
    flowSourceStatusOwner(visibleRows(), bindingById(), sourceById());

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

  const insertParameter = (target: DraftInsertTarget, parameterId: string) => {
    const placed = placeParameterControl(props.draft, parameterId, target);
    if (placed.ok) {
      props.onDraftChange(placed.draft);
      props.onSelect(designBlockIdForParameter(parameterId));
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

  const removeBlock = (item: DraftLayoutItem) => {
    if (item.kind === "parameter") {
      const unplaced = unplaceParameterControl(props.draft, item.parameterId);
      if (unplaced.ok) {
        props.onDraftChange(unplaced.draft);
        if (props.selectedId === designBlockIdForParameter(item.parameterId)) {
          props.onSelect(null);
        }
      }
      return;
    }
    const removed = removeDisplay(props.draft, item.draftId);
    if (removed.ok) {
      props.onDraftChange(removed.draft);
      if (props.selectedId === designBlockIdForComponent(item.draftId)) {
        props.onSelect(null);
      }
    }
  };

  const flowItem = (item: DraftLayoutItem): CompositionFlowLayoutItem =>
    item.kind === "component"
      ? { kind: "component", component: item.draftId }
      : { kind: "parameter", parameter: item.parameterId };

  const renderPalette = (target: DraftInsertTarget) => {
    const unplaced = () => unplacedParameters(props.draft);
    const close = () => props.onPaletteTarget(null);
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.stopPropagation();
        close();
      }
    };
    return (
      <div
        class="designPalette"
        role="dialog"
        aria-label={t("composition.studioAddBlock")}
        onKeyDown={onKeyDown}
      >
        <div class="designPaletteEntries">
          <button
            type="button"
            class="designPaletteItem"
            onClick={(event) => {
              event.stopPropagation();
              insertText(target);
            }}
          >
            <UiIcon name="canvas-text" />
            <span>{t("composition.studioText")}</span>
          </button>
          <button
            type="button"
            class="designPaletteItem"
            onClick={(event) => {
              event.stopPropagation();
              props.onPaletteTarget(null);
              props.onRequestDisplayPicker(target);
            }}
          >
            <UiIcon name="canvas-metric" />
            <span>{t("composition.studioMetric")}</span>
          </button>
          <button
            type="button"
            class="designPaletteItem"
            onClick={(event) => {
              event.stopPropagation();
              props.onPaletteTarget(null);
              props.onRequestDisplayPicker(target);
            }}
          >
            <UiIcon name="canvas-table" />
            <span>{t("composition.studioTable")}</span>
          </button>
        </div>
        <div class="designPaletteParams">
          <span class="ui-label">{t("composition.studioParameters")}</span>
          <Show
            when={unplaced().length > 0}
            fallback={
              <p class="ui-muted">{t("composition.studioEmptyParameters")}</p>
            }
          >
            <For each={unplaced()}>
              {(parameter) => (
                <button
                  type="button"
                  class="designPaletteItem"
                  aria-label={t("composition.studioPlaceParameter", {
                    name: parameter.label ?? parameter.id,
                  })}
                  onClick={(event) => {
                    event.stopPropagation();
                    insertParameter(target, parameter.id);
                  }}
                >
                  <UiIcon name="canvas-input" />
                  <span>{parameter.label ?? parameter.id}</span>
                </button>
              )}
            </For>
          </Show>
        </div>
      </div>
    );
  };

  const renderGap = (target: DraftInsertTarget, inline: boolean) => (
    <div class={inline ? "designGap designGap--inline" : "designGap"}>
      <button
        type="button"
        class="designAdd"
        aria-label={t("composition.studioAddBlock")}
        title={t("composition.studioAddBlock")}
        aria-expanded={paletteKey() === gapKey(target)}
        onClick={(event) => {
          event.stopPropagation();
          togglePalette(target);
        }}
      >
        <UiIcon name="plus" />
      </button>
      <Show when={paletteKey() === gapKey(target)}>
        {renderPalette(target)}
      </Show>
    </div>
  );

  const renderBlock = (
    row: DraftLayoutRow,
    item: DraftLayoutItem,
    itemIndex: number,
    rowItemCount: number,
  ) => {
    const blockId = () =>
      item.kind === "parameter"
        ? designBlockIdForParameter(item.parameterId)
        : designBlockIdForComponent(item.draftId);
    const selected = () => props.selectedId === blockId();
    const name = () => blockName(props.draft, definitionById(), item);
    const binding = () =>
      item.kind === "component" ? bindingById().get(item.draftId) : undefined;
    const ownsSourceStatus = () => {
      const current = binding();
      return current !== undefined && current.kind !== "text" &&
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
              <IconButton
                icon="trash"
                label={t("composition.studioRemoveDisplay", { name: name() })}
                onClick={(event) => {
                  event.stopPropagation();
                  removeBlock(item);
                }}
              />
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
            definition={item.kind === "parameter"
              ? definitionById().get(item.parameterId)
              : undefined}
            parameterValues={props.parameterValues}
            onParameterChange={props.onParameterChange}
            sources={props.sources}
            sourceById={sourceById()}
            ownsSourceStatus={ownsSourceStatus()}
            fieldNames={props.fieldNames}
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
                    {renderGap(
                      {
                        rowId: row.id,
                        rowIndex: rowIndex(),
                        itemIndex: itemIndex(),
                      },
                      true,
                    )}
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
    </div>
  );
}
