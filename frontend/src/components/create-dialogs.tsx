import {
  createEffect,
  createMemo,
  createSignal,
  For,
  Index,
  onCleanup,
  onMount,
  Show,
} from "solid-js";
import {
  buildEntryMarkdownFromFields,
  type EntryInputMode,
} from "~/lib/entry-input";
import { FieldStack, FieldStackRow } from "~/components/FieldStack";
import { FieldInput, fieldValueToText } from "~/components/fields";
import { FormTargetSelect } from "~/components/FormTargetSelect";
import { t, type TranslationKey } from "~/lib/i18n";
import { formatUserFacingError } from "~/lib/user-facing-error";
import type { Form, FormCreatePayload } from "~/lib/types";
import {
  isReservedMetadataColumn,
  RESERVED_METADATA_COLUMNS,
} from "~/lib/metadata-columns";
import {
  filterCreatableEntryForms,
  type FormNameValidationIssue,
  getFormNameValidationIssue,
  isReservedMetadataForm,
  RESERVED_METADATA_CLASSES,
} from "~/lib/metadata-forms";

const fieldTypeDescriptionKey = (type: string) => {
  if (type === "string") return "createDialog.form.fieldType.string";
  if (type === "sql") return "createDialog.form.fieldType.sql";
  if (type === "markdown") return "createDialog.form.fieldType.markdown";
  if (type === "number") return "createDialog.form.fieldType.number";
  if (type === "double") return "createDialog.form.fieldType.double";
  if (type === "float") return "createDialog.form.fieldType.float";
  if (type === "integer") return "createDialog.form.fieldType.integer";
  if (type === "long") return "createDialog.form.fieldType.long";
  if (type === "boolean") return "createDialog.form.fieldType.boolean";
  if (type === "date") return "createDialog.form.fieldType.date";
  if (type === "time") return "createDialog.form.fieldType.time";
  if (type === "timestamp") return "createDialog.form.fieldType.timestamp";
  if (type === "timestamp_tz") return "createDialog.form.fieldType.timestampTz";
  if (type === "timestamp_ns") return "createDialog.form.fieldType.timestampNs";
  if (type === "timestamp_tz_ns") {
    return "createDialog.form.fieldType.timestampTzNs";
  }
  if (type === "uuid") return "createDialog.form.fieldType.uuid";
  if (type === "asset_reference") {
    return "createDialog.form.fieldType.assetReference";
  }
  if (type === "row_reference") {
    return "createDialog.form.fieldType.rowReference";
  }
  if (type === "binary") return "createDialog.form.fieldType.binary";
  if (type === "list") return "createDialog.form.fieldType.list";
  if (type === "object_list") return "createDialog.form.fieldType.objectList";
  return "createDialog.form.fieldType.unknown";
};

const fieldTypeDescription = (type: string) => t(fieldTypeDescriptionKey(type));

