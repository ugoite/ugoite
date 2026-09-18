import {
  createEffect,
  createSignal,
  For,
  type Accessor,
  type JSX,
  Show,
} from "solid-js";

export interface ListItemHelpers {
  /** Remove the item at its current position. */
  remove: () => void;
}

export interface ListEditorProps<T> {
  /** Current items in display order. Read lazily: rows subscribe narrowly. */
  values: readonly T[];
  onChange: (values: T[]) => void;
  /** Build a new default item appended by the add action. */
  createItem: () => T;
  /**
   * Render one row. `value` is an accessor: read it only inside leaf
   * bindings (props, text) and event handlers, never eagerly in the row
   * body. Eager reads would subscribe the row to every keystroke and
   * re-create focused inputs. `index` tracks the live position.
   */
  renderItem: (
    value: Accessor<T>,
    index: Accessor<number>,
    helpers: ListItemHelpers,
  ) => JSX.Element;
  addLabel: string;
  emptyText?: string;
  class?: string;
}

let listEditorKeyCounter = 0;

/**
 * Variable-length collection of repeated typed editors.
 *
 * Rows carry stable identities that survive value edits: typing updates
 * only leaf bindings, so focused inputs stay mounted and keyboard order
 * always matches DOM order. Rows mount on add and dispose on remove only.
 * Reorder controls are intentionally absent: ordering is not semantically
 * meaningful for the current list kinds.
 */
export function ListEditor<T>(props: ListEditorProps<T>) {
  // Stable per-position identities. Length-driven only: value edits keep
  // every identity, so rows (and focus) survive keystrokes.
  const [rowIds, setRowIds] = createSignal<string[]>([]);
  createEffect(() => {
    const count = props.values.length;
    setRowIds((ids) => {
      if (ids.length === count) return ids;
      const next = ids.slice(0, count);
      while (next.length < count) {
        next.push(`list-item-${listEditorKeyCounter += 1}`);
      }
      return next;
    });
  });

  const removeAt = (position: number) => {
    props.onChange(props.values.filter((_, index) => index !== position));
  };

  return (
    <div class={props.class ?? "ui-stack-sm"}>
      <For each={rowIds()}>
        {(id, index) => (
          <>
            {props.renderItem(
              () => props.values[index()],
              index,
              { remove: () => removeAt(index()) },
            )}
          </>
        )}
      </For>
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
