import { Show } from "solid-js";
import { ListEditor } from "~/components/ListEditor";
import { ObjectListEditor } from "~/components/ObjectListEditor";
import { RowReferenceSelect } from "~/components/fields/RowReferenceSelect";
import {
  hasRowReferencePicker,
  normalizeRowReferenceTargetForm,
} from "~/components/fields/row-reference";
import {
  draftValueToDisplayString,
  normalizeBooleanListValue,
  normalizeNumberListValue,
  normalizeObjectListValue,
  normalizeStringListValue,
  parseBooleanAlias,
  parseNumberItemText,
} from "~/lib/draft-values";
import { t } from "~/lib/i18n";
import type { Form } from "~/lib/types";

export interface FieldDefinitionLike {
  type: string;
  required?: boolean;
  deprecated?: boolean;
  target_form?: string;
  items?: { type: string; target_form?: string };
}

export interface FieldInputProps {
  field: FieldDefinitionLike;
  /** Typed draft value. Strings stay strings; Rust owns coercion. */
  value: unknown;
  onChange: (value: unknown) => void;
  fieldId: string;
  /**
   * Human field name for accessible row labels ("Checklist item 1").
   * Defaults to `fieldId` when the call site has no display name.
   */
  fieldName?: string;
  /** Space scope for the canonical EntryQuery row-reference picker. */
  spaceId?: string;
  /** Form catalog used to resolve Row Reference targets and capabilities. */
  forms?: readonly Form[];
  invalid?: boolean;
  describedBy?: string;
  /** Long-text rendering for string-like fields (markdown document fields). */
  multiline?: boolean;
  placeholder?: string;
  /** Kept for form-level validation hooks around row-reference selection. */
  onRowReferencePendingChange?: (pending: boolean) => void;
}

const NUMERIC_FIELD_TYPES = new Set([
  "integer",
  "long",
  "double",
  "float",
]);

const isListRowReferenceField = (field: FieldDefinitionLike) =>
  field.type === "list" &&
  field.items?.type === "row_reference" &&
  (field.items.target_form?.trim() ?? "") !== "";

/** Display a typed value in a scalar text control without deciding semantics. */
export const fieldValueToText = (value: unknown): string =>
  typeof value === "string" ? value : draftValueToDisplayString(value);

/**
 * Resolve a boolean-looking value to a checkbox state. Returns null for
 * legacy text Rust would reject (e.g. "maybe") so callers keep the raw
 * text control instead of destroying the stored value.
 */
export const resolveBooleanCheckbox = (value: unknown): boolean | null => {
  if (typeof value === "boolean") return value;
  if (typeof value === "number") {
    if (value === 1) return true;
    if (value === 0) return false;
    return null;
  }
  if (value === null || value === undefined) return false;
  if (typeof value === "string") {
    if (value.trim() === "") return false;
    const alias = parseBooleanAlias(value);
    return alias ?? null;
  }
  return null;
};

function ScalarTextInput(props: {
  fieldId: string;
  type: string;
  inputMode?: "decimal";
  value: string;
  invalid?: boolean;
  describedBy?: string;
  placeholder?: string;
  multiline: boolean;
  onChange: (value: string) => void;
}) {
  return (
    <Show
      when={props.multiline}
      fallback={
        <input
          id={props.fieldId}
          class="ui-input"
          type={props.type}
          inputmode={props.inputMode}
          value={props.value}
          aria-invalid={props.invalid ? "true" : undefined}
          aria-describedby={props.invalid ? props.describedBy : undefined}
          placeholder={props.placeholder ?? t("entryDetail.fieldPlaceholder")}
          onInput={(event) => props.onChange(event.currentTarget.value)}
        />
      }
    >
      <textarea
        id={props.fieldId}
        class="ui-input ui-textarea"
        value={props.value}
        aria-invalid={props.invalid ? "true" : undefined}
        aria-describedby={props.invalid ? props.describedBy : undefined}
        placeholder={props.placeholder ?? t("entryDetail.fieldPlaceholder")}
        onInput={(event) => props.onChange(event.currentTarget.value)}
      />
    </Show>
  );
}

function StringListRows(props: FieldInputProps & { fieldName: string }) {
  const readItems = () => normalizeStringListValue(props.value as never);
  return (
    <ListEditor
      values={readItems()}
      onChange={props.onChange}
      createItem={() => ""}
      addLabel={t("entryDetail.listAddItem")}
      renderItem={(item, index, helpers) => (
        <div class="flex items-center gap-2">
          <input
            id={index() === 0 ? props.fieldId : `${props.fieldId}-${index()}`}
            class="ui-input"
            value={item()}
            aria-label={t("entryDetail.listItemLabel", {
              field: props.fieldName,
              index: index() + 1,
            })}
            aria-invalid={props.invalid ? "true" : undefined}
            aria-describedby={props.invalid ? props.describedBy : undefined}
            placeholder={t("entryDetail.fieldPlaceholder")}
            onInput={(event) =>
              props.onChange(
                readItems().map((entry, position) =>
                  position === index() ? event.currentTarget.value : entry
                ),
              )}
          />
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
      )}
    />
  );
}

