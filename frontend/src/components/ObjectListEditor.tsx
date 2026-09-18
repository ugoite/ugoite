import { createSignal, For, Show, type Accessor } from "solid-js";
import { t } from "~/lib/i18n";
import { ListEditor } from "~/components/ListEditor";

export interface ObjectListEditorProps {
  fieldName: string;
  values: readonly Record<string, unknown>[];
  onChange: (values: Record<string, unknown>[]) => void;
  invalid: boolean;
  describedBy: string | undefined;
  inputClass?: string;
}

const withKey = (
  item: Record<string, unknown>,
  key: string,
  value: unknown,
): Record<string, unknown> => ({ ...item, [key]: value });

const withoutKey = (
  item: Record<string, unknown>,
  key: string,
): Record<string, unknown> => {
  const next = { ...item };
  delete next[key];
  return next;
};

function displayForEdit(value: unknown): string {
  if (typeof value === "string") return value;
  if (typeof value === "number" || typeof value === "boolean") {
    return String(value);
  }
  try {
    return JSON.stringify(value);
  } catch {
    return "";
  }
}

function commitTextEdit(raw: string, previous: unknown): unknown {
  // Numbers and JSON shapes round-trip as their kind; anything else stays
  // a string and the shared Rust boundary decides validity.
  if (typeof previous === "number") {
    if (raw.trim() === "") return raw;
    const parsed = Number(raw);
    return Number.isFinite(parsed) ? parsed : raw;
  }
  if (raw.trim() === "") return raw;
  if (previous !== null && typeof previous === "object") {
    try {
      return JSON.parse(raw);
    } catch {
      return raw;
    }
  }
  return raw;
}

function AddPropertyRow(props: { onAdd: (key: string) => void }) {
  const [key, setKey] = createSignal("");
  const trimmed = () => key().trim();
  return (
    <div class="flex items-center gap-2">
      <input
        class="ui-input ui-input-sm"
        value={key()}
        aria-label={t("entryDetail.objectNewKeyPlaceholder")}
        placeholder={t("entryDetail.objectNewKeyPlaceholder")}
        onInput={(event) => setKey(event.currentTarget.value)}
        onKeyDown={(event) => {
          if (event.key === "Enter") {
            event.preventDefault();
            if (!trimmed()) return;
            props.onAdd(trimmed());
            setKey("");
          }
        }}
      />
      <button
        type="button"
        class="ui-button ui-button-secondary ui-button-sm text-sm"
        disabled={!trimmed()}
        onClick={() => {
          if (!trimmed()) return;
          props.onAdd(trimmed());
          setKey("");
        }}
      >
        {t("entryDetail.objectAddProperty")}
      </button>
    </div>
  );
}

/**
 * Variable-length collection of repeated typed object editors.
 *
 * Each item renders one group with its properties; every nested property
 * uses a typed control for its runtime kind (text, number-aware text,
 * checkbox, JSON fallback) instead of raw JSON. Reads stay in leaf
 * bindings so rows (and focus) survive keystrokes; only add/remove
 * re-structures rows. Adding or removing items or properties marks the
 * entry dirty through `onChange`.
 */
export function ObjectListEditor(props: ObjectListEditorProps) {
  return (
    <ListEditor
      values={props.values}
      onChange={props.onChange}
      createItem={() => ({})}
      addLabel={t("entryDetail.listAddItem")}
      emptyText={t("entryDetail.objectEmpty")}
      class="ui-stack"
      renderItem={(item, index, helpers) => {
        // Property keys drive the inner structure. The outer row reads the
        // item to derive its key set, so same-key edits only refresh leaf
        // bindings while add/remove re-structure inner rows; Solid's keyed
        // For reuses DOM for unchanged keys, keeping focus stable.
        // AddPropertyRow is a component boundary, so its draft survives.
        const keys = (): string[] => Object.keys(item() ?? {});
        const itemLabel = () =>
          t("entryDetail.listItemLabel", {
            field: props.fieldName,
            index: index() + 1,
          });
        const setItem = (next: Record<string, unknown>) => {
          props.onChange(
            props.values.map((entry, position) =>
              position === index() ? next : entry
            ),
          );
        };
        const readItem = (): Record<string, unknown> => item() ?? {};
        return (
          <div
            class="ui-card ui-stack-sm"
            role="group"
            aria-label={itemLabel()}
          >
            <div class="flex flex-wrap items-center justify-between gap-2">
              <span class="text-sm font-medium">{itemLabel()}</span>
              <button
                type="button"
                class="ui-button ui-button-secondary ui-button-sm text-sm"
                aria-label={t("entryDetail.listRemoveItem", {
                  field: props.fieldName,
                  index: index() + 1,
                })}
                onClick={helpers.remove}
              >
                {t("common.remove")}
              </button>
            </div>
            <For each={keys()}>
              {(key) => {
                const current = (): unknown => readItem()[key];
                return (
                  <div class="flex items-center gap-2">
                    <span class="text-xs ui-muted min-w-0 flex-1 truncate">
                      {key}
                    </span>
                    <Show
                      when={typeof current() === "boolean"}
                      fallback={
                        <input
                          class={props.inputClass ?? "ui-input ui-input-sm"}
                          value={displayForEdit(current())}
                          aria-label={key}
                          aria-invalid={props.invalid ? "true" : undefined}
                          aria-describedby={props.invalid
                            ? props.describedBy
                            : undefined}
                          inputmode={typeof current() === "number"
                            ? "decimal"
                            : undefined}
                          onInput={(event) =>
                            setItem(
                              withKey(
                                readItem(),
                                key,
                                commitTextEdit(
                                  event.currentTarget.value,
                                  current(),
                                ),
                              ),
                            )}
                        />
                      }
                    >
                      <input
                        type="checkbox"
                        checked={current() === true}
                        aria-label={key}
                        aria-invalid={props.invalid ? "true" : undefined}
                        aria-describedby={props.invalid
                          ? props.describedBy
                          : undefined}
                        onChange={(event) =>
                          setItem(
                            withKey(
                              readItem(),
                              key,
                              event.currentTarget.checked,
                            ),
                          )}
                      />
                    </Show>
                    <button
                      type="button"
                      class="ui-button ui-button-secondary ui-button-sm text-sm"
                      aria-label={t("entryDetail.objectRemoveProperty", {
                        key,
                      })}
                      onClick={() => setItem(withoutKey(readItem(), key))}
                    >
                      {t("common.remove")}
                    </button>
                  </div>
                );
              }}
            </For>
            <AddPropertyRow
              onAdd={(key) => {
                if (key in readItem()) return;
                setItem(withKey(readItem(), key, ""));
              }}
            />
          </div>
        );
      }}
    />
  );
}