const formatDatetimeLocal = (date: Date) => {
  const pad = (value: number) => String(value).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${
    pad(date.getDate())
  }T${
    pad(
      date.getHours(),
    )
  }:${pad(date.getMinutes())}`;
};

const isLongTextField = (name: string, def: Form["fields"][string]) =>
  def.type === "markdown" || name.toLowerCase() === "sql";

const isActiveRequiredField = (def: Form["fields"][string]) =>
  def.required && !def.deprecated;

const createFieldInputId = (prefix: string, name: string, index: number) => {
  const normalized = name
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9_-]+/g, "-")
    .replace(/^-+|-+$/g, "");
  return `${prefix}-${index}-${normalized || "field"}`;
};

/* v8 ignore start */
const resolveTextareaPlaceholder = () =>
  t("createDialog.entry.textareaPlaceholder");
/* v8 ignore stop */

const buildInputModeHints = (mode: EntryInputMode): string[] => {
  const hints = mode === "webform"
    ? [t("entryGuidance.webFormMode")]
    : [t("entryGuidance.editAfterCreate")];
  if (mode === "chat") {
    hints.push(t("entryGuidance.chatMode"));
  }
  if (mode === "markdown") {
    hints.push(t("entryGuidance.markdownMode"));
  }
  return hints;
};

const appendInputTypeHints = (
  hints: string[],
  types: Set<string>,
  mode: EntryInputMode,
): string[] => {
  if (types.has("list")) hints.push(t("entryGuidance.listValue"));
  if (types.has("boolean")) hints.push(t("entryGuidance.booleanValue"));
  if (types.has("row_reference")) {
    hints.push(
      t(
        mode === "markdown"
          ? "entryGuidance.rowReferenceMarkdown"
          : "entryGuidance.rowReference",
      ),
    );
  }
  if (types.has("asset_reference")) {
    hints.push(t("entryGuidance.assetReferenceMarkdown"));
  }
  return hints;
};

const columnTypeSelectClass =
  "ui-input ui-input-sm min-w-0 w-full sm:w-auto sm:min-w-[10rem] sm:flex-shrink-0";
const columnEditorRowClass = "flex flex-col gap-2 sm:flex-row sm:items-center";
const columnEditorControlsClass =
  "grid w-full grid-cols-[minmax(0,1fr)_auto] items-center gap-2 sm:flex sm:w-auto sm:flex-shrink-0";
const columnSharedInputClass =
  "ui-input ui-input-sm w-full sm:min-w-[14rem] sm:flex-1";
const columnNameInputClass = columnSharedInputClass;
const columnAuxRowClass =
  "ml-1 flex flex-col gap-2 sm:flex-row sm:items-center";
const columnAuxInputClass = columnSharedInputClass;

type FieldIssueSource = {
  name: string;
  type: string;
  targetForm?: string;
  itemsType?: string;
  itemsTargetForm?: string;
};

type FieldIssueContext = {
  availableForms?: string[];
  currentFormName?: string;
};

const getRowReferenceIssue = (
  field: FieldIssueSource,
  context?: FieldIssueContext,
) => {
  const isListRowReference = field.type === "list" &&
    field.itemsType === "row_reference";
  if (field.type !== "row_reference" && !isListRowReference) return null;
  const targetForm =
    (isListRowReference ? field.itemsTargetForm : field.targetForm)?.trim();
  /* v8 ignore start */
  if (!targetForm) return t("createDialog.validation.targetFormRequired");
  if (isReservedMetadataForm(targetForm)) {
    return t("createDialog.validation.targetFormReserved");
  }
  /* v8 ignore stop */
  /* v8 ignore start */
  if (context?.availableForms && context.availableForms.length > 0) {
    const validTargets = new Set(
      context.availableForms.map((formName) => formName.trim().toLowerCase()),
    );
    if (context.currentFormName?.trim()) {
      validTargets.add(context.currentFormName.trim().toLowerCase());
    }
    if (!validTargets.has(targetForm.toLowerCase())) {
      return t("createDialog.validation.targetFormDoesNotExist");
    }
  }
  /* v8 ignore stop */
  return null;
};

const buildFieldIssues = (
  fields: FieldIssueSource[],
  context?: FieldIssueContext,
) => {
  const issues = new Map<number, string>();
  const seen = new Map<string, number>();
  fields.forEach((field, index) => {
    const trimmed = field.name.trim();
    if (!trimmed) return;
    if (isReservedMetadataColumn(trimmed)) {
      issues.set(
        index,
        t("createDialog.validation.reservedMetadataColumnName"),
      );
      return;
    }
    const rowIssue = getRowReferenceIssue(field, context);
    /* v8 ignore start */
    if (rowIssue) {
      issues.set(index, rowIssue);
      return;
    }
    /* v8 ignore stop */
    const normalized = trimmed.toLowerCase();
    /* v8 ignore start */
    if (seen.has(normalized)) {
      issues.set(index, t("createDialog.validation.duplicateColumnName"));
      return;
    }
    /* v8 ignore stop */
    seen.set(normalized, index);
  });
  return issues;
};

const hasReservedMetadataFieldName = (fields: FieldIssueSource[]) =>
  fields.some((field) => {
    const trimmed = field.name.trim();
    return trimmed ? isReservedMetadataColumn(trimmed) : false;
  });

const formNameValidationMessage = (issue: FormNameValidationIssue) => {
  switch (issue) {
    case "syntax":
      return t("createDialog.validation.formNameSyntax");
    case "reserved":
      return t("createDialog.validation.reservedMetadataFormName");
    case "duplicate":
      return t("createDialog.validation.duplicateFormName");
  }
};

export interface CreateEntryDialogProps {
  open: boolean;
  forms: Form[];
  spaceId?: string;
  onClose: () => void;
  onSubmit: (
    title: string,
    formName: string,
    requiredValues: Record<string, string>,
    inputMode?: EntryInputMode,
  ) => Promise<void> | void;
}

function resolveSubmitErrorMessage(
  error: unknown,
  fallback: TranslationKey,
): string {
  return formatUserFacingError(error, fallback);
}

/**
 * Dialog for creating a new entry with optional form selection.
 */
export function CreateEntryDialog(props: CreateEntryDialogProps) {
  const [selectedForm, setSelectedForm] = createSignal("");
  const [inputMode, setInputMode] = createSignal<EntryInputMode>("webform");
  const [errorMessage, setErrorMessage] = createSignal<string | null>(null);
  const [fieldValues, setFieldValues] = createSignal<Record<string, unknown>>(
    {},
  );
  const [markdownInput, setMarkdownInput] = createSignal("");
  const [lastGeneratedMarkdown, setLastGeneratedMarkdown] = createSignal("");
  const [initializedFormName, setInitializedFormName] = createSignal("");
  // Unresolved row-reference searches per field (query names no saved entry
  // yet). Fed by the shared selector so required guards keep working.
  const [rowReferencePending, setRowReferencePending] = createSignal<
    Record<string, boolean>
  >({});
  const [chatStep, setChatStep] = createSignal(0);
  let dialogRef: HTMLDialogElement | undefined;

  const selectableForms = createMemo(() =>
    filterCreatableEntryForms(props.forms)
  );
  const pickerSpaceId = createMemo(() => props.spaceId?.trim() ?? "");

  const selectedFormDef = createMemo(() =>
    selectableForms().find((entryForm) => entryForm.name === selectedForm())
  );

  const requiredFields = createMemo(() => {
    const form = selectedFormDef();
    if (!form) return [] as Array<[string, Form["fields"][string]]>;
    /* v8 ignore start */
    return Object.entries(form.fields || {}).filter(([, def]) =>
      isActiveRequiredField(def)
    );
    /* v8 ignore stop */
  });

  const webFormFields = createMemo(() => {
    const form = selectedFormDef();
    if (!form) return [] as Array<[string, Form["fields"][string]]>;
    /* v8 ignore start */
    return Object.entries(form.fields || {});
    /* v8 ignore stop */
  });

  const chatFields = createMemo(() => webFormFields());

  /* v8 ignore start */
  const currentChatField = createMemo(() => {
    const fields = chatFields();
    if (fields.length === 0) return null;
    return fields[Math.min(chatStep(), fields.length - 1)] ?? null;
  });
  /* v8 ignore stop */

  const inputGuidance = createMemo(() => {
    const form = selectedFormDef();
    if (!form) return [] as string[];
    /* v8 ignore start */
    const types = new Set(
      Object.values(form.fields || {}).map((field) => field.type),
    );
    /* v8 ignore stop */
    return appendInputTypeHints(
      buildInputModeHints(inputMode()),
      types,
      inputMode(),
    );
  });

  const buildDefaultValue = (name: string, field: Form["fields"][string]) => {
    /* v8 ignore start */
    if (name.toLowerCase() === "sql") return "";
    switch (field.type) {
      case "integer":
      case "long":
      case "number":
      case "double":
      case "float":
        return "0";
      case "date": {
        return new Date().toISOString().slice(0, 10);
      }
      case "time": {
        return new Date().toTimeString().slice(0, 5);
      }
      case "timestamp":
      case "timestamp_tz":
      case "timestamp_ns":
      case "timestamp_tz_ns":
        return formatDatetimeLocal(new Date());
      case "object_list":
        return "[]";
      default:
        return "";
    }
    /* v8 ignore stop */
  };

  /* v8 ignore start */
  const handleKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Escape" && props.open) {
      props.onClose();
    }
  };

  onMount(() => {
    if (typeof document !== "undefined") {
      document.addEventListener("keydown", handleKeyDown);
    }
  });

  onCleanup(() => {
    if (typeof document !== "undefined") {
      document.removeEventListener("keydown", handleKeyDown);
    }
  });
  /* v8 ignore stop */

  // Reset form and focus when dialog opens
  /* v8 ignore start */
  const handleDialogClick = (e: MouseEvent) => {
    // Close if clicking on backdrop (the dialog element itself, not its content)
    if (e.target === dialogRef) {
      props.onClose();
    }
  };
  /* v8 ignore stop */

  createEffect(() => {
    /* v8 ignore next */
    if (!props.open) return;
    setErrorMessage(null);
    setInputMode("webform");
    setMarkdownInput("");
    setLastGeneratedMarkdown("");
    setInitializedFormName("");
    setRowReferencePending({});
    setChatStep(0);
    const availableForms = selectableForms();
    /* v8 ignore start */
    if (availableForms.length === 1) {
      setSelectedForm(availableForms[0].name);
    } else {
      setSelectedForm("");
    }
    /* v8 ignore stop */
  });

  createEffect(() => {
    if (!props.open) return;
    const form = selectedFormDef();
    if (!form) {
      setFieldValues({});
      setMarkdownInput("");
      setLastGeneratedMarkdown("");
      setInitializedFormName("");
      setRowReferencePending({});
      return;
    }
    if (initializedFormName() === form.name) {
      return;
    }
    const defaults: Record<string, unknown> = {};
    /* v8 ignore start */
    for (const [name, def] of Object.entries(form.fields || {})) {
      if (!isActiveRequiredField(def)) continue;
      defaults[name] = buildDefaultValue(name, def);
    }
    /* v8 ignore stop */
    setFieldValues(defaults);
    // Title-less Entry: the preview carries no H1; names live in fields.
    const generated = buildEntryMarkdownFromFields(
      form,
      "",
      serializeCreateValues(defaults),
    );
    setMarkdownInput(generated);
    setLastGeneratedMarkdown(generated);
    setInitializedFormName(form.name);
    setRowReferencePending({});
    setChatStep(0);
  });

  createEffect(() => {
    const form = selectedFormDef();
    if (!form) return;
    if (inputMode() !== "markdown") return;
    const generated = buildEntryMarkdownFromFields(
      form,
      "",
      serializeCreateValues(fieldValues()),
    );
    const current = markdownInput();
    const previousGenerated = lastGeneratedMarkdown();
    if (current === "" || current === previousGenerated) {
      setMarkdownInput(generated);
    }
    setLastGeneratedMarkdown(generated);
  });

  const setFieldValue = (name: string, nextValue: unknown) => {
    setErrorMessage(null);
    setFieldValues((prev) => ({
      ...prev,
      [name]: nextValue,
    }));
  };

  const clearFieldValue = (name: string) => {
    setErrorMessage(null);
    setFieldValues((prev) => {
      const next = { ...prev };
      delete next[name];
      return next;
    });
  };

  /** Display text for required/answered checks. Rust owns validity. */
  const fieldText = (name: string) => fieldValueToText(fieldValues()[name]);

  /**
   * Serialize the typed draft for the legacy string contract (submit payload
   * and Markdown preview). Scalars pass through untouched; collections use
   * the same display encoding the editors read back. Blanks drop out like
   * before; object items are never filtered here — Rust rejects empty
   * objects with field guidance.
   */
  const serializeCreateValues = (
    values: Record<string, unknown>,
  ): Record<string, string> => {
    const serialized: Record<string, string> = {};
    for (const [name, value] of Object.entries(values)) {
      if (value === null || value === undefined) continue;
      if (typeof value === "boolean") {
        if (value) serialized[name] = "true";
        continue;
      }
      if (typeof value === "number") {
        serialized[name] = String(value);
        continue;
      }
      if (Array.isArray(value)) {
        if (value.length === 0) continue;
        if (
          value.every((item) => typeof item === "string" && item.trim() !== "")
        ) {
          const text = (value as string[])
            .map((item) => `- ${item}`)
            .join("\n");
          if (text.trim()) serialized[name] = text;
          continue;
        }
        if (
          value.every((item) =>
            typeof item === "string" || typeof item === "number" ||
            typeof item === "boolean"
          )
        ) {
          const items = (value as Array<string | number | boolean>).filter(
            (item) => String(item).trim() !== "",
          );
          if (items.length === 0) continue;
          serialized[name] = items.map((item) => `- ${String(item)}`).join(
            "\n",
          );
          continue;
        }
        try {
          serialized[name] = JSON.stringify(value);
        } catch {
          continue;
        }
        continue;
      }
      if (typeof value === "object") {
        try {
          serialized[name] = JSON.stringify(value);
        } catch {
          continue;
        }
        continue;
      }
      const text = value;
      if (!text.trim()) continue;
      serialized[name] = text;
    }
    return serialized;
  };

  const rowReferenceSelectionPending = (
    name: string,
    def: Form["fields"][string],
  ) =>
    !def.deprecated &&
    def.type === "row_reference" &&
    (def.target_form?.trim() ?? "") !== "" &&
    Boolean(pickerSpaceId()) &&
    (rowReferencePending()[name] ?? false);

  const firstUnresolvedRowReferenceField = (
    fields: Array<[string, Form["fields"][string]]>,
  ) =>
    fields.find(([name, def]) => rowReferenceSelectionPending(name, def)) ??
      null;

  const renderFieldInput = (
    prefix: "webform" | "chat",
    name: string,
    def: Form["fields"][string],
    index: number,
  ) => {
    const fieldId = createFieldInputId(prefix, name, index);
    return (
      <FieldInput
        field={def}
        value={fieldValues()[name]}
        onChange={(next) => {
          // Clearing a row reference drops the key from the draft (submit
          // payloads omit it, as before); other types keep legacy behavior.
          if (next === "" && def.type === "row_reference") {
            clearFieldValue(name);
            return;
          }
          setFieldValue(name, next);
        }}
        fieldId={fieldId}
        fieldName={name}
        spaceId={pickerSpaceId()}
        multiline={isLongTextField(name, def)}
        placeholder={isLongTextField(name, def)
          ? resolveTextareaPlaceholder()
          : undefined}
        onRowReferencePendingChange={(pending) =>
          setRowReferencePending((prev) => ({ ...prev, [name]: pending }))}
      />
    );
  };

  const moveChatStep = (delta: number) =>
    setChatStep((prev) => {
      const lastIndex = Math.max(chatFields().length - 1, 0);
      return Math.min(Math.max(prev + delta, 0), lastIndex);
    });

  const goToChatStep = (nextStep: number) =>
    setChatStep(
      Math.min(Math.max(nextStep, 0), Math.max(chatFields().length - 1, 0)),
    );

  /* v8 ignore start */
  const handleAdvanceChatStep = () => {
    const current = currentChatField();
    if (!current) return;
    const [name, def] = current;
    if (rowReferenceSelectionPending(name, def)) {
      setErrorMessage(
        t("createDialog.entry.error.selectRowReference", { field: name }),
      );
      return;
    }
    if (isActiveRequiredField(def) && !fieldText(name).trim()) {
      setErrorMessage(
        t("createDialog.entry.error.answerRequired", { field: name }),
      );
      return;
    }
    setErrorMessage(null);
    moveChatStep(1);
  };

  const handleSkipChatField = () => {
    const current = currentChatField();
    if (!current) return;
    const [name, def] = current;
    if (isActiveRequiredField(def)) {
      setErrorMessage(
        t("createDialog.entry.error.skipRequired", { field: name }),
      );
      return;
    }
    clearFieldValue(name);
    setRowReferencePending((prev) => {
      const next = { ...prev };
      delete next[name];
      return next;
    });
    setErrorMessage(null);
    moveChatStep(1);
  };
  /* v8 ignore stop */

  const resetEntryDraft = () => {
    setErrorMessage(null);
    setSelectedForm("");
    setRowReferencePending({});
    setMarkdownInput("");
    setLastGeneratedMarkdown("");
    setChatStep(0);
  };

  const buildMissingRequiredFieldsMessage = (missing: string[]) =>
    t("createDialog.entry.error.fillRequiredFields", {
      fields: missing.join(", "),
    });

  const validateStructuredSubmission = () => {
    const unresolvedRowReference = firstUnresolvedRowReferenceField(
      webFormFields(),
    );
    if (unresolvedRowReference) {
      return t("createDialog.entry.error.selectRowReference", {
        field: unresolvedRowReference[0],
      });
    }
    const missing = requiredFields()
      .map(([name]) => name)
      .filter((name) => !fieldText(name).trim());
    return missing.length > 0
      ? buildMissingRequiredFieldsMessage(missing)
      : null;
  };

  const validateMarkdownSubmission = () =>
    markdownInput().trim()
      ? null
      : t("createDialog.entry.error.provideMarkdown");

  // Title-less Entry: the legacy title argument is always empty; names
  // live in Form fields. The callback shape is kept for compatibility.
  const submitEntry = async (_entryTitle: string, formName: string) => {
    if (inputMode() === "markdown") {
      await props.onSubmit(
        "",
        formName,
        { __markdown: markdownInput().trim() },
        "markdown",
      );
      return;
    }
    await props.onSubmit("", formName, fieldValues(), inputMode());
  };

  const handleSubmit = async (e: Event) => {
    e.preventDefault();
    const formName = selectedForm().trim();
    /* v8 ignore start */
    if (!formName) {
      setErrorMessage(t("createDialog.entry.error.provideForm"));
      return;
    }
    /* v8 ignore stop */
    setErrorMessage(null);
    const submitError = inputMode() === "markdown"
      ? validateMarkdownSubmission()
      : validateStructuredSubmission();
    if (submitError) {
      setErrorMessage(submitError);
      return;
    }
    try {
      await submitEntry("", formName);
      resetEntryDraft();
    } catch (error) {
      setErrorMessage(
        resolveSubmitErrorMessage(
          error,
          "dashboard.error.failedCreateEntry",
        ),
      );
    }
  };

  /* v8 ignore start */
  return (
    <Show when={props.open}>
      <dialog
        ref={dialogRef}
        open
        class="fixed inset-0 z-[70] flex items-center justify-center ui-backdrop w-full h-full"
        onClick={handleDialogClick}
        onKeyDown={(e) => {
          if (e.key === "Escape") props.onClose();
        }}
      >
        <div
          class="ui-dialog w-full max-w-md mx-4 flex flex-col max-h-[90vh] overflow-y-auto"
          role="document"
          onClick={(e) => e.stopPropagation()}
          onKeyDown={(e) => e.stopPropagation()}
        >
          <h2 class="text-lg font-semibold mb-4">
            {t("createDialog.entry.heading")}
          </h2>

          <form onSubmit={handleSubmit} class="ui-stack-sm">
            <Show
              when={selectableForms().length > 0}
              fallback={
                <div class="ui-card ui-card-dashed text-sm ui-muted">
                  {t("createDialog.entry.empty")}
                </div>
              }
            >
              <div class="ui-field">
                <label class="ui-label" for="entry-form">
                  {t("createDialog.entry.formLabel")}{" "}
                  <span class="ui-text-danger">*</span>
                </label>
                <select
                  id="entry-form"
                  class="ui-input"
                  value={selectedForm()}
                  onChange={(e) => {
                    setSelectedForm(e.currentTarget.value);
                    setErrorMessage(null);
                  }}
                >
                  <option value="" disabled>
                    {t("createDialog.entry.formPlaceholder")}
                  </option>
                  <For each={selectableForms()}>
                    {(entryForm) => (
                      <option value={entryForm.name}>{entryForm.name}</option>
                    )}
                  </For>
                </select>
                <Show when={selectedFormDef()}>
                  {(entryForm) => (
                    <div class="ui-card mt-3">
                      <p class="text-xs font-semibold ui-muted uppercase tracking-wide">
                        {t("createDialog.entry.fieldsTitle")}
                      </p>
                      <div class="mt-2 flex flex-wrap gap-2">
                        <Show
                          when={Object.keys(entryForm().fields || {}).length >
                            0}
                          fallback={
                            <span class="text-xs ui-muted">
                              {t("createDialog.entry.noFieldsDefined")}
                            </span>
                          }
                        >
                          <For each={Object.entries(entryForm().fields)}>
                            {([name, def]) => (
                              <span class="ui-pill gap-1">
                                <span class="font-medium">{name}</span>
                                <span class="ui-muted">({def.type})</span>
                                <Show when={isActiveRequiredField(def)}>
                                  <span class="ui-text-danger">*</span>
                                </Show>
                              </span>
                            )}
                          </For>
                        </Show>
                      </div>
                    </div>
                  )}
                </Show>
              </div>
            </Show>

            <div class="ui-field">
              <p class="ui-label">{t("createDialog.entry.inputMode")}</p>
              <div class="flex flex-wrap gap-2">
                <button
                  type="button"
                  class={`ui-button text-xs ${
                    inputMode() === "webform"
                      ? "ui-button-primary"
                      : "ui-button-secondary"
                  }`}
                  onClick={() => {
                    setErrorMessage(null);
                    setInputMode("webform");
                  }}
                >
                  {t("createDialog.entry.inputMode.webform")}
                </button>
                <button
                  type="button"
                  class={`ui-button text-xs ${
                    inputMode() === "markdown"
                      ? "ui-button-primary"
                      : "ui-button-secondary"
                  }`}
                  onClick={() => {
                    setErrorMessage(null);
                    setInputMode("markdown");
                  }}
                >
                  {t("createDialog.entry.inputMode.markdown")}
                </button>
                <button
                  type="button"
                  class={`ui-button text-xs ${
                    inputMode() === "chat"
                      ? "ui-button-primary"
                      : "ui-button-secondary"
                  }`}
                  onClick={() => {
                    setErrorMessage(null);
                    setInputMode("chat");
                  }}
                >
                  {t("createDialog.entry.inputMode.chat")}
                </button>
              </div>
            </div>

            <Show when={selectedFormDef() && inputGuidance().length > 0}>
              <div class="ui-alert ui-alert-warning text-xs space-y-1">
                <For each={inputGuidance()}>{(hint) => <p>{hint}</p>}</For>
              </div>
            </Show>

            <Show when={inputMode() === "markdown" && selectedFormDef()}>
              <div class="ui-card">
                <p class="text-sm font-semibold">
                  {t("createDialog.entry.markdownTitle")}
                </p>
                <textarea
                  aria-label={t("createDialog.entry.markdownAria")}
                  class="ui-input ui-textarea mt-3 min-h-56"
                  value={markdownInput()}
                  onInput={(e) => {
                    setErrorMessage(null);
                    setMarkdownInput(e.currentTarget.value);
                  }}
                />
              </div>
            </Show>

            <Show
              when={inputMode() === "webform" && webFormFields().length > 0}
            >
              <div class="ui-card">
                <p class="text-sm font-semibold">
                  {t("createDialog.entry.formFieldsTitle")}
                </p>
                <div class="ui-stack-sm mt-3">
                  <For each={webFormFields()}>
                    {([name, def], index) => {
                      const fieldId = createFieldInputId(
                        "webform",
                        name,
                        index(),
                      );
                      return (
                        <div class="ui-field">
                          <label class="ui-label" for={fieldId}>
                            {name}
                            <span class="ui-muted ml-2 text-xs">
                              {t(
                                isActiveRequiredField(def)
                                  ? "createDialog.entry.fieldMeta.required"
                                  : "createDialog.entry.fieldMeta.optional",
                                { type: def.type },
                              )}
                            </span>
                          </label>
                          {renderFieldInput("webform", name, def, index())}
                        </div>
                      );
                    }}
                  </For>
                </div>
              </div>
            </Show>

            <Show when={inputMode() === "chat" && chatFields().length > 0}>
              <div class="ui-card">
                <p class="text-sm font-semibold">
                  {t("createDialog.entry.chatTitle")}
                </p>
                <p class="text-xs ui-muted mt-1">
                  {t("createDialog.entry.chatProgress", {
                    current: Math.min(chatStep() + 1, chatFields().length),
                    total: chatFields().length,
                  })}
                </p>
                <div class="mt-3 flex flex-wrap gap-2">
                  <For each={chatFields()}>
                    {([name, def], index) => {
                      const answered = () =>
                        !!(fieldValues()[name] || "").trim();
                      const current = () => index() === chatStep();
                      return (
                        <button
                          type="button"
                          class={`ui-button text-xs ${
                            current()
                              ? "ui-button-primary"
                              : "ui-button-secondary"
                          }`}
                          onClick={() => {
                            goToChatStep(index());
                            setErrorMessage(null);
                          }}
                        >
                          {name} (
                          {answered()
                            ? t("createDialog.entry.chatStatus.answered")
                            : isActiveRequiredField(def)
                            ? t("createDialog.entry.chatStatus.required")
                            : t("createDialog.entry.chatStatus.optional")}
                          )
                        </button>
                      );
                    }}
                  </For>
                </div>
                <Show when={currentChatField()} keyed>
                  {(current) => {
                    const [name, def] = current;
                    const fieldIndex = chatFields().findIndex(
                      ([candidateName]) => candidateName === name,
                    );
                    const fieldId = createFieldInputId(
                      "chat",
                      name,
                      Math.max(fieldIndex, 0),
                    );
                    return (
                      <div class="ui-field mt-3">
                        <label class="ui-label" for={fieldId}>
                          {name}
                          <span class="ui-muted ml-2 text-xs">
                            {t(
                              isActiveRequiredField(def)
                                ? "createDialog.entry.chatFieldMeta.required"
                                : "createDialog.entry.chatFieldMeta.optional",
                              { type: def.type },
                            )}
                          </span>
                        </label>
                        {renderFieldInput(
                          "chat",
                          name,
                          def,
                          Math.max(fieldIndex, 0),
                        )}
                        <div class="mt-3 flex items-center justify-between">
                          <button
                            type="button"
                            class="ui-button ui-button-secondary text-xs"
                            disabled={chatStep() <= 0}
                            onClick={() => moveChatStep(-1)}
                          >
                            {t("createDialog.entry.chatPrevious")}
                          </button>
                          <div class="flex items-center gap-2">
                            <button
                              type="button"
                              class="ui-button ui-button-secondary text-xs"
                              onClick={handleSkipChatField}
                            >
                              {t(
                                isActiveRequiredField(def)
                                  ? "createDialog.entry.chatSkip"
                                  : "createDialog.entry.chatSkipOptional",
                              )}
                            </button>
                            <Show when={chatStep() < chatFields().length - 1}>
                              <button
                                type="button"
                                class="ui-button ui-button-secondary text-xs"
                                onClick={handleAdvanceChatStep}
                              >
                                {t("createDialog.entry.chatNext")}
                              </button>
                            </Show>
                          </div>
                        </div>
                      </div>
                    );
                  }}
                </Show>
              </div>
            </Show>

            <Show when={errorMessage()}>
              <div class="ui-alert ui-alert-error text-sm" role="alert">
                {errorMessage()}
              </div>
            </Show>

            <div class="ui-dialog-actions pt-2">
              <button
                type="button"
                onClick={props.onClose}
                class="ui-button ui-button-secondary text-sm"
              >
                {t("common.cancel")}
              </button>
              <button
                type="submit"
                disabled={!selectedForm().trim() || selectableForms().length ===
                    0}
                class="ui-button ui-button-primary text-sm"
              >
                {t("common.create")}
              </button>
            </div>
          </form>
        </div>
      </dialog>
    </Show>
  );
  /* v8 ignore stop */
}

export interface CreateFormDialogProps {
  open: boolean;
  columnTypes: string[];
  formNames: string[];
  onClose: () => void;
  onSubmit: (payload: FormCreatePayload) => Promise<void> | void;
}

/**
 * Dialog for creating a new form.
 */
export function CreateFormDialog(props: CreateFormDialogProps) {
  const [name, setName] = createSignal("");
  const [fields, setFields] = createSignal<
    Array<
      {
        name: string;
        type: string;
        required: boolean;
        targetForm?: string;
        itemsType?: string;
        itemsTargetForm?: string;
      }
    >
  >([]);
  const [submitError, setSubmitError] = createSignal<string | null>(null);
  let inputRef: HTMLInputElement | undefined;
  let dialogRef: HTMLDialogElement | undefined;
  const listItemTypes = createMemo(() =>
    props.columnTypes.filter((type) =>
      type !== "list" && type !== "object_list"
    )
  );

  const fieldIssues = createMemo(() =>
    buildFieldIssues(fields(), {
      availableForms: props.formNames,
      currentFormName: name(),
    })
  );

  const nameValidationIssue = createMemo(() => {
    const value = name().trim();
    return value ? getFormNameValidationIssue(value, props.formNames) : null;
  });
  const nameIssue = createMemo(() => {
    const issue = nameValidationIssue();
    return issue ? formNameValidationMessage(issue) : "";
  });

  const showReservedNameGuidance = createMemo(
    () =>
      hasReservedMetadataFieldName(fields()) ||
      nameValidationIssue() === "reserved",
  );

  const hasFieldIssues = createMemo(() => fieldIssues().size > 0);

  createEffect(() => {
    if (!props.open) return;
    setSubmitError(null);
  });

  // Handle escape key
  /* v8 ignore start */
  const handleKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Escape" && props.open) {
      props.onClose();
    }
  };

  onMount(() => {
    if (typeof document !== "undefined") {
      document.addEventListener("keydown", handleKeyDown);
    }
  });

  onCleanup(() => {
    if (typeof document !== "undefined") {
      document.removeEventListener("keydown", handleKeyDown);
    }
  });

  const handleDialogClick = (e: MouseEvent) => {
    if (e.target === dialogRef) {
      props.onClose();
    }
  };
  /* v8 ignore stop */

  const handleSubmit = async (e: Event) => {
    e.preventDefault();
    const formName = name().trim();
    /* v8 ignore start */
    if (!formName || hasFieldIssues()) return;
    if (nameIssue()) {
      inputRef?.focus();
      return;
    }
    /* v8 ignore stop */
    setSubmitError(null);

    const fieldRecord: Record<
      string,
      {
        type: string;
        required: boolean;
        target_form?: string;
        items?: { type: string; target_form?: string };
      }
    > = {};
    let template = `# ${formName}\n\n`;

    for (const f of fields()) {
      const trimmedName = f.name.trim();
      /* v8 ignore start */
      if (trimmedName) {
        const target_form = f.type === "row_reference"
          ? f.targetForm?.trim()
          : undefined;
        const items = f.type === "list" && f.itemsType
          ? {
            type: f.itemsType,
            target_form: f.itemsType === "row_reference"
              ? f.itemsTargetForm?.trim()
              : undefined,
          }
          : undefined;
        /* v8 ignore stop */
        fieldRecord[trimmedName] = {
          type: f.type,
          required: f.required,
          target_form,
          items,
        };
        template += `## ${trimmedName}\n\n`;
      }
    }

    try {
      await props.onSubmit({
        name: formName,
        template,
        fields: fieldRecord,
        allow_extra_attributes: "deny",
      });
      setName("");
      setFields([]);
    } catch (error) {
      setSubmitError(
        resolveSubmitErrorMessage(
          error,
          "dashboard.error.failedCreateForm",
        ),
      );
    }
  };

  const addField = () => {
    setSubmitError(null);
    setFields([...fields(), {
      name: "",
      type: "string",
      required: false,
      itemsType: "",
    }]);
  };

  const removeField = (index: number) => {
    setSubmitError(null);
    const newFields = [...fields()];
    newFields.splice(index, 1);
    setFields(newFields);
  };

  const updateField = (
    index: number,
    key: keyof (typeof fields extends () => infer R ? R : never)[0],
    value: string | boolean,
  ) => {
    setSubmitError(null);
    const newFields = [...fields()];
    newFields[index] = {
      ...newFields[index],
      [key]: value,
    } as (typeof fields extends () => infer R ? R
      : never)[0];
    setFields(newFields);
  };

  /* v8 ignore start */
  return (
    <Show when={props.open}>
      <dialog
        ref={dialogRef}
        open
        class="fixed inset-0 z-[70] flex items-center justify-center ui-backdrop w-full h-full"
        onClick={handleDialogClick}
        onKeyDown={(e) => {
          if (e.key === "Escape") props.onClose();
        }}
      >
        <div
          class="ui-dialog ui-dialog-form flex flex-col"
          role="document"
          onClick={(e) => e.stopPropagation()}
          onKeyDown={(e) => e.stopPropagation()}
        >
          <h2 class="text-lg font-semibold mb-4">
            {t("createDialog.form.heading")}
          </h2>

          <form
            onSubmit={handleSubmit}
            class="ui-dialog-form-content"
          >
            <div class="ui-field">
              <label class="ui-label" for="form-name">
                {t("createDialog.form.nameLabel")}
              </label>
              <input
                ref={inputRef}
                id="form-name"
                type="text"
                value={name()}
                onInput={(e) => {
                  setSubmitError(null);
                  setName(e.currentTarget.value);
                }}
                placeholder={t("createDialog.form.namePlaceholder")}
                class="ui-input"
                classList={{ "ui-input-error": Boolean(nameIssue()) }}
                aria-invalid={Boolean(nameIssue()) || undefined}
                aria-describedby={nameIssue()
                  ? "form-name-help form-name-error"
                  : "form-name-help"}
                autofocus
              />
              <p id="form-name-help" class="text-xs ui-muted">
                {t("createDialog.form.nameHelp")}
              </p>
              <Show when={nameIssue()}>
                <span id="form-name-error" class="text-xs ui-text-danger">
                  {nameIssue()}
                </span>
              </Show>
            </div>

            <div class="ui-stack-sm">
              <div class="flex justify-between items-center">
                <span class="text-sm font-semibold">
                  {t("createDialog.form.columnsTitle")}
                </span>
                <button
                  type="button"
                  onClick={addField}
                  class="ui-button ui-button-secondary ui-button-sm text-xs"
                >
                  {t("createDialog.form.addColumn")}
                </button>
              </div>

              <FieldStack label={t("createDialog.form.columnsTitle")}>
                <Index each={fields()}>
                  {(field, i) => (
                    <FieldStackRow class="flex flex-col gap-1">
                      <div class={columnEditorRowClass}>
                        <input
                          type="text"
                          placeholder={t(
                            "createDialog.form.columnNamePlaceholder",
                          )}
                          value={field().name}
                          onInput={(e) =>
                            updateField(i, "name", e.currentTarget.value)}
                          class={columnNameInputClass}
                          classList={{ "ui-input-error": fieldIssues().has(i) }}
                          aria-invalid={fieldIssues().has(i) || undefined}
                        />
                        <div class={columnEditorControlsClass}>
                          <select
                            aria-label={t("createDialog.form.fieldTypeLabel")}
                            value={field().type}
                            onChange={(e) =>
                              updateField(i, "type", e.currentTarget.value)}
                            class={columnTypeSelectClass}
                            aria-describedby={`create-form-field-type-${i}-description`}
                          >
                            <For each={props.columnTypes}>
                              {(type) => <option value={type}>{type}</option>}
                            </For>
                          </select>
                          <button
                            type="button"
                            onClick={() => removeField(i)}
                            class="ui-button ui-button-secondary ui-button-sm"
                            aria-label={t("createDialog.form.removeColumnAria")}
                          >
                            ×
                          </button>
                        </div>
                      </div>
                      <p
                        id={`create-form-field-type-${i}-description`}
                        class="ml-1 text-xs ui-muted"
                      >
                        {fieldTypeDescription(field().type)}
                      </p>
                      <label class="ml-1 inline-flex items-center gap-2 text-xs ui-muted">
                        <input
                          type="checkbox"
                          checked={field().required}
                          onChange={(event) =>
                            updateField(
                              i,
                              "required",
                              event.currentTarget.checked,
                            )}
                        />
                        {t("createDialog.form.requiredLabel")}
                      </label>
                      <Show when={field().type === "row_reference"}>
                        <div class={columnAuxRowClass}>
                          <span class="text-xs ui-muted">
                            {t("createDialog.form.targetFormLabel")}
                          </span>
                          <FormTargetSelect
                            label={t("createDialog.form.targetFormLabel")}
                            value={field().targetForm || ""}
                            options={props.formNames}
                            placeholder={t(
                              "createDialog.form.targetFormPlaceholder",
                            )}
                            inputClass={columnAuxInputClass}
                            onChange={(value) =>
                              updateField(i, "targetForm", value)}
                          />
                        </div>
                      </Show>
                      <Show when={field().type === "list"}>
                        <div class={columnAuxRowClass}>
                          <span class="text-xs ui-muted">
                            {t("createDialog.form.listItemTypeLabel")}
                          </span>
                          <select
                            class={columnAuxInputClass}
                            aria-label={t(
                              "createDialog.form.listItemTypeLabel",
                            )}
                            value={field().itemsType || ""}
                            onChange={(event) =>
                              updateField(
                                i,
                                "itemsType",
                                event.currentTarget.value,
                              )}
                          >
                            <option value="">
                              {t("createDialog.form.listItemTypeText")}
                            </option>
                            <For each={listItemTypes()}>
                              {(type) => (
                                <option value={type}>
                                  {type === "asset_reference"
                                    ? t("createDialog.form.listItemTypeAsset")
                                    : type}
                                </option>
                              )}
                            </For>
                          </select>
                        </div>
                        <Show when={field().itemsType === "row_reference"}>
                          <div class={columnAuxRowClass}>
                            <span class="text-xs ui-muted">
                              {t("createDialog.form.targetFormLabel")}
                            </span>
                            <FormTargetSelect
                              label={t("createDialog.form.targetFormLabel")}
                              value={field().itemsTargetForm || ""}
                              options={props.formNames}
                              placeholder={t(
                                "createDialog.form.targetFormPlaceholder",
                              )}
                              inputClass={columnAuxInputClass}
                              onChange={(value) =>
                                updateField(i, "itemsTargetForm", value)}
                            />
                          </div>
                        </Show>
                      </Show>
                      <Show when={fieldIssues().has(i)}>
                        <span class="text-xs ui-text-danger">
                          {fieldIssues().get(i)}
                        </span>
                      </Show>
                    </FieldStackRow>
                  )}
                </Index>
              </FieldStack>
              <Show when={fields().length === 0}>
                <div class="ui-card text-sm ui-muted italic text-center">
                  {t("createDialog.form.noColumnsDefined")}
                </div>
              </Show>
              <Show when={showReservedNameGuidance()}>
                <div class="ui-alert ui-alert-warning text-xs space-y-1">
                  <p>
                    {t("createDialog.form.warning.reservedColumns", {
                      columns: RESERVED_METADATA_COLUMNS.join(", "),
                    })}
                  </p>
                  <p>
                    {t("createDialog.form.warning.reservedForms", {
                      forms: RESERVED_METADATA_CLASSES.join(", "),
                    })}
                  </p>
                  <p>{t("createDialog.form.warning.listFields")}</p>
                  <p>{t("createDialog.form.warning.booleanFields")}</p>
                </div>
              </Show>
            </div>
            <Show when={submitError()}>
              <div class="ui-alert ui-alert-error text-sm" role="alert">
                {submitError()}
              </div>
            </Show>

            <div class="ui-dialog-actions pt-4">
              <button
                type="button"
                onClick={props.onClose}
                class="ui-button ui-button-secondary text-sm"
              >
                {t("common.cancel")}
              </button>
              <button
                type="submit"
                disabled={!name().trim() || hasFieldIssues()}
                class="ui-button ui-button-primary text-sm"
              >
                {t("createDialog.form.create")}
              </button>
            </div>
          </form>
        </div>
      </dialog>
    </Show>
  );
}
/* v8 ignore stop */