function NumberListRows(props: FieldInputProps & { fieldName: string }) {
  const readItems = () => normalizeNumberListValue(props.value as never);
  return (
    <ListEditor
      values={readItems()}
      onChange={props.onChange}
      createItem={() => ""}
      addLabel={t("entryDetail.listAddItem")}
      renderItem={(item, index, helpers) => (
        <div class="flex items-center gap-2">
          <input
            id={index() === 0 ? props.fieldId : `${props.fieldId}-${index()}`}
            class="ui-input"
            value={String(item())}
            aria-label={t("entryDetail.listItemLabel", {
              field: props.fieldName,
              index: index() + 1,
            })}
            aria-invalid={props.invalid ? "true" : undefined}
            aria-describedby={props.invalid ? props.describedBy : undefined}
            placeholder={t("entryDetail.fieldPlaceholder")}
            inputmode="decimal"
            onInput={(event) =>
              props.onChange(
                readItems().map((entry, position) =>
                  position === index()
                    ? parseNumberItemText(event.currentTarget.value)
                    : entry
                ),
              )}
          />
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
      )}
    />
  );
}

function BooleanListRows(props: FieldInputProps & { fieldName: string }) {
  const readItems = () => normalizeBooleanListValue(props.value as never);
  return (
    <ListEditor
      values={readItems()}
      onChange={props.onChange}
      createItem={() => false as never}
      addLabel={t("entryDetail.listAddItem")}
      renderItem={(item, index, helpers) => (
        <div class="flex items-center gap-2">
          <input
            id={index() === 0 ? props.fieldId : `${props.fieldId}-${index()}`}
            type="checkbox"
            checked={typeof item() === "boolean"
              ? (item() as boolean)
              : parseBooleanAlias(
                typeof item() === "string" ? (item() as string) : "",
              ) === true}
            aria-label={t("entryDetail.listItemLabel", {
              field: props.fieldName,
              index: index() + 1,
            })}
            aria-invalid={props.invalid ? "true" : undefined}
            aria-describedby={props.invalid ? props.describedBy : undefined}
            onChange={(event) =>
              props.onChange(
                readItems().map((entry, position) =>
                  position === index() ? event.currentTarget.checked : entry
                ),
              )}
          />
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
      )}
    />
  );
}

function RowReferenceListRows(
  props: FieldInputProps & { fieldName: string; targetForm: string },
) {
  const readItems = () =>
    Array.isArray(props.value)
      ? (props.value as unknown[]).filter((item): item is string =>
        typeof item === "string"
      )
      : typeof props.value === "string" && props.value.trim() !== ""
      ? [props.value as string]
      : [];
  return (
    <ListEditor
      values={readItems()}
      onChange={props.onChange}
      createItem={() => ""}
      addLabel={t("entryDetail.listAddItem")}
      renderItem={(item, index, helpers) => (
        <div class="ui-stack-sm">
          <div class="flex items-center justify-end">
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
          <RowReferenceSelect
            spaceId={props.spaceId ?? ""}
            targetForm={props.targetForm}
            forms={props.forms}
            value={item() ?? ""}
            fieldId={index() === 0
              ? props.fieldId
              : `${props.fieldId}-${index()}`}
            invalid={props.invalid}
            describedBy={props.invalid ? props.describedBy : undefined}
            onChange={(next) =>
              props.onChange(
                readItems().map((entry, position) =>
                  position === index() ? next : entry
                ),
              )}
          />
        </div>
      )}
    />
  );
}

/**
 * One component family for structured field editing, shared by the create
 * dialog and the Entry detail editor.
 *
 * Frontend owns display/interaction only: numbers stay number-oriented
 * text (`inputmode="decimal"` preserves partial input like "12."),
 * booleans use a typed checkbox, row references resolve through the shared
 * selector, and lists repeat the same row UI in both call sites. Validity
 * authority stays in Rust; nothing here converts strings via custom rules.
 * Asset kinds are a call-site override (the Entry pane injects AssetField);
 * this family falls back to the legacy text/textarea surface for them.
 */
