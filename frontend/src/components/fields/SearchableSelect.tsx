import { createMemo, createSignal, For, Show } from "solid-js";
import { LocalBusyIndicator } from "~/components/LocalBusyIndicator";

export interface SearchableOption {
  id: string;
  title: string;
}

export interface SearchableSelectLabels {
  placeholder: string;
  help: string;
  selectedHeading: string;
  clear: string;
  loading: string;
  loadError: string;
  noMatches: string;
}

export interface SearchableSelectProps {
  id: string;
  query: string;
  onQueryInput: (value: string) => void;
  options: SearchableOption[];
  selected: SearchableOption | null;
  onSelect: (option: SearchableOption) => void;
  onClear: () => void;
  loading: boolean;
  error: unknown;
  invalid?: boolean;
  describedBy?: string;
  labels: SearchableSelectLabels;
  /** Override the Escape behavior (default: reset the search text). */
  onEscape?: () => void;
}

/**
 * Generic searchable single-select shared by every row-reference control.
 *
 * Display/interaction only: options render human-readable titles, selection
 * stores the stable option id through `onSelect`, and validity stays in
 * Rust. Keyboard: ArrowDown/ArrowUp move the highlight (wrapping),
 * Enter confirms the highlighted (or first) option, Escape dismisses the
 * current search text.
 */
export function SearchableSelect(props: SearchableSelectProps) {
  const [activeIndex, setActiveIndex] = createSignal(0);
  const [listId] = createSignal(
    `searchable-${Math.random().toString(36).slice(2)}`,
  );

  const visibleOptions = createMemo(() => props.options);
  const showList = createMemo(() =>
    !props.loading && !props.error && visibleOptions().length > 0
  );

  const clampActive = (count: number, next: number) => {
    if (count === 0) return 0;
    return ((next % count) + count) % count;
  };

  const handleKeyDown = (event: KeyboardEvent) => {
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      const count = visibleOptions().length;
      if (count === 0) return;
      event.preventDefault();
      setActiveIndex((index) =>
        clampActive(
          count,
          event.key === "ArrowDown" ? index + 1 : index - 1,
        )
      );
      return;
    }
    if (event.key === "Enter") {
      const options = visibleOptions();
      if (options.length === 0 || props.loading || props.error) return;
      const option = options[clampActive(options.length, activeIndex())];
      if (!option) return;
      event.preventDefault();
      props.onSelect(option);
      return;
    }
    if (event.key === "Escape") {
      event.preventDefault();
      if (props.onEscape) {
        props.onEscape();
        return;
      }
      props.onQueryInput(props.selected ? props.selected.title : "");
      setActiveIndex(0);
    }
  };

  return (
    <div class="ui-stack-sm">
      <input
        id={props.id}
        type="search"
        role="combobox"
        class="ui-input"
        value={props.query}
        aria-expanded={showList() || undefined}
        aria-controls={listId()}
        aria-activedescendant={showList()
          ? `${listId()}-${clampActive(visibleOptions().length, activeIndex())}`
          : undefined}
        aria-invalid={props.invalid ? "true" : undefined}
        aria-describedby={props.describedBy}
        placeholder={props.labels.placeholder}
        onInput={(event) => {
          setActiveIndex(0);
          props.onQueryInput(event.currentTarget.value);
        }}
        onKeyDown={handleKeyDown}
        autocomplete="off"
      />
      <p class="text-xs ui-muted">{props.labels.help}</p>
      <Show when={props.selected}>
        {(selectedOption) => (
          <div class="ui-reference-picker-selection">
            <p class="text-[11px] font-semibold uppercase tracking-wide ui-muted">
              {props.labels.selectedHeading}
            </p>
            <div class="flex flex-wrap items-center justify-between gap-3">
              <div class="min-w-0">
                <p class="truncate text-sm font-medium">
                  {selectedOption().title}
                </p>
                <p class="truncate text-xs ui-muted">{selectedOption().id}</p>
              </div>
              <button
                type="button"
                class="ui-button ui-button-secondary ui-button-sm text-xs"
                onClick={() => {
                  setActiveIndex(0);
                  props.onClear();
                }}
              >
                {props.labels.clear}
              </button>
            </div>
          </div>
        )}
      </Show>
      <Show when={props.loading}>
        <LocalBusyIndicator label={props.labels.loading} />
      </Show>
      <Show when={!props.loading && props.error}>
        <p class="text-xs ui-text-danger">{props.labels.loadError}</p>
      </Show>
      <Show when={showList()}>
        <ul
          id={listId()}
          role="listbox"
          class="ui-reference-picker-list"
          aria-label={props.labels.placeholder}
        >
          <For each={visibleOptions()}>
            {(option, index) => (
              <li
                id={`${listId()}-${index()}`}
                role="option"
                aria-selected={clampActive(
                  visibleOptions().length,
                  activeIndex(),
                ) ===
                  index()}
                class="ui-reference-picker-option"
              >
                <button
                  type="button"
                  class="ui-reference-picker-button"
                  data-active={clampActive(
                        visibleOptions().length,
                        activeIndex(),
                      ) ===
                      index() || undefined}
                  onClick={() => {
                    setActiveIndex(index());
                    props.onSelect(option);
                  }}
                  onMouseMove={() => setActiveIndex(index())}
                >
                  <p class="text-sm font-medium">{option.title}</p>
                  <p class="text-xs ui-muted">{option.id}</p>
                </button>
              </li>
            )}
          </For>
        </ul>
      </Show>
      <Show
        when={!props.loading && !props.error && props.query.trim() &&
          visibleOptions().length === 0}
      >
        <p class="text-xs ui-muted">{props.labels.noMatches}</p>
      </Show>
    </div>
  );
}
