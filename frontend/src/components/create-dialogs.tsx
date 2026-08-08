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
import { t, type TranslationKey } from "~/lib/i18n";
import { createResource } from "~/lib/recoverable-resource";
import { searchApi } from "~/lib/ugoite-client";
import { formatUserFacingError } from "~/lib/user-facing-error";
import type { Form, FormCreatePayload } from "~/lib/types";
import {
  isReservedMetadataColumn,
  RESERVED_METADATA_COLUMNS,
} from "~/lib/metadata-columns";
import {
  filterCreatableEntryForms,
  isReservedMetadataForm,
  RESERVED_METADATA_CLASSES,
} from "~/lib/metadata-forms";

const numericFieldTypes = new Set([
  "integer",
  "long",
  "number",
  "double",
  "float",
]);

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

const isTextareaField = (name: string, def: Form["fields"][string]) =>
  isLongTextField(name, def) || def.type === "object_list";

const createFieldInputId = (prefix: string, name: string, index: number) => {
  const normalized = name
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9_-]+/g, "-")
    .replace(/^-+|-+$/g, "");
  return `${prefix}-${index}-${normalized || "field"}`;
};

/* v8 ignore start */
const resolveTextareaPlaceholder = (def: Form["fields"][string]) => {
  return def.type === "object_list"
    ? "[]"
    : t("createDialog.entry.textareaPlaceholder");
};
/* v8 ignore stop */

type RowReferenceOption = {
  id: string;
  title: string;
  label: string;
};

const rowReferenceSuggestionLimit = 8;

const normalizeRowReferenceTargetForm = (def: Form["fields"][string]) =>
  def.target_form?.trim() ?? "";

const buildRowReferenceOptions = (
  entries: Array<{ id: string; title?: string | null }>,
): RowReferenceOption[] =>
  entries
    .map((entry) => {
      const title = entry.title?.trim() || entry.id;
      return {
        id: entry.id,
        title,
        label: title === entry.id ? entry.id : `${title} (${entry.id})`,
      };
    })
    .sort(
      (left, right) =>
        left.title.localeCompare(right.title) ||
        left.id.localeCompare(right.id),
    );

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

