import {
  createEffect,
  createMemo,
  createResource,
  createSignal,
  createUniqueId,
  Show,
} from "solid-js";
import { Portal } from "solid-js/web";
import { ListEditor } from "~/components/ListEditor";
import { ObjectListEditor } from "~/components/ObjectListEditor";
import { UiIcon } from "~/components/UiIcon";
import { handleDialogKeyDown, useDialogFocus } from "~/components/dialog-focus";
import { RowReferenceSelect } from "~/components/fields/RowReferenceSelect";
import {
  hasRowReferencePicker,
  normalizeRowReferenceTargetForm,
} from "~/components/fields/row-reference";
import {
  draftValueToDisplayString,
  isBooleanListField,
  isPlainNumberListField,
  isPlainStringListField,
} from "~/lib/draft-values";
import { coerceEntryFieldValue } from "~/lib/ugoite-client/protocol";
import { t } from "~/lib/i18n";
import { renderMarkdownPreview } from "~/lib/markdown";
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

const isBoolean = (value: unknown): value is boolean =>
  typeof value === "boolean";
const isString = (value: unknown): value is string => typeof value === "string";
const isNumberInput = (value: unknown): value is number | string =>
  typeof value === "number" || isString(value);
const isObject = (value: unknown): value is Record<string, unknown> =>
  typeof value === "object" && value !== null && !Array.isArray(value);

const isArrayOf = <T,>(
  value: unknown,
  isItem: (item: unknown) => item is T,
): value is T[] => Array.isArray(value) && value.every(isItem);

/** Decode stored text only when it does not already fit the control. */
function createEditorValue<T>(
  props: FieldInputProps,
  accepts: (value: unknown) => value is T,
  itemType?: string,
) {
  const controlValue = createMemo(() => {
    const value = props.value;
    if (value == null || (isString(value) && value.trim() === "")) return null;
    return accepts(value) ? value : undefined;
  });
  const [parsed] = createResource(
    () => controlValue() === undefined ? { value: props.value } : false,
    ({ value }) =>
      coerceEntryFieldValue(props.field.type, itemType, value).catch(() =>
        undefined
      ),
  );
  return createMemo((): T | null | undefined => {
    const value = controlValue();
    if (value !== undefined || parsed.loading) return value;
    const decoded = parsed();
    return accepts(decoded) || decoded === null ? decoded : undefined;
  });
}

function MarkdownFieldPreview(props: { value: string }) {
  const [open, setOpen] = createSignal(false);
  const titleId = `markdown-field-preview-title-${createUniqueId()}`;
  let dialog: HTMLDivElement | undefined;
  let closeButton: HTMLButtonElement | undefined;
  let previewButton: HTMLButtonElement | undefined;

  const close = () => setOpen(false);
  const handleKeyDown = (event: KeyboardEvent) =>
    handleDialogKeyDown(event, dialog, close);

  useDialogFocus(() => open(), {
    dialog: () => dialog,
    initialFocus: () => closeButton,
    returnFocus: () => previewButton,
    onClose: close,
  });

  return (
    <>
      <div class="flex justify-end">
        <button
          ref={previewButton}
          type="button"
          class="ui-button ui-button-secondary ui-icon-button"
          aria-label={t("entryDetail.markdownPreview")}
          title={t("entryDetail.markdownPreview")}
          onClick={() => setOpen(true)}
        >
          <UiIcon name="preview" />
        </button>
      </div>
      <Portal>
        <Show when={open()}>
          <div
            class="ui-backdrop"
            role="presentation"
            onClick={(event) => {
              if (event.target === event.currentTarget) close();
            }}
          >
            <div
              ref={dialog}
              class="ui-dialog ui-markdown-field-preview-dialog"
              role="dialog"
              aria-modal="true"
              aria-labelledby={titleId}
              onKeyDown={handleKeyDown}
            >
              <header class="ui-dialog-header">
                <h2 id={titleId} class="ui-dialog-title">
                  {t("entryDetail.markdownPreview")}
                </h2>
                <button
                  ref={closeButton}
                  type="button"
                  class="ui-button ui-button-secondary ui-icon-button"
                  aria-label={t("common.close")}
                  title={t("common.close")}
                  onClick={close}
                >
                  <UiIcon name="close" />
                </button>
              </header>
              <div
                class="ui-markdown-field-preview"
                innerHTML={renderMarkdownPreview(props.value)}
              />
            </div>
          </div>
        </Show>
      </Portal>
    </>
  );
}