export function FieldInput(props: FieldInputProps) {
  // Accessible row labels use the human field name; ids stay DOM-only.
  const labelName = () => props.fieldName ?? props.fieldId;
  if (
    props.field.type === "row_reference" &&
    hasRowReferencePicker(props.field, props.spaceId ?? "")
  ) {
    return (
      <RowReferenceSelect
        spaceId={props.spaceId ?? ""}
        targetForm={normalizeRowReferenceTargetForm(props.field)}
        forms={props.forms}
        value={typeof props.value === "string" ? props.value : ""}
        fieldId={props.fieldId}
        invalid={props.invalid}
        describedBy={props.invalid ? props.describedBy : undefined}
        onChange={props.onChange}
        onPendingChange={props.onRowReferencePendingChange}
      />
    );
  }

  if (isListRowReferenceField(props.field)) {
    if ((props.spaceId ?? "").trim() !== "") {
      return (
        <RowReferenceListRows
          {...props}
          fieldName={labelName()}
          targetForm={props.field.items?.target_form?.trim() ?? ""}
        />
      );
    }
  } else if (props.field.type === "list" && !props.field.items) {
    return <StringListRows {...props} fieldName={labelName()} />;
  } else if (
    props.field.type === "list" &&
    props.field.items?.type === "string"
  ) {
    return <StringListRows {...props} fieldName={labelName()} />;
  } else if (
    props.field.type === "list" &&
    ["integer", "long", "float", "double"].includes(
      props.field.items?.type ?? "",
    )
  ) {
    return <NumberListRows {...props} fieldName={labelName()} />;
  } else if (
    props.field.type === "list" && props.field.items?.type === "boolean"
  ) {
    return <BooleanListRows {...props} fieldName={labelName()} />;
  }

  if (props.field.type === "object_list") {
    return (
      <ObjectListEditor
        fieldName={labelName()}
        values={normalizeObjectListValue(props.value as never)}
        onChange={props.onChange}
        invalid={props.invalid ?? false}
        describedBy={props.invalid ? props.describedBy : undefined}
      />
    );
  }

  if (props.field.type === "boolean") {
    // Control kind follows the value so legacy text Rust would reject stays
    // readable: parseable values (including empty) get the typed checkbox,
    // unparseable legacy text keeps a text control. The switch is reactive
    // because drafts can arrive after mount (undefined first, value later).
    const useText = () => resolveBooleanCheckbox(props.value) === null;
    return (
      <Show
        when={useText()}
        fallback={
          <input
            id={props.fieldId}
            type="checkbox"
            checked={resolveBooleanCheckbox(props.value) ?? false}
            aria-invalid={props.invalid ? "true" : undefined}
            aria-describedby={props.invalid ? props.describedBy : undefined}
            onChange={(event) => props.onChange(event.currentTarget.checked)}
          />
        }
      >
        <ScalarTextInput
          fieldId={props.fieldId}
          type="text"
          value={fieldValueToText(props.value)}
          invalid={props.invalid}
          describedBy={props.describedBy}
          placeholder={props.placeholder}
          multiline={false}
          onChange={props.onChange}
        />
      </Show>
    );
  }

  if (NUMERIC_FIELD_TYPES.has(props.field.type)) {
    return (
      <ScalarTextInput
        fieldId={props.fieldId}
        type="text"
        inputMode="decimal"
        value={fieldValueToText(props.value)}
        invalid={props.invalid}
        describedBy={props.describedBy}
        placeholder={props.placeholder}
        multiline={false}
        onChange={props.onChange}
      />
    );
  }

  if (props.field.type === "date") {
    return (
      <ScalarTextInput
        fieldId={props.fieldId}
        type="date"
        value={fieldValueToText(props.value)}
        invalid={props.invalid}
        describedBy={props.describedBy}
        placeholder={props.placeholder}
        multiline={false}
        onChange={props.onChange}
      />
    );
  }

  if (
    props.field.type === "list" ||
    (props.multiline && typeof props.value === "string")
  ) {
    return (
      <ScalarTextInput
        fieldId={props.fieldId}
        type="text"
        value={fieldValueToText(props.value)}
        invalid={props.invalid}
        describedBy={props.describedBy}
        placeholder={props.placeholder}
        multiline
        onChange={props.onChange}
      />
    );
  }

  return (
    <ScalarTextInput
      fieldId={props.fieldId}
      type="text"
      value={fieldValueToText(props.value)}
      invalid={props.invalid}
      describedBy={props.describedBy}
      placeholder={props.placeholder}
      multiline={props.multiline ?? false}
      onChange={props.onChange}
    />
  );
}

/** Alias kept for the dialog/editor call sites migrating to one family. */
export const StructuredFieldInput = FieldInput;