function processFields(
  fields: Array<{
    name: string;
    type: string;
    required: boolean;
    deprecated?: boolean;
    targetForm?: string;
    itemsType?: string;
    itemsTargetForm?: string;
  }>,
) {
  const fieldRecord: Record<
    string,
    {
      type: string;
      required: boolean;
      deprecated?: boolean;
      target_form?: string;
      items?: { type: string; target_form?: string };
    }
  > = {};
  for (const f of fields) {
    const trimmedName = f.name.trim();
    /* v8 ignore start */
    if (trimmedName) {
      const target_form = f.type === "row_reference"
        ? f.targetForm?.trim()
        : undefined;
      const items = f.type === "list" && f.itemsType
        ? {
          type: f.itemsType,
          target_form: f.itemsType === "row_reference"
            ? f.itemsTargetForm?.trim()
            : undefined,
        }
        : undefined;
      /* v8 ignore stop */
      fieldRecord[trimmedName] = {
        type: f.type,
        required: f.required,
        ...(f.deprecated ? { deprecated: true } : {}),
        target_form,
        items,
      };
    }
  }

  return fieldRecord;
}

const findUnsupportedEditChanges = (
  fields: Array<{ name: string; type: string; itemsType?: string }>,
  existingFields: Record<
    string,
    { type: string; items?: { type?: string } }
  >,
) => {
  const fieldTypeSignature = (type: string, itemsType?: string) =>
    type === "list" ? `${type}<${itemsType || "string"}>` : type;
  const draftFields = new Map(
    fields
      .map((field) => [field.name.trim(), field] as const)
      .filter(([name]) => Boolean(name)),
  );
  const issues: string[] = [];
  for (const [name, existing] of Object.entries(existingFields)) {
    const draft = draftFields.get(name);
    if (!draft) {
      issues.push(t("createDialog.form.editIssue.removed", { field: name }));
    } else if (
      fieldTypeSignature(draft.type, draft.itemsType) !==
        fieldTypeSignature(existing.type, existing.items?.type)
    ) {
      issues.push(
        t("createDialog.form.editIssue.typeChanged", {
          field: name,
          from: fieldTypeSignature(existing.type, existing.items?.type),
          to: fieldTypeSignature(draft.type, draft.itemsType),
        }),
      );
    }
  }
  return issues;
};