export interface CreateEntryDialogProps {
  open: boolean;
  forms: Form[];
  spaceId?: string;
  defaultForm?: string;
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

type RowReferencePickerProps = {
  spaceId: string;
  fieldId: string;
  targetForm: string;
  query: string;
  selectedOption: RowReferenceOption | null;
  onQueryInput: (value: string) => void;
  onSelect: (option: RowReferenceOption) => void;
  onClear: () => void;
};

function RowReferencePicker(props: RowReferencePickerProps) {
  const [options] = createResource(
    () => ({
      spaceId: props.spaceId.trim(),
      targetForm: props.targetForm.trim(),
      query: props.query,
    }),
    async ({ spaceId, targetForm, query }) => {
      const entries = await searchApi.rowReferenceOptions(
        spaceId,
        targetForm,
        query,
        rowReferenceSuggestionLimit,
      );
      return buildRowReferenceOptions(entries);
    },
    {
      initialValue: [] as RowReferenceOption[],
    },
  );

  return (
    <div class="ui-stack-sm">
      <input
        id={props.fieldId}
        type="search"
        class="ui-input"
        value={props.query}
        placeholder={t("createDialog.entry.rowReference.searchPlaceholder", {
          form: props.targetForm,
        })}
        onInput={(event) => props.onQueryInput(event.currentTarget.value)}
        autocomplete="off"
      />
      <p class="text-xs ui-muted">
        {t("createDialog.entry.rowReference.help", { form: props.targetForm })}
      </p>
      <Show when={props.selectedOption}>
        {(selectedOption) => (
          <div class="ui-reference-picker-selection">
            <p class="text-[11px] font-semibold uppercase tracking-wide ui-muted">
              {t("createDialog.entry.rowReference.selected")}
            </p>
            <div class="flex flex-wrap items-center justify-between gap-3">
              <div>
                <p class="text-sm font-medium">{selectedOption().title}</p>
                <p class="text-xs ui-muted">{selectedOption().id}</p>
              </div>
              <button
                type="button"
                class="ui-button ui-button-secondary text-xs"
                onClick={props.onClear}
              >
                {t("createDialog.entry.rowReference.clear")}
              </button>
            </div>
          </div>
        )}
      </Show>
      <Show when={options.loading}>
        <p class="text-xs ui-muted">
          {t("createDialog.entry.rowReference.loading", {
            form: props.targetForm,
          })}
        </p>
      </Show>
      <Show when={!options.loading && options.error}>
        <p class="text-xs ui-text-danger">
          {t("createDialog.entry.rowReference.loadError", {
            form: props.targetForm,
          })}
        </p>
      </Show>
      <Show when={!options.loading && !options.error && options().length > 0}>
        <ul class="ui-reference-picker-list">
          <For each={options()}>
            {(option) => (
              <li class="ui-reference-picker-option">
                <button
                  type="button"
                  class="ui-reference-picker-button"
                  onClick={() => props.onSelect(option)}
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
        when={!options.loading && !options.error && props.query.trim() &&
          options().length === 0}
      >
        <p class="text-xs ui-muted">
          {t("createDialog.entry.rowReference.noMatches", {
            form: props.targetForm,
          })}
        </p>
      </Show>
    </div>
  );
}

/**
 * Dialog for creating a new entry with optional form selection.
 */
export function CreateEntryDialog(props: CreateEntryDialogProps) {
  const [title, setTitle] = createSignal("");
  const [selectedForm, setSelectedForm] = createSignal("");
  const [inputMode, setInputMode] = createSignal<EntryInputMode>("webform");
  const [errorMessage, setErrorMessage] = createSignal<string | null>(null);
  const [fieldValues, setFieldValues] = createSignal<Record<string, string>>(
    {},
  );
  const [markdownInput, setMarkdownInput] = createSignal("");
  const [lastGeneratedMarkdown, setLastGeneratedMarkdown] = createSignal("");
  const [initializedFormName, setInitializedFormName] = createSignal("");
  const [rowReferenceQueries, setRowReferenceQueries] = createSignal<
    Record<string, string>
  >({});
  const [rowReferenceSelections, setRowReferenceSelections] = createSignal<
    Record<string, RowReferenceOption>
  >({});
  const [chatStep, setChatStep] = createSignal(0);
  let inputRef: HTMLInputElement | undefined;
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
    return Object.entries(form.fields || {}).filter(([, def]) => def.required);
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
    setTitle("");
    setInputMode("webform");
    setMarkdownInput("");
    setLastGeneratedMarkdown("");
    setInitializedFormName("");
    setRowReferenceQueries({});
    setRowReferenceSelections({});
    setChatStep(0);
    const availableForms = selectableForms();
    /* v8 ignore start */
    const defaultForm = props.defaultForm?.trim();
    if (
      defaultForm &&
      availableForms.some((entryForm) => entryForm.name === defaultForm)
    ) {
      setSelectedForm(defaultForm);
    } else if (availableForms.length === 1) {
      setSelectedForm(availableForms[0].name);
    } else {
      setSelectedForm("");
    }
    /* v8 ignore stop */
    inputRef?.focus();
  });

  createEffect(() => {
    if (!props.open) return;
    const form = selectedFormDef();
    if (!form) {
      setFieldValues({});
      setMarkdownInput("");
      setLastGeneratedMarkdown("");
      setInitializedFormName("");
      setRowReferenceQueries({});
      setRowReferenceSelections({});
      return;
    }
    if (initializedFormName() === form.name) {
      return;
    }
    const defaults: Record<string, string> = {};
    /* v8 ignore start */
    for (const [name, def] of Object.entries(form.fields || {})) {
      if (!def.required) continue;
      defaults[name] = buildDefaultValue(name, def);
    }
    /* v8 ignore stop */
    setFieldValues(defaults);
    const generated = buildEntryMarkdownFromFields(
      form,
      title().trim() || form.name,
      defaults,
    );
    setMarkdownInput(generated);
    setLastGeneratedMarkdown(generated);
    setInitializedFormName(form.name);
    setRowReferenceQueries({});
    setRowReferenceSelections({});
    setChatStep(0);
  });

  createEffect(() => {
    const form = selectedFormDef();
    if (!form) return;
    if (inputMode() !== "markdown") return;
    const generated = buildEntryMarkdownFromFields(
      form,
      title().trim() || form.name,
      fieldValues(),
    );
    const current = markdownInput();
    const previousGenerated = lastGeneratedMarkdown();
    if (current === "" || current === previousGenerated) {
      setMarkdownInput(generated);
    }
    setLastGeneratedMarkdown(generated);
  });

  const setFieldValue = (name: string, nextValue: string) => {
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

  const setRowReferenceQuery = (name: string, nextValue: string) =>
    setRowReferenceQueries((prev) => ({
      ...prev,
      [name]: nextValue,
    }));

  const clearRowReferenceQuery = (name: string) =>
    setRowReferenceQueries((prev) => {
      const next = { ...prev };
      delete next[name];
      return next;
    });

  const clearRowReferenceSelectionState = (name: string) =>
    setRowReferenceSelections((prev) => {
      const next = { ...prev };
      delete next[name];
      return next;
    });

  const hasRowReferencePicker = (def: Form["fields"][string]) =>
    Boolean(pickerSpaceId()) &&
    def.type === "row_reference" &&
    normalizeRowReferenceTargetForm(def) !== "";

  const resolveSelectedRowReferenceOption = (name: string) =>
    rowReferenceSelections()[name] ?? null;

  const rowReferenceSelectionPending = (
    name: string,
    def: Form["fields"][string],
  ) =>
    hasRowReferencePicker(def) &&
    (rowReferenceQueries()[name] ?? "").trim() !== "" &&
    !(fieldValues()[name] ?? "").trim();

  const handleRowReferenceQueryInput = (name: string, nextQuery: string) => {
    setRowReferenceQuery(name, nextQuery);
    clearFieldValue(name);
    clearRowReferenceSelectionState(name);
    setErrorMessage(null);
  };

  const handleRowReferenceSelect = (
    name: string,
    option: RowReferenceOption,
  ) => {
    setRowReferenceQuery(name, option.label);
    setFieldValue(name, option.id);
    setRowReferenceSelections((prev) => ({
      ...prev,
      [name]: option,
    }));
    setErrorMessage(null);
  };

  const clearRowReferenceSelection = (name: string) => {
    clearRowReferenceQuery(name);
    clearFieldValue(name);
    clearRowReferenceSelectionState(name);
    setErrorMessage(null);
  };

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
    if (hasRowReferencePicker(def)) {
      return (
        <RowReferencePicker
          spaceId={pickerSpaceId()}
          fieldId={fieldId}
          targetForm={normalizeRowReferenceTargetForm(def)}
          query={rowReferenceQueries()[name] ?? ""}
          selectedOption={resolveSelectedRowReferenceOption(name)}
          onQueryInput={(nextQuery) =>
            handleRowReferenceQueryInput(name, nextQuery)}
          onSelect={(option) => handleRowReferenceSelect(name, option)}
          onClear={() => clearRowReferenceSelection(name)}
        />
      );
    }

    const useTextarea = isTextareaField(name, def);
    const value = fieldValues()[name] ?? "";

    if (useTextarea) {
      return (
        <textarea
          id={fieldId}
          class="ui-input ui-textarea"
          placeholder={resolveTextareaPlaceholder(def)}
          value={value}
          onInput={(event) => setFieldValue(name, event.currentTarget.value)}
        />
      );
    }

    return (
      <input
        id={fieldId}
        type="text"
        inputmode={numericFieldTypes.has(def.type) ? "decimal" : undefined}
        class="ui-input"
        value={value}
        onInput={(event) => setFieldValue(name, event.currentTarget.value)}
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
    if (def.required && !(fieldValues()[name] || "").trim()) {
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
    if (def.required) {
      setErrorMessage(
        t("createDialog.entry.error.skipRequired", { field: name }),
      );
      return;
    }
    clearRowReferenceQuery(name);
    clearFieldValue(name);
    setErrorMessage(null);
    moveChatStep(1);
  };
  /* v8 ignore stop */

  const resetEntryDraft = () => {
    setErrorMessage(null);
    setTitle("");
    setSelectedForm("");
    setRowReferenceQueries({});
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
      .filter((name) => !(fieldValues()[name] || "").trim());
    return missing.length > 0
      ? buildMissingRequiredFieldsMessage(missing)
      : null;
  };

  const validateMarkdownSubmission = () =>
    markdownInput().trim()
      ? null
      : t("createDialog.entry.error.provideMarkdown");

  const submitEntry = async (entryTitle: string, formName: string) => {
    if (inputMode() === "markdown") {
      await props.onSubmit(
        entryTitle,
        formName,
        { __markdown: markdownInput().trim() },
        "markdown",
      );
      return;
    }
    await props.onSubmit(entryTitle, formName, fieldValues(), inputMode());
  };

  const handleSubmit = async (e: Event) => {
    e.preventDefault();
    const entryTitle = title().trim();
    const formName = selectedForm().trim();
    /* v8 ignore start */
    if (!entryTitle || !formName) {
      setErrorMessage(t("createDialog.entry.error.provideTitleAndForm"));
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
      await submitEntry(entryTitle, formName);
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
            <div class="ui-field">
              <label class="ui-label" for="entry-title">
                {t("createDialog.entry.titleLabel")}
              </label>
              <input
                ref={inputRef}
                id="entry-title"
                type="text"
                value={title()}
                onInput={(e) => {
                  setErrorMessage(null);
                  setTitle(e.currentTarget.value);
                }}
                placeholder={t("createDialog.entry.titlePlaceholder")}
                class="ui-input"
                autofocus
              />
            </div>

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
                                <Show when={def.required}>
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
                                def.required
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
                            : def.required
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
                              def.required
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
                                def.required
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
                disabled={!title().trim() || !selectedForm().trim() ||
                  selectableForms().length === 0}
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
  const targetFormOptions = createMemo(() => {
    const options = new Set(props.formNames);
    const current = name().trim();
    if (current) options.add(current);
    return Array.from(options);
  });
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

  const nameIssue = createMemo(
    () =>
      /* v8 ignore start */
      isReservedMetadataForm(name())
        ? t("createDialog.validation.reservedMetadataFormName")
        : "",
    /* v8 ignore stop */
  );

  const showReservedNameGuidance = createMemo(
    () => hasReservedMetadataFieldName(fields()) || Boolean(nameIssue()),
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
    if (!formName || hasFieldIssues() || nameIssue()) return;
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
                autofocus
              />
              <Show when={nameIssue()}>
                <span class="text-xs ui-text-danger">{nameIssue()}</span>
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

              <Index each={fields()}>
                {(field, i) => (
                  <div class="flex flex-col gap-1">
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
                        <input
                          type="text"
                          list={`row-ref-targets-${i}`}
                          aria-label={t("createDialog.form.targetFormLabel")}
                          placeholder={t(
                            "createDialog.form.targetFormPlaceholder",
                          )}
                          value={field().targetForm || ""}
                          onInput={(e) =>
                            updateField(i, "targetForm", e.currentTarget.value)}
                          class={columnAuxInputClass}
                        />
                        <datalist id={`row-ref-targets-${i}`}>
                          <For each={targetFormOptions()}>
                            {(option) => <option value={option} />}
                          </For>
                        </datalist>
                      </div>
                    </Show>
                    <Show when={field().type === "list"}>
                      <div class={columnAuxRowClass}>
                        <span class="text-xs ui-muted">
                          {t("createDialog.form.listItemTypeLabel")}
                        </span>
                        <select
                          class={columnAuxInputClass}
                          aria-label={t("createDialog.form.listItemTypeLabel")}
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
                          <input
                            type="text"
                            list={`list-row-ref-targets-${i}`}
                            aria-label={t("createDialog.form.targetFormLabel")}
                            placeholder={t(
                              "createDialog.form.targetFormPlaceholder",
                            )}
                            value={field().itemsTargetForm || ""}
                            onInput={(event) =>
                              updateField(
                                i,
                                "itemsTargetForm",
                                event.currentTarget.value,
                              )}
                            class={columnAuxInputClass}
                          />
                          <datalist id={`list-row-ref-targets-${i}`}>
                            <For each={targetFormOptions()}>
                              {(option) => <option value={option} />}
                            </For>
                          </datalist>
                        </div>
                      </Show>
                    </Show>
                    <Show when={fieldIssues().has(i)}>
                      <span class="text-xs ui-text-danger">
                        {fieldIssues().get(i)}
                      </span>
                    </Show>
                  </div>
                )}
              </Index>
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
                disabled={!name().trim() || hasFieldIssues() ||
                  Boolean(nameIssue())}
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
    targetForm?: string;
    itemsType?: string;
    itemsTargetForm?: string;
    defaultValue?: string;
  }>,
  existingFields: Record<
    string,
    {
      type: string;
      required: boolean;
      target_form?: string;
      items?: { type: string; target_form?: string };
    }
  >,
) {
  const fieldRecord: Record<
    string,
    {
      type: string;
      required: boolean;
      target_form?: string;
      items?: { type: string; target_form?: string };
    }
  > = {};
  const strategies: Record<string, string | null> = {};

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
        target_form,
        items,
      };
      if (!existingFields[trimmedName] && f.defaultValue) {
        strategies[trimmedName] = f.defaultValue;
      }
    }
  }

  return { fieldRecord, strategies };
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
      targetForm?: string;
      itemsType?: string;
      itemsTargetForm?: string;
      defaultValue?: string;
      isNew?: boolean;
    }>
  >([]);
  const [submitError, setSubmitError] = createSignal<string | null>(null);
  let dialogRef: HTMLDialogElement | undefined;
  const targetFormOptions = createMemo(() => {
    const options = new Set(props.formNames);
    /* v8 ignore start */
    if (props.entryForm?.name) options.add(props.entryForm.name);
    /* v8 ignore stop */
    return Array.from(options);
  });
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

    const { fieldRecord, strategies } = processFields(
      fields(),
      props.entryForm.fields,
    );

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
        strategies: Object.keys(strategies).length > 0 ? strategies : undefined,
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

              <Index each={fields()}>
                {(field, i) => (
                  <div class="flex flex-col gap-1 border-b pb-2 mb-2 last:border-0">
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
                        <input
                          type="text"
                          list={`row-ref-targets-edit-${i}`}
                          aria-label={t("createDialog.form.targetFormLabel")}
                          placeholder={t(
                            "createDialog.form.targetFormPlaceholder",
                          )}
                          value={field().targetForm || ""}
                          onInput={(e) =>
                            updateField(i, "targetForm", e.currentTarget.value)}
                          class={columnAuxInputClass}
                        />
                        <datalist id={`row-ref-targets-edit-${i}`}>
                          <For each={targetFormOptions()}>
                            {(option) => <option value={option} />}
                          </For>
                        </datalist>
                      </div>
                    </Show>
                    <Show when={field().type === "list"}>
                      <div class={columnAuxRowClass}>
                        <span class="text-xs ui-muted">
                          {t("createDialog.form.listItemTypeLabel")}
                        </span>
                        <select
                          class={columnAuxInputClass}
                          aria-label={t("createDialog.form.listItemTypeLabel")}
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
                          <input
                            type="text"
                            list={`list-row-ref-targets-edit-${i}`}
                            aria-label={t("createDialog.form.targetFormLabel")}
                            placeholder={t(
                              "createDialog.form.targetFormPlaceholder",
                            )}
                            value={field().itemsTargetForm || ""}
                            onInput={(event) =>
                              updateField(
                                i,
                                "itemsTargetForm",
                                event.currentTarget.value,
                              )}
                            class={columnAuxInputClass}
                          />
                          <datalist id={`list-row-ref-targets-edit-${i}`}>
                            <For each={targetFormOptions()}>
                              {(option) => <option value={option} />}
                            </For>
                          </datalist>
                        </div>
                      </Show>
                    </Show>
                    <Show when={fieldIssues().has(i) && field().isNew}>
                      <span class="text-xs ui-text-danger">
                        {fieldIssues().get(i)}
                      </span>
                    </Show>
                    <Show
                      when={!props.entryForm.fields[field().name] ||
                        field().isNew}
                    >
                      <div class={columnAuxRowClass}>
                        <span class="text-xs ui-muted">
                          {t("createDialog.form.defaultValueLabel")}
                        </span>
                        <input
                          type="text"
                          placeholder={t(
                            "createDialog.form.defaultValuePlaceholder",
                          )}
                          value={field().defaultValue || ""}
                          onInput={(e) =>
                            updateField(
                              i,
                              "defaultValue",
                              e.currentTarget.value,
                            )}
                          class={columnAuxInputClass}
                        />
                      </div>
                    </Show>
                  </div>
                )}
              </Index>
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