function ScalarTextInput(props: {
  fieldId: string;
  type: string;
  inputMode?: "decimal";
  step?: string;
  value: string;
  invalid?: boolean;
  describedBy?: string;
  placeholder?: string;
  multiline: boolean;
  markdownPreview?: boolean;
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
          step={props.step}
          value={props.value}
          aria-invalid={props.invalid ? "true" : undefined}
          aria-describedby={props.invalid ? props.describedBy : undefined}
          placeholder={props.placeholder ?? t("entryDetail.fieldPlaceholder")}
          onInput={(event) => props.onChange(event.currentTarget.value)}
        />
      }
    >
      <div class="ui-stack-sm">
        <Show when={props.markdownPreview}>
          <MarkdownFieldPreview value={props.value} />
        </Show>
        <textarea
          id={props.fieldId}
          class="ui-input ui-textarea"
          value={props.value}
          aria-invalid={props.invalid ? "true" : undefined}
          aria-describedby={props.invalid ? props.describedBy : undefined}
          placeholder={props.placeholder ?? t("entryDetail.fieldPlaceholder")}
          onInput={(event) => props.onChange(event.currentTarget.value)}
        />
      </div>
    </Show>
  );
}

function AutoGrowStringInput(props: {
  fieldId: string;
  value: string;
  invalid?: boolean;
  describedBy?: string;
  placeholder?: string;
  onChange: (value: string) => void;
}) {
  const [textarea, setTextarea] = createSignal<HTMLTextAreaElement>();
  const resize = (textarea: HTMLTextAreaElement) => {
    textarea.style.height = "auto";
    textarea.style.height = `${Math.max(textarea.scrollHeight, 38)}px`;
  };

  createEffect(() => {
    const element = textarea();
    props.value;
    if (element) resize(element);
  });

  return (
    <textarea
      ref={setTextarea}
      id={props.fieldId}
      class="ui-input ui-textarea ui-textarea-auto"
      rows={1}
      value={props.value}
      aria-invalid={props.invalid ? "true" : undefined}
      aria-describedby={props.invalid ? props.describedBy : undefined}
      placeholder={props.placeholder ?? t("entryDetail.fieldPlaceholder")}
      onInput={(event) => {
        props.onChange(event.currentTarget.value);
        resize(event.currentTarget);
      }}
    />
  );
}

function UuidInput(props: {
  fieldId: string;
  value: string;
  invalid?: boolean;
  describedBy?: string;
  placeholder?: string;
  onChange: (value: string) => void;
}) {
  return (
    <div class="flex items-center gap-2">
      <input
        id={props.fieldId}
        class="ui-input min-w-0"
        type="text"
        value={props.value}
        aria-invalid={props.invalid ? "true" : undefined}
        aria-describedby={props.invalid ? props.describedBy : undefined}
        placeholder={props.placeholder ?? t("entryDetail.fieldPlaceholder")}
        onInput={(event) => props.onChange(event.currentTarget.value)}
      />
      <button
        type="button"
        class="ui-button ui-button-secondary ui-icon-button shrink-0"
        aria-label={t("entryDetail.generateUuid")}
        title={t("entryDetail.generateUuid")}
        onClick={() => props.onChange(crypto.randomUUID())}
      >
        <UiIcon name="refresh" />
      </button>
    </div>
  );
}