export interface EditFormDialogProps {
  open: boolean;
  entryForm: Form;
  columnTypes: string[];
  formNames: string[];
  onClose: () => void;
  onSubmit: (payload: FormCreatePayload) => Promise<void> | void;
}

export function EditFormDialog(props: EditFormDialogProps) {
  const [fields, setFields] = createSignal<
    Array<{
      name: string;
      type: string;
      required: boolean;
      deprecated?: boolean;
      targetForm?: string;
      itemsType?: string;
      itemsTargetForm?: string;
      isNew?: boolean;
    }>
  >([]);
  const [submitError, setSubmitError] = createSignal<string | null>(null);
  let dialogRef: HTMLDialogElement | undefined;
  const listItemTypes = createMemo(() =>
    props.columnTypes.filter((type) =>
      type !== "list" && type !== "object_list"
    )
  );

  const fieldIssues = createMemo(() =>
    buildFieldIssues(fields(), {
      availableForms: props.formNames,
      currentFormName: props.entryForm?.name,
    })
  );

  const unsupportedEditChanges = createMemo(() =>
    findUnsupportedEditChanges(fields(), props.entryForm.fields)
  );
  const hasFieldIssues = createMemo(() =>
    fieldIssues().size > 0 || unsupportedEditChanges().length > 0
  );
  const nameIssue = createMemo(
    () =>
      /* v8 ignore start */
      props.entryForm ? isReservedMetadataForm(props.entryForm.name) : false,
    /* v8 ignore stop */
  );
  const showReservedNameGuidance = createMemo(
    () => hasReservedMetadataFieldName(fields()) || nameIssue(),
  );

  /* v8 ignore start */
  const handleKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Escape" && props.open) {
      props.onClose();
    }
  };

  onMount(() => {
    if (typeof document !== "undefined") {
      document.addEventListener("keydown", handleKeyDown);
    }
  });

  onCleanup(() => {
    if (typeof document !== "undefined") {
      document.removeEventListener("keydown", handleKeyDown);
    }
  });
  /* v8 ignore stop */

  createEffect(() => {
    /* v8 ignore next */
    if (props.open && props.entryForm) {
      setSubmitError(null);
      const initialFields = Object.entries(props.entryForm.fields).map((
        [name, def],
      ) => ({
        name,
        type: def.type,
        required: def.required,
        deprecated: def.deprecated,
        targetForm: def.target_form,
        itemsType: def.items?.type,
        itemsTargetForm: def.items?.target_form,
        isNew: false,
      }));
      setFields(initialFields);
    }
  });

  /* v8 ignore start */
  const handleDialogClick = (e: MouseEvent) => {
    if (e.target === dialogRef) {
      props.onClose();
    }
  };
  /* v8 ignore stop */

  const handleSubmit = async (e: Event) => {
    e.preventDefault();
    /* v8 ignore start */
    if (hasFieldIssues() || nameIssue()) return;
    /* v8 ignore stop */
    setSubmitError(null);

    const fieldRecord = processFields(fields());

    let template = `# ${props.entryForm.name}\n\n`;
    for (const f of fields()) {
      /* v8 ignore start */
      if (f.name.trim()) template += `## ${f.name.trim()}\n\n`;
      /* v8 ignore stop */
    }

    try {
      await props.onSubmit({
        name: props.entryForm.name,
        template,
        fields: fieldRecord,
        allow_extra_attributes: props.entryForm.allow_extra_attributes ??
          "deny",
      });
    } catch (error) {
      setSubmitError(
        resolveSubmitErrorMessage(
          error,
          "dashboard.error.failedUpdateForm",
        ),
      );
    }
  };

  const addField = () => {
    setSubmitError(null);
    setFields([...fields(), {
      name: "",
      type: "string",
      required: false,
      itemsType: "",
      isNew: true,
    }]);
  };

  const removeField = (index: number) => {
    setSubmitError(null);
    const newFields = [...fields()];
    newFields.splice(index, 1);
    setFields(newFields);
  };

  const updateField = (
    index: number,
    key: keyof (typeof fields extends () => infer R ? R : never)[0],
    value: string | boolean,
  ) => {
    setSubmitError(null);
    const newFields = [...fields()];
    newFields[index] = {
      ...newFields[index],
      [key]: value,
    } as (typeof fields extends () => infer R ? R
      : never)[0];
    setFields(newFields);
  };

  /* v8 ignore start */
  return (
    <Show when={props.open}>
      <dialog
        ref={dialogRef}
        open
        class="fixed inset-0 z-[70] flex items-center justify-center ui-backdrop w-full h-full"
        onClick={handleDialogClick}
        onKeyDown={(e) => {
          if (e.key === "Escape") props.onClose();
        }}
      >
        <div
          class="ui-dialog ui-dialog-form flex flex-col"
          role="document"
          onClick={(e) => e.stopPropagation()}
          onKeyDown={(e) => e.stopPropagation()}
        >
          <h2 class="text-lg font-semibold mb-4">
            {t("createDialog.form.editHeading", {
              name: props.entryForm?.name ?? "",
            })}
          </h2>
          <div class="ui-alert ui-alert-warning text-sm">
            <p>
              <strong>{t("createDialog.form.editWarningLabel")}</strong>{" "}
              {t("createDialog.form.editWarningBody")}
            </p>
          </div>

          <form
            onSubmit={handleSubmit}
            class="ui-dialog-form-content"
          >
            <div class="ui-stack-sm">
              <div class="flex justify-between items-center">
                <span class="text-sm font-semibold">
                  {t("createDialog.form.columnsTitle")}
                </span>
                <button
                  type="button"
                  onClick={addField}
                  class="ui-button ui-button-secondary ui-button-sm text-xs"
                >
                  {t("createDialog.form.addColumn")}
                </button>
              </div>

              <FieldStack label={t("createDialog.form.columnsTitle")}>
                <Index each={fields()}>
                  {(field, i) => (
                    <FieldStackRow class="flex flex-col gap-1 border-b pb-2 mb-2 last:border-0">
                      <div class={columnEditorRowClass}>
                        <input
                          type="text"
                          placeholder={t(
                            "createDialog.form.columnNamePlaceholder",
                          )}
                          disabled={!field().isNew &&
                            !!props.entryForm.fields[field().name]}
                          value={field().name}
                          onInput={(e) =>
                            updateField(i, "name", e.currentTarget.value)}
                          class={columnNameInputClass}
                          classList={{
                            "ui-input-error": fieldIssues().has(i) &&
                              field().isNew,
                          }}
                          aria-invalid={fieldIssues().has(i) || undefined}
                          title={!field().isNew
                            ? t("createDialog.form.renameHint")
                            : ""}
                        />
                        <div class={columnEditorControlsClass}>
                          <select
                            aria-label={t("createDialog.form.fieldTypeLabel")}
                            value={field().type}
                            onChange={(e) =>
                              updateField(i, "type", e.currentTarget.value)}
                            disabled={!field().isNew}
                            class={columnTypeSelectClass}
                            aria-describedby={`edit-form-field-type-${i}-description`}
                          >
                            <For each={props.columnTypes}>
                              {(type) => <option value={type}>{type}</option>}
                            </For>
                          </select>
                          <button
                            type="button"
                            onClick={() => removeField(i)}
                            disabled={!field().isNew}
                            class="ui-button ui-button-secondary ui-button-sm"
                            aria-label={t("createDialog.form.removeColumnAria")}
                          >
                            ×
                          </button>
                        </div>
                      </div>
                      <p
                        id={`edit-form-field-type-${i}-description`}
                        class="ml-1 text-xs ui-muted"
                      >
                        {fieldTypeDescription(field().type)}
                      </p>
                      <label class="ml-1 inline-flex items-center gap-2 text-xs ui-muted">
                        <input
                          type="checkbox"
                          checked={field().required}
                          onChange={(event) =>
                            updateField(
                              i,
                              "required",
                              event.currentTarget.checked,
                            )}
                        />
                        {t("createDialog.form.requiredLabel")}
                      </label>
                      <Show when={field().type === "row_reference"}>
                        <div class={columnAuxRowClass}>
                          <span class="text-xs ui-muted">
                            {t("createDialog.form.targetFormLabel")}
                          </span>
                          <FormTargetSelect
                            label={t("createDialog.form.targetFormLabel")}
                            value={field().targetForm || ""}
                            options={props.formNames}
                            placeholder={t(
                              "createDialog.form.targetFormPlaceholder",
                            )}
                            inputClass={columnAuxInputClass}
                            onChange={(value) =>
                              updateField(i, "targetForm", value)}
                          />
                        </div>
                      </Show>
                      <Show when={field().type === "list"}>
                        <div class={columnAuxRowClass}>
                          <span class="text-xs ui-muted">
                            {t("createDialog.form.listItemTypeLabel")}
                          </span>
                          <select
                            class={columnAuxInputClass}
                            aria-label={t(
                              "createDialog.form.listItemTypeLabel",
                            )}
                            value={field().itemsType || ""}
                            disabled={!field().isNew}
                            onChange={(event) =>
                              updateField(
                                i,
                                "itemsType",
                                event.currentTarget.value,
                              )}
                          >
                            <option value="">
                              {t("createDialog.form.listItemTypeText")}
                            </option>
                            <For each={listItemTypes()}>
                              {(type) => (
                                <option value={type}>
                                  {type === "asset_reference"
                                    ? t("createDialog.form.listItemTypeAsset")
                                    : type}
                                </option>
                              )}
                            </For>
                          </select>
                        </div>
                        <Show when={field().itemsType === "row_reference"}>
                          <div class={columnAuxRowClass}>
                            <span class="text-xs ui-muted">
                              {t("createDialog.form.targetFormLabel")}
                            </span>
                            <FormTargetSelect
                              label={t("createDialog.form.targetFormLabel")}
                              value={field().itemsTargetForm || ""}
                              options={props.formNames}
                              placeholder={t(
                                "createDialog.form.targetFormPlaceholder",
                              )}
                              inputClass={columnAuxInputClass}
                              onChange={(value) =>
                                updateField(i, "itemsTargetForm", value)}
                            />
                          </div>
                        </Show>
                      </Show>
                      <Show when={fieldIssues().has(i) && field().isNew}>
                        <span class="text-xs ui-text-danger">
                          {fieldIssues().get(i)}
                        </span>
                      </Show>
                    </FieldStackRow>
                  )}
                </Index>
              </FieldStack>
              <Show when={fields().length === 0}>
                <div class="ui-card text-sm ui-muted italic text-center">
                  {t("createDialog.form.noColumnsDefined")}
                </div>
              </Show>
              <Show when={showReservedNameGuidance()}>
                <div class="ui-alert ui-alert-warning text-xs space-y-1">
                  <p>
                    {t("createDialog.form.warning.reservedColumns", {
                      columns: RESERVED_METADATA_COLUMNS.join(", "),
                    })}
                  </p>
                  <Show when={nameIssue()}>
                    <p>
                      {t("createDialog.form.warning.reservedFormsEdit", {
                        forms: RESERVED_METADATA_CLASSES.join(", "),
                      })}
                    </p>
                  </Show>
                  <p>{t("createDialog.form.warning.listFields")}</p>
                  <p>{t("createDialog.form.warning.booleanFields")}</p>
                </div>
              </Show>
              <Show when={unsupportedEditChanges().length > 0}>
                <div class="ui-alert ui-alert-error text-xs" role="alert">
                  <p>{t("createDialog.form.editIssue.heading")}</p>
                  <ul class="mt-1 list-disc pl-5">
                    <For each={unsupportedEditChanges()}>
                      {(issue) => <li>{issue}</li>}
                    </For>
                  </ul>
                </div>
              </Show>
            </div>
            <Show when={submitError()}>
              <div class="ui-alert ui-alert-error text-sm" role="alert">
                {submitError()}
              </div>
            </Show>

            <div class="ui-dialog-actions pt-4">
              <button
                type="button"
                onClick={props.onClose}
                class="ui-button ui-button-secondary text-sm"
              >
                {t("common.cancel")}
              </button>
              <button
                type="submit"
                disabled={hasFieldIssues() || Boolean(nameIssue())}
                class="ui-button ui-button-primary text-sm"
              >
                {t("createDialog.form.saveChanges")}
              </button>
            </div>
          </form>
        </div>
      </dialog>
    </Show>
  );
}

/* v8 ignore stop */
