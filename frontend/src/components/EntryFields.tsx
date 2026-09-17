import { For, type JSX, Show } from "solid-js";
import { Dynamic } from "solid-js/web";
import { t } from "~/lib/i18n";

export interface EntryFieldDescriptor {
  name: string;
  type?: string;
  targetForm?: string;
  required?: boolean;
  fieldId: string;
}

export interface EntryFieldsControlHelpers {
  invalid: boolean;
  describedBy: string | undefined;
}

export interface EntryFieldsProps {
  titleValue: string;
  titleId?: string;
  onTitleChange?: (value: string) => void;
  /**
   * Field structure (order-stable per form). Values are NOT snapshots here:
   * read them through `getValue`/`renderControl` so row structure never
   * depends on keystrokes and focused inputs stay mounted.
   */
  fields: EntryFieldDescriptor[];
  /** Live value read for the default controls (reactive binding). */
  getValue?: (name: string) => string;
  onFieldChange?: (name: string, value: string) => void;
  /** Read-only mode: every control is disabled and the wrapper gets `.readonly`. */
  readOnly?: boolean;
  isInvalid?: (name: string) => boolean;
  describedBy?: (fieldId: string) => string;
  /**
   * Override control rendering (asset pickers, row references, ...).
   * `helpers` is a thunk: only call it inside the control's own reactive
   * bindings, never eagerly here — otherwise the field-expression effect
   * would re-create the control on every keystroke and detach focused
   * inputs.
   */
  renderControl?: (
    field: EntryFieldDescriptor,
    helpers: () => EntryFieldsControlHelpers,
  ) => JSX.Element;
  /**
   * Show the raw field-type line under each field name. Normal editing hides
   * it (PR4: the control itself expresses the type); advanced and detail
   * surfaces (e.g. revision review) opt in explicitly.
   */
  showFieldTypes?: boolean;
  /**
   * Extra content below a field (hints, validation messages). Rendered
   * through a `Dynamic` boundary so its inline `Show`s keep granular
   * subscriptions instead of re-creating row content.
   */
  belowField?: (props: { name: string }) => JSX.Element;
}

export function createEntryFieldInputId(fieldName: string, index: number) {
  const normalized = fieldName
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9_-]+/g, "-")
    .replace(/^-+|-+$/g, "");
  return `entry-field-${index}-${normalized || "field"}`;
}

function defaultControl(
  field: EntryFieldDescriptor,
  helpers: () => EntryFieldsControlHelpers,
  props: EntryFieldsProps,
): JSX.Element {
  // Helpers stay lazy: these bindings own their subscriptions, so
  // validation state flows without re-creating the control.
  const invalid = () => helpers().invalid;
  const describedBy = () => helpers().describedBy;
  const handleInput = (value: string) =>
    props.onFieldChange?.(field.name, value);
  // Control type is mount-time structure (mirrors the editor's type-first
  // branching): re-picking it per keystroke would steal focus.
  const initialValue = props.getValue?.(field.name) ?? "";
  const multiline = field.type === "markdown" ||
    field.type === "list" ||
    field.type === "object_list" ||
    (field.type === undefined && initialValue.includes("\n"));
  if (multiline) {
    return (
      <textarea
        id={field.fieldId}
        class="ui-input ui-textarea"
        value={props.getValue?.(field.name) ?? initialValue}
        disabled={props.readOnly}
        aria-invalid={invalid() ? "true" : undefined}
        aria-describedby={invalid() ? describedBy() : undefined}
        placeholder={t("entryDetail.fieldPlaceholder")}
        onInput={(event) => handleInput(event.currentTarget.value)}
      />
    );
  }
  return (
    <input
      id={field.fieldId}
      class="ui-input"
      type={field.type === "date" ? "date" : "text"}
      value={props.getValue?.(field.name) ?? initialValue}
      disabled={props.readOnly}
      aria-invalid={invalid() ? "true" : undefined}
      aria-describedby={invalid() ? describedBy() : undefined}
      placeholder={t("entryDetail.fieldPlaceholder")}
      onInput={(event) => handleInput(event.currentTarget.value)}
    />
  );
}

/**
 * Shared entry field renderer. Title, body, and other fields share one
 * spacing contract (`.form` / `.field`); read-only mode disables every
 * control and marks the wrapper `.readonly`. Used by the entry editor and
 * the revision review route.
 *
 * Rows are keyed by descriptor identity (`For`): pass a stable-identity
 * array (memoized per form) and keep values in live bindings (`getValue`,
 * `renderControl`, `belowField`). Rebuilding descriptor objects on every
 * keystroke would detach focused inputs; freezing row content instead (e.g.
 * position-keyed `Index` with snapshots) would miss structure changes such
 * as new-entry form switches.
 */
export function EntryFields(props: EntryFieldsProps) {
  const titleId = () => props.titleId ?? "entry-title-editor";
  const readOnly = () => props.readOnly ?? false;

  return (
    <div
      class="form entry-fields"
      classList={{ readonly: readOnly() }}
    >
      <div class="field ui-entry-field ui-entry-title-field">
        <div class="ui-entry-field-heading">
          <label class="ui-label" for={titleId()}>
            {t("common.title")}
          </label>
          <Show when={!readOnly()}>
            <span class="ui-pill">{t("entryDetail.optional")}</span>
          </Show>
        </div>
        <input
          id={titleId()}
          class="ui-input ui-entry-title-input"
          value={props.titleValue}
          disabled={readOnly()}
          placeholder={t("common.untitled")}
          onInput={(event) => props.onTitleChange?.(event.currentTarget.value)}
        />
      </div>

      <div class="ui-entry-field-list">
        <For each={props.fields}>
          {(field) => {
            const getHelpers = (): EntryFieldsControlHelpers => ({
              invalid: props.isInvalid?.(field.name) ?? false,
              describedBy: (props.isInvalid?.(field.name) ?? false)
                ? props.describedBy?.(field.fieldId)
                : undefined,
            });
            const isInvalid = () => getHelpers().invalid;
            return (
              <div
                class="field ui-entry-field"
                classList={{ "ui-entry-field-error": isInvalid() }}
              >
                <div class="ui-entry-field-heading">
                  <div class="min-w-0">
                    <label class="ui-label" for={field.fieldId}>
                      {field.name}
                    </label>
                    <Show when={props.showFieldTypes && field.type}>
                      <p class="mt-0.5 text-xs ui-muted">
                        {field.type}
                        <Show when={field.targetForm}>
                          {` · ${field.targetForm}`}
                        </Show>
                      </p>
                    </Show>
                  </div>
                  <span
                    class={field.required ? "ui-entry-required" : "ui-pill"}
                  >
                    {field.required
                      ? t("entryDetail.required")
                      : t("entryDetail.optional")}
                  </span>
                </div>
                {props.renderControl
                  ? props.renderControl(field, getHelpers)
                  : defaultControl(field, getHelpers, props)}
                {props.belowField
                  ? (
                    <Dynamic
                      component={props.belowField}
                      name={field.name}
                    />
                  )
                  : null}
              </div>
            );
          }}
        </For>
      </div>
    </div>
  );
}