const isZonedTimestampType = (type: string) =>
  type === "timestamp_tz" || type === "timestamp_tz_ns";

/** Format zoned values as the browser-local wall time shown by datetime-local. */
export const timestampValueForInput = (
  type: string,
  value: string,
): string => {
  if (!isZonedTimestampType(type) || !/(?:Z|[+-]\d{2}:\d{2})$/i.test(value)) {
    return value;
  }
  const parsed = new Date(value);
  if (Number.isNaN(parsed.getTime())) return value;
  const pad = (part: number) => String(part).padStart(2, "0");
  const fraction = value.match(/\.\d+(?=(?:Z|[+-]\d{2}:\d{2})$)/i)?.[0] ?? "";
  return `${parsed.getFullYear()}-${pad(parsed.getMonth() + 1)}-${pad(
    parsed.getDate(),
  )}T${pad(parsed.getHours())}:${pad(parsed.getMinutes())}:${pad(
    parsed.getSeconds(),
  )}${fraction}`;
};

const supportsDatetimeLocalValue = (value: string): boolean => {
  if (value === "") return true;
  const match = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})(?::(\d{2})(?:\.\d+)?)?$/.exec(
    value,
  );
  if (!match) return false;
  const parsed = new Date(
    `${match[1]}-${match[2]}-${match[3]}T${match[4]}:${match[5]}:${
      match[6] ?? "00"
    }`,
  );
  return !Number.isNaN(parsed.getTime()) &&
    parsed.getFullYear() === Number(match[1]) &&
    parsed.getMonth() + 1 === Number(match[2]) &&
    parsed.getDate() === Number(match[3]) &&
    parsed.getHours() === Number(match[4]) &&
    parsed.getMinutes() === Number(match[5]) &&
    parsed.getSeconds() === Number(match[6] ?? "00");
};

function UnparsedFieldInput(props: FieldInputProps) {
  return (
    <ScalarTextInput
      fieldId={props.fieldId}
      type="text"
      value={fieldValueToText(props.value)}
      invalid={props.invalid}
      describedBy={props.describedBy}
      placeholder={props.placeholder}
      multiline={props.multiline ?? false}
      markdownPreview={props.field.type === "markdown"}
      onChange={props.onChange}
    />
  );
}

function TextListRows(
  props: FieldInputProps & { fieldName: string; numeric: boolean },
) {
  // Decode legacy Markdown into text rows. Numeric items stay as entered until
  // the save boundary, so partial text and exact integers do not round-trip
  // through JavaScript Number.
  const value = createEditorValue(
    props,
    (value): value is (string | number)[] =>
      isArrayOf(value, props.numeric ? isNumberInput : isString),
  );
  const readItems = () => value() ?? [];
  return (
    <Show
      when={value() !== undefined}
      fallback={<UnparsedFieldInput {...props} multiline />}
    >
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
              inputmode={props.numeric ? "decimal" : undefined}
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
    </Show>
  );
}

