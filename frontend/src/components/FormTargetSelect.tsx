import { createEffect, createMemo, createSignal, For, Show } from "solid-js";
import { t } from "~/lib/i18n";

export interface FormTargetSelectProps {
  /** Accessible name for the combobox input. */
  label: string;
  /** Stored stable Form identifier (Form name). */
  value: string;
  /** Available Form names shown as candidates. */
  options: readonly string[];
  placeholder?: string;
  inputClass?: string;
  onChange: (value: string) => void;
}

let formTargetSelectCounter = 0;

/**
 * Single searchable select for a row_reference target Form.
 *
 * One combobox box: typing filters candidates, keyboard navigation works,
 * the selected human-readable name is displayed while the stored value
 * remains the stable Form identifier. Free-text entry is impossible:
 * blurring or pressing Escape reverts to the last committed value, so an
 * invalid stored value can only come from a deleted target and is then
 * shown as an informational unknown option alongside the field error.
 */
export function FormTargetSelect(props: FormTargetSelectProps) {
  const listId = `form-target-list-${formTargetSelectCounter += 1}`;
  const [open, setOpen] = createSignal(false);
  const [query, setQuery] = createSignal(props.value);
  const [activeIndex, setActiveIndex] = createSignal(0);

  // Follow external resets (dialog open/close) while the popup is closed.
  createEffect(() => {
    if (!open()) setQuery(props.value);
  });

  // Keep the keyboard cursor inside the visible matches when the option
  // list shrinks while the popup is open.
  createEffect(() => {
    const count = filtered().length;
    if (count === 0) {
      setActiveIndex(0);
      return;
    }
    setActiveIndex((index) => Math.min(index, count - 1));
  });

  const sortedOptions = createMemo(() => [...props.options].sort());
  const filtered = createMemo(() => {
    const needle = query().trim().toLowerCase();
    if (!needle) return sortedOptions();
    return sortedOptions().filter((option) =>
      option.toLowerCase().includes(needle)
    );
  });
  const unknownValue = createMemo(() => {
    const current = props.value.trim();
    return current && !props.options.includes(current) ? current : "";
  });

  const commit = (value: string) => {
    props.onChange(value);
    setQuery(value);
    setOpen(false);
    setActiveIndex(0);
  };

  const revert = () => {
    setQuery(props.value);
    setOpen(false);
    setActiveIndex(0);
  };

  const openPopup = () => {
    setOpen(true);
    setActiveIndex(0);
  };

  const handleKeyDown = (event: KeyboardEvent) => {
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      if (!open()) {
        openPopup();
        return;
      }
      const count = filtered().length;
      if (count === 0) return;
      const delta = event.key === "ArrowDown" ? 1 : -1;
      setActiveIndex((index) => (index + delta + count) % count);
      return;
    }
    if (event.key === "Enter") {
      if (!open()) return;
      event.preventDefault();
      const match = filtered()[activeIndex()];
      if (match !== undefined) commit(match);
      return;
    }
    if (event.key === "Escape") {
      if (!open()) return;
      event.preventDefault();
      revert();
    }
  };

  return (
    <div
      class="formTargetSelect"
      onFocusOut={(event) => {
        if (
          event.currentTarget.contains(event.relatedTarget as Node | null)
        ) {
          return;
        }
        if (open()) revert();
      }}
    >
      <input
        type="text"
        role="combobox"
        aria-label={props.label}
        aria-expanded={open()}
        aria-controls={listId}
        aria-autocomplete="list"
        aria-activedescendant={open() && filtered().length > 0
          ? `${listId}-option-${activeIndex()}`
          : undefined}
        aria-invalid={unknownValue() ? "true" : undefined}
        placeholder={props.placeholder}
        autocomplete="off"
        class={props.inputClass}
        value={query()}
        onInput={(event) => {
          setQuery(event.currentTarget.value);
          openPopup();
        }}
        onFocus={() => openPopup()}
        onKeyDown={handleKeyDown}
      />
      <Show when={open()}>
        <ul
          id={listId}
          role="listbox"
          aria-label={props.label}
          class="formTargetSelectPopup"
          onMouseDown={(event) => event.preventDefault()}
        >
          <Show when={unknownValue()}>
            {(unknown) => (
              <li
                role="option"
                aria-selected="false"
                aria-disabled="true"
                class="formTargetSelectOption formTargetSelectUnknown"
              >
                {t("createDialog.form.targetFormUnknown", {
                  name: unknown(),
                })}
              </li>
            )}
          </Show>
          <For each={filtered()}>
            {(option, index) => (
              <li
                id={`${listId}-option-${index()}`}
                role="option"
                aria-selected={option === props.value}
                class="formTargetSelectOption"
                classList={{
                  formTargetSelectActive: index() === activeIndex(),
                }}
                onClick={() => commit(option)}
                onMouseMove={() => setActiveIndex(index())}
              >
                {option}
              </li>
            )}
          </For>
          <Show when={props.options.length === 0}>
            <li
              role="option"
              aria-selected="false"
              aria-disabled="true"
              class="formTargetSelectOption formTargetSelectEmpty"
            >
              {t("createDialog.form.targetFormNoForms")}
            </li>
          </Show>
          <Show
            when={props.options.length > 0 && filtered().length === 0}
          >
            <li
              role="option"
              aria-selected="false"
              aria-disabled="true"
              class="formTargetSelectOption formTargetSelectEmpty"
            >
              {t("createDialog.form.targetFormNoMatches")}
            </li>
          </Show>
        </ul>
      </Show>
    </div>
  );
}
