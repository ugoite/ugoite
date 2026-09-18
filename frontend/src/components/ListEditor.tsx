import { Index, type JSX, Show } from "solid-js";

export interface ListItemHelpers {
  /** Remove the item at this position. */
  remove: () => void;
}

export interface ListEditorProps<T> {
  /** Current items in display order. */
  values: readonly T[];
  onChange: (values: T[]) => void;
  /** Build a new default item appended by the add action. */
  createItem: () => T;
  renderItem: (
    value: T,
    index: number,
    helpers: ListItemHelpers,
  ) => JSX.Element;
  addLabel: string;
  emptyText?: string;
  class?: string;
}

/**
 * Variable-length collection of repeated typed editors.
 *
 * Items render through `Index` (positional, nodes never recreated) so
 * focused inputs stay mounted while typing; adding or removing an item
 * reports through `onChange`. Keyboard and visual order always match DOM
 * order. Reorder controls are intentionally absent: ordering is not
 * semantically meaningful for the current list kinds.
 */
export function ListEditor<T>(props: ListEditorProps<T>) {
  const removeAt = (index: number) => {
    props.onChange(props.values.filter((_, position) => position !== index));
  };

  return (
    <div class={props.class ?? "ui-stack-sm"}>
      <Index each={props.values}>
        {(value, index) => (
          <>
            {props.renderItem(value(), index, {
              remove: () => removeAt(index),
            })}
          </>
        )}
      </Index>
      <Show when={props.values.length === 0 && props.emptyText}>
        {(text) => <p class="text-sm ui-muted">{text()}</p>}
      </Show>
      <div>
        <button
          type="button"
          class="ui-button ui-button-secondary ui-button-sm text-sm"
          onClick={() => props.onChange([...props.values, props.createItem()])}
        >
          {props.addLabel}
        </button>
      </div>
    </div>
  );
}