function BooleanListRows(props: FieldInputProps & { fieldName: string }) {
  const value = createEditorValue(
    props,
    (value): value is (boolean | null)[] =>
      isArrayOf(
        value,
        (item): item is boolean | null => item === null || isBoolean(item),
      ),
    "boolean",
  );
  const readItems = () => value() ?? [];
  return (
    <Show
      when={value() !== undefined}
      fallback={<UnparsedFieldInput {...props} multiline />}
    >
      <ListEditor
        values={readItems()}
        onChange={props.onChange}
        createItem={() => false}
        addLabel={t("entryDetail.listAddItem")}
        renderItem={(item, index, helpers) => (
          <div class="flex items-center gap-2">
            <input
              id={index() === 0 ? props.fieldId : `${props.fieldId}-${index()}`}
              type="checkbox"
              checked={item() === true}
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
    </Show>
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
 * Frontend owns display/interaction only: strings grow from one row,
 * Markdown stays multiline, temporal fields use native controls, numbers
 * preserve partial text input, UUIDs can be generated, booleans use a typed
 * checkbox, and row references resolve through the shared selector. Validity
 * authority stays in Rust; nothing here validates values locally.
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
  } else if (
    isPlainStringListField(props.field) || isPlainNumberListField(props.field)
  ) {
    return (
      <TextListRows
        {...props}
        fieldName={labelName()}
        numeric={isPlainNumberListField(props.field)}
      />
    );
  } else if (isBooleanListField(props.field)) {
    return <BooleanListRows {...props} fieldName={labelName()} />;
  }

  if (props.field.type === "object_list") {
    const value = createEditorValue(
      props,
      (value): value is Record<string, unknown>[] => isArrayOf(value, isObject),
    );
    return (
      <Show
        when={value() !== undefined}
        fallback={<UnparsedFieldInput {...props} multiline />}
      >
        <ObjectListEditor
          fieldName={labelName()}
          values={value() ?? []}
          onChange={props.onChange}
          invalid={props.invalid ?? false}
          describedBy={props.invalid ? props.describedBy : undefined}
        />
      </Show>
    );
  }

  if (props.field.type === "boolean") {
    const value = createEditorValue(props, isBoolean);
    return (
      <Show
        when={value() !== undefined}
        fallback={<UnparsedFieldInput {...props} />}
      >
        <input
          id={props.fieldId}
          type="checkbox"
          checked={value() === true}
          aria-invalid={props.invalid ? "true" : undefined}
          aria-describedby={props.invalid ? props.describedBy : undefined}
          onChange={(event) => props.onChange(event.currentTarget.checked)}
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

  if (props.field.type === "time") {
    const value = fieldValueToText(props.value);
    const nativeValue = value === "" ||
      /^(?:[01]\d|2[0-3]):[0-5]\d(?::[0-5]\d(?:\.\d+)?)?$/.test(value);
    return (
      <ScalarTextInput
        fieldId={props.fieldId}
        type={nativeValue ? "time" : "text"}
        step={nativeValue ? "any" : undefined}
        value={value}
        invalid={props.invalid}
        describedBy={props.describedBy}
        placeholder={props.placeholder}
        multiline={false}
        onChange={props.onChange}
      />
    );
  }

  if (
    props.field.type === "timestamp" ||
    props.field.type === "timestamp_tz" ||
    props.field.type === "timestamp_ns" ||
    props.field.type === "timestamp_tz_ns"
  ) {
    const value = fieldValueToText(props.value);
    const inputValue = timestampValueForInput(props.field.type, value);
    const nativeValue = supportsDatetimeLocalValue(inputValue);
    return (
      <ScalarTextInput
        fieldId={props.fieldId}
        type={nativeValue ? "datetime-local" : "text"}
        step={nativeValue ? "any" : undefined}
        value={inputValue}
        invalid={props.invalid}
        describedBy={props.describedBy}
        placeholder={props.placeholder}
        multiline={false}
        onChange={props.onChange}
      />
    );
  }

  if (props.field.type === "uuid") {
    return (
      <UuidInput
        fieldId={props.fieldId}
        value={fieldValueToText(props.value)}
        invalid={props.invalid}
        describedBy={props.describedBy}
        placeholder={props.placeholder}
        onChange={props.onChange}
      />
    );
  }

  if (props.field.type === "string") {
    return (
      <AutoGrowStringInput
        fieldId={props.fieldId}
        value={fieldValueToText(props.value)}
        invalid={props.invalid}
        describedBy={props.describedBy}
        placeholder={props.placeholder}
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
        markdownPreview={props.field.type === "markdown"}
        onChange={props.onChange}
      />
    );
  }

  return <UnparsedFieldInput {...props} />;
}

/** Alias kept for the dialog/editor call sites migrating to one family. */
export const StructuredFieldInput = FieldInput;
