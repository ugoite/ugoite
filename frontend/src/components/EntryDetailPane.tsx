import { A } from "@solidjs/router";
import {
  createEffect,
  createMemo,
  createSignal,
  For,
  Show,
} from "solid-js";
import type { Accessor } from "solid-js";

import { AccessPolicyEditor } from "~/components/AccessPolicyEditor";
import { locale, t } from "~/lib/i18n";
import { createResource } from "~/lib/recoverable-resource";
import {
  parseMarkdownH2Sections,
  renderMarkdownPreview,
  replaceFirstH1,
  updateH2Section,
} from "~/lib/markdown";
import { buildEntryMarkdownFromFields } from "~/lib/entry-input";
import {
  entryApi,
  RevisionConflictError,
  searchApi,
} from "~/lib/ugoite-client";
import type { Entry, Form, FormField } from "~/lib/types";

export interface EntryDetailPaneProps {
  spaceId: Accessor<string>;
  entryId?: Accessor<string>;
  forms?: Accessor<Form[]>;
  /** A form turns the pane into the new-entry editor without creating a server record first. */
  createForm?: Accessor<Form | undefined>;
  onCreateFormChange?: (formName: string) => void;
  onDeleted: () => void;
  onCancel?: () => void;
  onCreated?: (entryId: string) => void;
  onAfterSave?: () => void;
}

type EntryViewMode = "fields" | "preview" | "source";

type RowReferenceOption = {
  id: string;
  title: string;
};

const CLASS_VALIDATION_MARKER = "Form validation failed:";
const UNKNOWN_FIELDS_MARKER = "Unknown form fields:";
const BOOLEAN_VALUE_REGEX = /^(true|false|yes|no|on|off|1|0)$/i;
const NUMERIC_FIELD_TYPES = new Set([
  "integer",
  "long",
  "number",
  "double",
  "float",
]);
const ROW_REFERENCE_SUGGESTION_LIMIT = 8;

function parseFormValidationError(message: string) {
  if (!message.includes(CLASS_VALIDATION_MARKER)) return null;
  const payload = message.split(CLASS_VALIDATION_MARKER)[1]?.trim();
  /* v8 ignore start */
  if (!payload) return null;
  /* v8 ignore stop */
  try {
    const parsed = JSON.parse(payload) as Array<{
      field?: string;
      message?: string;
    }>;
    const items = parsed
      /* v8 ignore start */
      .map((item) => item.message || item.field)
      /* v8 ignore stop */
      /* v8 ignore start */
      .filter((item): item is string => Boolean(item));
    /* v8 ignore stop */
    return {
      title: "Form validation failed",
      items: items.length > 0
        ? items
        : ["Please review the form requirements."],
    };
  } catch {
    return {
      title: "Form validation failed",
      items: [payload],
    };
  }
}

function parseUnknownFieldsError(message: string) {
  if (!message.includes(UNKNOWN_FIELDS_MARKER)) return null;
  const payload = message.split(UNKNOWN_FIELDS_MARKER)[1]?.trim();
  /* v8 ignore start */
  const items = payload
    ? payload
      .split(",")
      .map((item) => item.trim())
      .filter(Boolean)
    : [];
  return {
    title: "Unknown form fields",
    items: items.length > 0 ? items : [payload || "Unknown fields found."],
  };
  /* v8 ignore stop */
}

function parseValidationErrorMessage(message: string) {
  return parseFormValidationError(message) || parseUnknownFieldsError(message);
}

function normalizeFieldName(fieldName: string) {
  return fieldName.trim().toLowerCase();
}

function readMarkdownTitle(markdown: string, fallback = "") {
  const heading = markdown.split(/\r?\n/).find((line) => /^#\s+/.test(line));
  if (!heading) return fallback;
  return heading.replace(/^#\s+/, "");
}

function buildEditorGuidance(form: Form | null, markdown: string) {
  if (!form) {
    return {
      missingRequired: [] as string[],
      unknownSections: [] as string[],
      typeIssues: [] as string[],
    };
  }

  const sections = parseMarkdownH2Sections(markdown);
  const sectionMap = new Map<string, { title: string; content: string }>();
  for (const section of sections) {
    sectionMap.set(normalizeFieldName(section.title), section);
  }

  /* v8 ignore start */
  const formFields = Object.entries(form.fields || {});
  /* v8 ignore stop */
  const knownFieldNames = new Set(
    formFields.map(([fieldName]) => normalizeFieldName(fieldName)),
  );

  const missingRequired = formFields
    .filter(([fieldName, fieldDef]) => {
      if (!fieldDef.required) return false;
      const section = sectionMap.get(normalizeFieldName(fieldName));
      return !section || !section.content.trim();
    })
    .map(([fieldName]) => fieldName);

  const unknownSections = sections
    .filter(
      (section) => !knownFieldNames.has(normalizeFieldName(section.title)),
    )
    /* v8 ignore start */
    .map((section) => section.title);
  /* v8 ignore stop */

  const typeIssues: string[] = [];
  for (const [fieldName, fieldDef] of formFields) {
    const section = sectionMap.get(normalizeFieldName(fieldName));
    if (!section) continue;
    const value = section.content.trim();
    if (!value) continue;
    /* v8 ignore start */
    if (fieldDef.type === "boolean" && !BOOLEAN_VALUE_REGEX.test(value)) {
      typeIssues.push(`${fieldName}: ${t("entryGuidance.booleanValue")}`);
    }
    if (
      fieldDef.type === "list" &&
      !value.includes("\n") &&
      !value.startsWith("-") &&
      value.includes(",")
    ) {
      typeIssues.push(`${fieldName}: ${t("entryGuidance.listValue")}`);
    }
    /* v8 ignore stop */
  }

  return { missingRequired, unknownSections, typeIssues };
}

function resolveInputType(field: FormField) {
  if (NUMERIC_FIELD_TYPES.has(field.type)) return "number";
  if (field.type === "date") return "date";
  if (field.type === "time") return "time";
  if (field.type === "timestamp") return "datetime-local";
  return "text";
}

function resolveInputValue(field: FormField, value: string) {
  if (field.type !== "timestamp") return value;
  const match =
    /^(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}(?::\d{2}(?:\.\d+)?)?)(?:Z|[+-]\d{2}:\d{2})$/
      .exec(value);
  return match?.[1] ?? value;
}

function createFieldInputId(fieldName: string, index: number) {
  const normalized = fieldName
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9_-]+/g, "-")
    .replace(/^-+|-+$/g, "");
  return `entry-field-${index}-${normalized || "field"}`;
}

function formatEntryDate(value: string | undefined) {
  if (!value) return "—";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return new Intl.DateTimeFormat(locale() === "ja" ? "ja-JP" : "en", {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(date);
}

async function fetchWithTimeout<T>(
  promise: Promise<T>,
  ms = 10000,
  errorMsg = "Operation timed out",
): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const timeout = new Promise<never>((_, reject) => {
    /* v8 ignore start */
    timer = setTimeout(() => reject(new Error(errorMsg)), ms);
    /* v8 ignore stop */
  });
  try {
    return await Promise.race([promise, timeout]);
  } finally {
    /* v8 ignore start */
    if (timer) clearTimeout(timer);
    /* v8 ignore stop */
  }
}

function EntryRowReferenceField(props: {
  spaceId: string;
  fieldId: string;
  targetForm: string;
  value: string;
  onChange: (value: string) => void;
}) {
  const [query, setQuery] = createSignal(props.value);
  const [selected, setSelected] = createSignal<RowReferenceOption | null>(
    props.value ? { id: props.value, title: props.value } : null,
  );
  const [lastPropValue, setLastPropValue] = createSignal(props.value);

  createEffect(() => {
    const value = props.value;
    if (value === lastPropValue()) return;
    setLastPropValue(value);
    setQuery(value);
    setSelected(value ? { id: value, title: value } : null);
  });

  const [options] = createResource(
    () => ({
      spaceId: props.spaceId.trim(),
      targetForm: props.targetForm.trim(),
      query: query(),
    }),
    async ({ spaceId, targetForm, query: searchQuery }) => {
      if (!spaceId || !targetForm) return [] as RowReferenceOption[];
      const entries = await searchApi.rowReferenceOptions(
        spaceId,
        targetForm,
        searchQuery,
        ROW_REFERENCE_SUGGESTION_LIMIT,
      );
      return entries
        .map((item) => ({
          id: item.id,
          title: item.title?.trim() || item.id,
        }))
        .sort(
          (left, right) =>
            left.title.localeCompare(right.title) ||
            left.id.localeCompare(right.id),
        );
    },
    { initialValue: [] as RowReferenceOption[] },
  );

  createEffect(() => {
    const selectedValue = selected();
    if (!selectedValue) return;
    const match = options().find((option) => option.id === selectedValue.id);
    if (match && match.title !== selectedValue.title) setSelected(match);
  });

  const handleQueryInput = (value: string) => {
    setQuery(value);
    setSelected(null);
    if (props.value) {
      setLastPropValue("");
      props.onChange("");
    }
  };

  const handleSelect = (option: RowReferenceOption) => {
    setSelected(option);
    setQuery(option.title);
    setLastPropValue(option.id);
    props.onChange(option.id);
  };

  const handleClear = () => {
    setSelected(null);
    setQuery("");
    setLastPropValue("");
    props.onChange("");
  };

  return (
    <div class="ui-stack-sm">
      <input
        id={props.fieldId}
        type="search"
        class="ui-input"
        value={query()}
        placeholder={t("createDialog.entry.rowReference.searchPlaceholder", {
          form: props.targetForm,
        })}
        onInput={(event) => handleQueryInput(event.currentTarget.value)}
        autocomplete="off"
      />
      <p class="text-xs ui-muted">
        {t("createDialog.entry.rowReference.help", { form: props.targetForm })}
      </p>
      <Show when={selected()}>
        {(option) => (
          <div class="ui-reference-picker-selection">
            <p class="text-[11px] font-semibold uppercase tracking-wide ui-muted">
              {t("createDialog.entry.rowReference.selected")}
            </p>
            <div class="flex flex-wrap items-center justify-between gap-3">
              <div class="min-w-0">
                <p class="truncate text-sm font-medium">{option().title}</p>
                <p class="truncate text-xs ui-muted">{option().id}</p>
              </div>
              <button
                type="button"
                class="ui-button ui-button-secondary ui-button-sm text-xs"
                onClick={handleClear}
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
                  onClick={() => handleSelect(option)}
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
        when={!options.loading &&
          !options.error &&
          query().trim() &&
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

export function EntryDetailPane(props: EntryDetailPaneProps) {
  const [editorContent, setEditorContent] = createSignal("");
  const [lastSavedContent, setLastSavedContent] = createSignal("");
  const [isDirty, setIsDirty] = createSignal(false);
  const [isSaving, setIsSaving] = createSignal(false);
  const [viewMode, setViewMode] = createSignal<EntryViewMode>("source");
  const [conflictMessage, setConflictMessage] = createSignal<string | null>(
    null,
  );
  const [validationError, setValidationError] = createSignal<
    {
      title: string;
      items: string[];
    } | null
  >(null);
  const [currentRevisionId, setCurrentRevisionId] = createSignal<string | null>(
    null,
  );
  const [lastLoadedEntryId, setLastLoadedEntryId] = createSignal<string | null>(
    null,
  );
  const [lastLoadedResourceRevisionId, setLastLoadedResourceRevisionId] =
    createSignal<string | null>(null);
  const [defaultedViewEntryId, setDefaultedViewEntryId] = createSignal<
    string | null
  >(null);
  const [entryError, setEntryError] = createSignal<string | null>(null);
  const [showAccessPolicy, setShowAccessPolicy] = createSignal(false);

  const [remoteEntry, { refetch: refetchEntry }] = createResource(
    () => {
      const wsId = props.spaceId();
      const entryId = props.entryId?.() ?? "";
      /* v8 ignore start */
      return wsId && entryId ? { wsId, entryId } : null;
      /* v8 ignore stop */
    },
    async (parameters) => {
      /* v8 ignore start */
      if (!parameters) return null;
      /* v8 ignore stop */
      try {
        setEntryError(null);
        return await fetchWithTimeout(
          entryApi.get(parameters.wsId, parameters.entryId),
          45_000,
          "Loading entry timed out",
        );
      } catch (error) {
        /* v8 ignore start */
        setEntryError(
          error instanceof Error ? error.message : "Failed to load entry",
        );
        /* v8 ignore stop */
        return null;
      }
    },
  );

  const isCreateMode = createMemo(() => Boolean(props.createForm?.()));
  const draftEntry = createMemo<Entry | null>(() => {
    const form = props.createForm?.();
    if (!form) return null;
    return {
      id: "__new__",
      title: form.name,
      form: form.name,
      content: buildEntryMarkdownFromFields(form, form.name, {}),
      revision_id: `draft:${form.name}`,
      created_at: "",
      updated_at: "",
    };
  });
  const entry = createMemo(() => isCreateMode() ? draftEntry() : remoteEntry());
  const entryLoading = createMemo(() =>
    isCreateMode() ? false : remoteEntry.loading
  );

  const currentForm = createMemo(() => {
    const formName = entry()?.form?.trim();
    if (!formName) return null;
    const availableForms = props.forms?.() ?? [];
    return (
      availableForms.find((candidate) => candidate.name === formName) ?? null
    );
  });
  const formWorkspaceHref = createMemo(() => {
    const formName = entry()?.form?.trim();
    const base = `/spaces/${props.spaceId()}/forms`;
    return formName ? `${base}?form=${encodeURIComponent(formName)}` : base;
  });

  const parsedSections = createMemo(() => {
    const map = new Map<string, string>();
    for (const section of parseMarkdownH2Sections(editorContent())) {
      map.set(normalizeFieldName(section.title), section.content);
    }
    return map;
  });

  const editorTitle = createMemo(() =>
    readMarkdownTitle(editorContent(), entry()?.title || "")
  );
  const editorGuidance = createMemo(() =>
    buildEditorGuidance(currentForm(), editorContent())
  );

  const fieldValue = (fieldName: string) =>
    parsedSections().get(normalizeFieldName(fieldName)) ?? "";

  const fieldIssue = (fieldName: string) =>
    editorGuidance().typeIssues.find((issue) =>
      issue.startsWith(`${fieldName}:`)
    );

  createEffect(() => {
    const loadedEntry = entry();
    if (!loadedEntry) return;
    if (
      loadedEntry.id === lastLoadedEntryId() &&
      loadedEntry.revision_id === lastLoadedResourceRevisionId()
    ) {
      return;
    }
    const content = loadedEntry.content ?? "";
    setLastLoadedEntryId(loadedEntry.id);
    setLastLoadedResourceRevisionId(loadedEntry.revision_id);
    setCurrentRevisionId(isCreateMode() ? null : loadedEntry.revision_id);
    setEditorContent(content);
    setLastSavedContent(isCreateMode() ? "" : content);
    setIsDirty(isCreateMode());
    setConflictMessage(null);
    setValidationError(null);
    setDefaultedViewEntryId(null);
  });

  createEffect(() => {
    const loadedEntry = entry();
    if (!loadedEntry || defaultedViewEntryId() === loadedEntry.id) return;
    if (loadedEntry.form && props.forms && props.forms().length === 0) return;
    setViewMode(currentForm() ? "fields" : "source");
    setDefaultedViewEntryId(loadedEntry.id);
  });

  const handleContentChange = (content: string) => {
    setEditorContent(content);
    setIsDirty(content !== lastSavedContent());
    setConflictMessage(null);
    setValidationError(null);
  };

  const handleTitleChange = (title: string) => {
    handleContentChange(replaceFirstH1(editorContent(), title));
  };

  const handleFieldChange = (fieldName: string, value: string) => {
    handleContentChange(updateH2Section(editorContent(), fieldName, value));
  };

  const handleEditorKeyDown = (event: KeyboardEvent) => {
    if ((event.metaKey || event.ctrlKey) && event.key === "s") {
      event.preventDefault();
      if (isDirty() && !isSaving()) void handleSave();
    }
  };

  type SaveContext =
    | {
      ok: true;
      wsId: string;
      create: boolean;
      entryId?: string;
      revisionId?: string;
    }
    | { ok: false; reason: string };

  const resolveSaveContext = (): SaveContext => {
    const wsId = props.spaceId();
    const entryId = props.entryId?.() ?? "";
    if (isCreateMode()) {
      if (!wsId) {
        return { ok: false, reason: "Cannot save: Space is not selected." };
      }
      return { ok: true, wsId, create: true };
    }
    /* v8 ignore start */
    const revisionId = currentRevisionId() || entry()?.revision_id;
    if (!wsId || !entryId || !revisionId) {
      return {
        ok: false,
        reason:
          "Cannot save: entry not properly loaded. Please try refreshing.",
      };
    }
    /* v8 ignore stop */
    return { ok: true, wsId, entryId, revisionId, create: false };
  };

  const handleSaveError = (error: unknown) => {
    /* v8 ignore start */
    if (error instanceof RevisionConflictError) {
      setConflictMessage(
        "This entry was modified elsewhere. Your draft is still in the editor; refresh to load the latest version.",
      );
      return;
    }
    const message = error instanceof Error ? error.message : "Failed to save";
    /* v8 ignore stop */
    const parsed = parseValidationErrorMessage(message);
    if (parsed) setValidationError(parsed);
    else setConflictMessage(message);
  };

  const handleSave = async () => {
    const context = resolveSaveContext();
    /* v8 ignore start */
    if (!context.ok) {
      setConflictMessage(context.reason);
      return;
    }
    /* v8 ignore stop */

    setIsSaving(true);
    setConflictMessage(null);
    setValidationError(null);
    const contentToSave = editorContent();
    try {
      const result = context.create
        ? await entryApi.create(context.wsId, { markdown: contentToSave })
        : await entryApi.update(context.wsId, context.entryId!, {
          markdown: contentToSave,
          parent_revision_id: context.revisionId!,
        });
      setCurrentRevisionId(result.revision_id);
      setLastSavedContent(contentToSave);
      setIsDirty(editorContent() !== contentToSave);
      props.onAfterSave?.();
      if (context.create) props.onCreated?.(result.id);
    } catch (error) {
      handleSaveError(error);
    } finally {
      setIsSaving(false);
    }
  };

  const handleDiscard = () => {
    /* v8 ignore start */
    if (isDirty() && !confirm(t("entryDetail.confirmDiscard"))) return;
    /* v8 ignore stop */
    setEditorContent(lastSavedContent());
    setIsDirty(false);
    setConflictMessage(null);
    setValidationError(null);
  };

  const handleRefresh = async () => {
    /* v8 ignore start */
    if (isDirty() && !confirm(t("entryDetail.confirmRefresh"))) return;
    /* v8 ignore stop */
    setLastLoadedEntryId(null);
    setLastLoadedResourceRevisionId(null);
    await refetchEntry();
  };

  const handleDelete = async () => {
    const wsId = props.spaceId();
    const entryId = props.entryId?.() ?? "";
    /* v8 ignore start */
    if (!wsId || !entryId) return;
    if (!confirm(t("entryDetail.confirmDelete"))) return;
    /* v8 ignore stop */

    try {
      await entryApi.delete(wsId, entryId);
      props.onDeleted();
    } catch (error) {
      /* v8 ignore start */
      alert(error instanceof Error ? error.message : "Failed to delete entry");
      /* v8 ignore stop */
    }
  };

  const handleCancel = () => {
    /* v8 ignore start */
    if (isDirty() && !confirm(t("entryDetail.confirmDiscard"))) return;
    /* v8 ignore stop */
    (props.onCancel ?? props.onDeleted)();
  };

  const renderFieldControl = (
    fieldName: string,
    fieldDef: FormField,
    fieldId: string,
  ) => {
    const value = () => fieldValue(fieldName);

    if (fieldDef.type === "boolean") {
      const currentValue = () => value().trim();
      const isCanonical = () =>
        !currentValue() || ["true", "false"].includes(currentValue());
      return (
        <select
          id={fieldId}
          class="ui-input"
          value={currentValue()}
          onChange={(event) =>
            handleFieldChange(fieldName, event.currentTarget.value)}
        >
          <Show when={!isCanonical()}>
            <option value={currentValue()}>{currentValue()}</option>
          </Show>
          <option value="">{t("entryDetail.boolean.unset")}</option>
          <option value="true">{t("entryDetail.boolean.true")}</option>
          <option value="false">{t("entryDetail.boolean.false")}</option>
        </select>
      );
    }

    if (fieldDef.type === "row_reference" && fieldDef.target_form?.trim()) {
      return (
        <EntryRowReferenceField
          spaceId={props.spaceId()}
          fieldId={fieldId}
          targetForm={fieldDef.target_form.trim()}
          value={value()}
          onChange={(nextValue) => handleFieldChange(fieldName, nextValue)}
        />
      );
    }

    if (
      fieldDef.type === "markdown" ||
      fieldDef.type === "list" ||
      fieldDef.type === "object_list"
    ) {
      return (
        <textarea
          id={fieldId}
          class="ui-input ui-textarea"
          value={value()}
          placeholder={fieldDef.type === "list"
            ? t("entryDetail.listPlaceholder")
            : t("entryDetail.fieldPlaceholder")}
          onInput={(event) =>
            handleFieldChange(fieldName, event.currentTarget.value)}
        />
      );
    }

    return (
      <input
        id={fieldId}
        class="ui-input"
        type={resolveInputType(fieldDef)}
        value={resolveInputValue(fieldDef, value())}
        step={fieldDef.type === "timestamp" ? "any" : undefined}
        placeholder={t("entryDetail.fieldPlaceholder")}
        onInput={(event) =>
          handleFieldChange(fieldName, event.currentTarget.value)}
      />
    );
  };

  /* v8 ignore start */
  return (
    <div class="ui-entry-page">
      <Show when={entryLoading()}>
        <div class="absolute inset-0 ui-backdrop z-50 flex items-center justify-center">
          <div class="ui-card text-center">
            <div class="animate-spin rounded-full h-8 w-8 border-b-2 border-current mx-auto mb-2" />
            <p class="ui-muted text-sm">{t("entryDetail.loading")}</p>
          </div>
        </div>
      </Show>

      <Show
        when={entry()}
        fallback={
          <div class="ui-entry-empty-state">
            <Show
              when={entryError()}
              fallback={
                <Show when={!entryLoading()} fallback={<div />}>
                  <p class="ui-muted">{t("entryDetail.notFound")}</p>
                </Show>
              }
            >
              <div class="text-center space-y-3">
                <p class="ui-alert ui-alert-error text-sm">{entryError()}</p>
                <p class="text-xs ui-muted">
                  Space: {props.spaceId()} / Entry: {props.entryId?.() ?? ""}
                </p>
                <div class="flex justify-center gap-2">
                  <button
                    type="button"
                    onClick={() => refetchEntry()}
                    class="ui-button ui-button-secondary text-sm"
                  >
                    {t("entryDetail.retry")}
                  </button>
                  <button
                    type="button"
                    class="ui-button ui-button-secondary text-sm"
                    onClick={props.onDeleted}
                  >
                    {t("entryDetail.back")}
                  </button>
                </div>
              </div>
            </Show>
          </div>
        }
      >
        {(currentEntry) => (
          <>
            <header class="ui-entry-header">
              <div class="min-w-0">
                <A
                  href={formWorkspaceHref()}
                  class="text-sm ui-link"
                >
                  {t("entryDetail.back")}
                </A>
                <div class="mt-2 flex flex-wrap items-center gap-2">
                  <h1 class="ui-page-title truncate">
                    {editorTitle() || t("common.untitled")}
                  </h1>
                  <Show when={currentEntry().form}>
                    <span class="ui-pill">{currentEntry().form}</span>
                  </Show>
                  <Show
                    when={isCreateMode() && props.forms &&
                      props.onCreateFormChange}
                  >
                    <label class="sr-only" for="entry-form-selector">
                      {t("common.form")}
                    </label>
                    <select
                      id="entry-form-selector"
                      class="ui-input ui-input-sm"
                      value={currentEntry().form || ""}
                      onChange={(event) =>
                        props.onCreateFormChange?.(event.currentTarget.value)}
                    >
                      <For each={props.forms?.() ?? []}>
                        {(form) => (
                          <option value={form.name}>{form.name}</option>
                        )}
                      </For>
                    </select>
                  </Show>
                </div>
                <p class="ui-page-subtitle mt-1">
                  {currentForm()
                    ? t("entryDetail.formFirstDescription")
                    : t("entryDetail.documentDescription")}
                </p>
              </div>
              <div class="ui-entry-save-area">
                <span
                  class="text-sm"
                  classList={{
                    "ui-warning": isDirty(),
                    "ui-muted": !isDirty(),
                  }}
                >
                  {isDirty()
                    ? t("entryDetail.unsaved")
                    : t("entryDetail.saved")}
                </span>
                <button
                  type="button"
                  class="ui-button ui-button-primary"
                  onClick={() => void handleSave()}
                  disabled={!isDirty() || isSaving()}
                  aria-label={t("entryDetail.save")}
                >
                  {isSaving() ? t("entryDetail.saving") : t("entryDetail.save")}
                </button>
              </div>
            </header>

            <Show when={validationError()}>
              {(error) => (
                <div class="ui-alert ui-alert-warning text-sm">
                  <p class="font-semibold">{error().title}</p>
                  <ul class="mt-2 list-disc pl-5 space-y-1">
                    <For each={error().items}>{(item) => <li>{item}</li>}</For>
                  </ul>
                </div>
              )}
            </Show>

            <Show when={conflictMessage()}>
              <div class="ui-alert ui-alert-error text-sm">
                {conflictMessage()}
              </div>
            </Show>

            <div class="ui-entry-workspace">
              <main class="ui-entry-main ui-card">
                <div class="ui-entry-main-header">
                  <div>
                    <h2 class="text-lg font-semibold">
                      {viewMode() === "fields"
                        ? t("entryDetail.mode.fields")
                        : viewMode() === "preview"
                        ? t("entryDetail.mode.preview")
                        : t("entryDetail.mode.source")}
                    </h2>
                    <p class="mt-1 text-sm ui-muted">
                      {viewMode() === "fields"
                        ? t("entryDetail.fieldsDescription")
                        : viewMode() === "preview"
                        ? t("entryDetail.previewDescription")
                        : t("entryDetail.sourceDescription")}
                    </p>
                  </div>
                  <div class="ui-entry-mode-tabs" role="tablist">
                    <Show when={currentForm()}>
                      <button
                        type="button"
                        role="tab"
                        aria-selected={viewMode() === "fields"}
                        class="ui-entry-mode-tab"
                        classList={{
                          "ui-entry-mode-tab-active": viewMode() === "fields",
                        }}
                        onClick={() => setViewMode("fields")}
                      >
                        {t("entryDetail.mode.fields")}
                      </button>
                    </Show>
                    <button
                      type="button"
                      role="tab"
                      aria-selected={viewMode() === "preview"}
                      class="ui-entry-mode-tab"
                      classList={{
                        "ui-entry-mode-tab-active": viewMode() === "preview",
                      }}
                      onClick={() => setViewMode("preview")}
                    >
                      {t("entryDetail.mode.preview")}
                    </button>
                    <button
                      type="button"
                      role="tab"
                      aria-selected={viewMode() === "source"}
                      class="ui-entry-mode-tab"
                      classList={{
                        "ui-entry-mode-tab-active": viewMode() === "source",
                      }}
                      onClick={() => setViewMode("source")}
                    >
                      {t("entryDetail.mode.source")}
                    </button>
                  </div>
                </div>

                <Show when={viewMode() === "fields" && currentForm()}>
                  {(entryForm) => (
                    <div class="ui-entry-form-body">
                      <div class="ui-entry-field ui-entry-title-field">
                        <div class="ui-entry-field-heading">
                          <label class="ui-label" for="entry-title-editor">
                            {t("common.title")}
                          </label>
                          <span class="ui-entry-required">
                            {t("entryDetail.required")}
                          </span>
                        </div>
                        <input
                          id="entry-title-editor"
                          class="ui-input ui-entry-title-input"
                          value={editorTitle()}
                          placeholder={t("common.untitled")}
                          onInput={(event) =>
                            handleTitleChange(event.currentTarget.value)}
                        />
                      </div>

                      <Show
                        when={Object.keys(entryForm().fields || {}).length > 0}
                        fallback={
                          <div class="ui-entry-no-fields">
                            <p class="font-medium">
                              {t("entryDetail.noFields")}
                            </p>
                            <p class="mt-1 text-sm ui-muted">
                              {t("entryDetail.noFieldsDescription")}
                            </p>
                            <button
                              type="button"
                              class="ui-button ui-button-secondary mt-4"
                              onClick={() => setViewMode("source")}
                            >
                              {t("entryDetail.openSource")}
                            </button>
                          </div>
                        }
                      >
                        <div class="ui-entry-field-list">
                          <For each={Object.entries(entryForm().fields || {})}>
                            {([fieldName, fieldDef], index) => {
                              const fieldId = createFieldInputId(
                                fieldName,
                                index(),
                              );
                              const isMissing = () =>
                                editorGuidance().missingRequired.includes(
                                  fieldName,
                                );
                              return (
                                <div
                                  class="ui-entry-field"
                                  classList={{
                                    "ui-entry-field-error": isMissing(),
                                  }}
                                >
                                  <div class="ui-entry-field-heading">
                                    <div class="min-w-0">
                                      <label class="ui-label" for={fieldId}>
                                        {fieldName}
                                      </label>
                                      <p class="mt-0.5 text-xs ui-muted">
                                        {fieldDef.type}
                                        <Show when={fieldDef.target_form}>
                                          {` · ${fieldDef.target_form}`}
                                        </Show>
                                      </p>
                                    </div>
                                    <span
                                      class={fieldDef.required
                                        ? "ui-entry-required"
                                        : "ui-pill"}
                                    >
                                      {fieldDef.required
                                        ? t("entryDetail.required")
                                        : t("entryDetail.optional")}
                                    </span>
                                  </div>
                                  {renderFieldControl(
                                    fieldName,
                                    fieldDef,
                                    fieldId,
                                  )}
                                  <Show when={isMissing()}>
                                    <p class="text-xs ui-text-danger">
                                      {t("entryDetail.requiredMessage")}
                                    </p>
                                  </Show>
                                  <Show when={fieldIssue(fieldName)}>
                                    {(issue) => (
                                      <p class="text-xs ui-text-danger">
                                        {issue()}
                                      </p>
                                    )}
                                  </Show>
                                </div>
                              );
                            }}
                          </For>
                        </div>
                      </Show>

                      <Show when={editorGuidance().unknownSections.length > 0}>
                        <div class="ui-entry-advanced-note">
                          <div>
                            <p class="text-sm font-medium">
                              {t("entryDetail.additionalContent")}
                            </p>
                            <p class="mt-1 text-xs ui-muted">
                              {editorGuidance().unknownSections.join(", ")}
                            </p>
                          </div>
                          <button
                            type="button"
                            class="ui-button ui-button-secondary ui-button-sm text-xs"
                            onClick={() => setViewMode("source")}
                          >
                            {t("entryDetail.reviewSource")}
                          </button>
                        </div>
                      </Show>
                    </div>
                  )}
                </Show>

                <Show when={viewMode() === "preview"}>
                  <div
                    class="ui-preview ui-entry-preview"
                    innerHTML={renderMarkdownPreview(editorContent())}
                  />
                </Show>

                <Show when={viewMode() === "source"}>
                  <div class="ui-entry-source-body">
                    <textarea
                      class="ui-editor ui-entry-source-editor"
                      value={editorContent()}
                      onInput={(event) =>
                        handleContentChange(event.currentTarget.value)}
                      onKeyDown={handleEditorKeyDown}
                      placeholder={t("entryDetail.sourcePlaceholder")}
                      spellcheck={false}
                    />
                    <div class="ui-entry-source-footer">
                      <p class="text-xs ui-muted">
                        {t("entryDetail.sourceHelp")}
                      </p>
                    </div>
                  </div>
                </Show>
              </main>

              <aside class="ui-entry-sidebar">
                <section class="ui-card ui-entry-side-card">
                  <h2 class="ui-entry-side-heading">
                    {t("entryDetail.detailsHeading")}
                  </h2>
                  <dl class="ui-entry-detail-list">
                    <Show when={currentEntry().form}>
                      <div>
                        <dt>{t("common.form")}</dt>
                        <dd>
                          <A
                            href={`/spaces/${props.spaceId()}/forms/${
                              encodeURIComponent(
                                currentEntry().form || "",
                              )
                            }`}
                            class="ui-link"
                          >
                            {currentEntry().form}
                          </A>
                        </dd>
                      </div>
                    </Show>
                    <Show when={!isCreateMode()}>
                      <div>
                        <dt>{t("common.updated")}</dt>
                        <dd>{formatEntryDate(currentEntry().updated_at)}</dd>
                      </div>
                    </Show>
                    <Show when={!isCreateMode()}>
                      <div>
                        <dt>{t("entryDetail.entryId")}</dt>
                        <dd class="font-mono break-all">{currentEntry().id}</dd>
                      </div>
                    </Show>
                  </dl>
                </section>

                <section class="ui-card ui-entry-side-card">
                  <h2 class="ui-entry-side-heading">
                    {t("entryDetail.actionsHeading")}
                  </h2>
                  <div class="ui-entry-action-list">
                    <Show when={!isCreateMode()}>
                      <A
                        href={`/spaces/${props.spaceId()}/entries/${
                          encodeURIComponent(props.entryId?.() ?? "")
                        }/history`}
                        class="ui-entry-action"
                      >
                        <span>{t("entryDetail.history")}</span>
                        <span aria-hidden="true">›</span>
                      </A>
                      <button
                        type="button"
                        class="ui-entry-action"
                        onClick={() => void handleRefresh()}
                      >
                        <span>{t("entryDetail.refresh")}</span>
                        <span aria-hidden="true">↻</span>
                      </button>
                    </Show>
                    <Show when={!isCreateMode()}>
                      <button
                        type="button"
                        class="ui-entry-action"
                        onClick={handleDiscard}
                        disabled={!isDirty()}
                      >
                        <span>{t("entryDetail.discard")}</span>
                        <span aria-hidden="true">×</span>
                      </button>
                    </Show>
                    <Show when={!isCreateMode()}>
                      <button
                        type="button"
                        class="ui-entry-action"
                        onClick={() => setShowAccessPolicy((value) => !value)}
                      >
                        <span>
                          {showAccessPolicy()
                            ? t("entryDetail.closeSharing")
                            : t("entryDetail.sharing")}
                        </span>
                        <span aria-hidden="true">›</span>
                      </button>
                    </Show>
                    <Show when={isCreateMode()}>
                      <button
                        type="button"
                        class="ui-entry-action"
                        onClick={handleCancel}
                      >
                        <span>{t("entryDetail.back")}</span>
                        <span aria-hidden="true">×</span>
                      </button>
                    </Show>
                  </div>
                  <Show when={!isCreateMode()}>
                    <div class="ui-entry-danger-zone">
                      <button
                        type="button"
                        onClick={handleDelete}
                        class="ui-button ui-button-danger ui-button-sm w-full"
                      >
                        {t("entryDetail.delete")}
                      </button>
                    </div>
                  </Show>
                </section>
              </aside>
            </div>

            <Show when={showAccessPolicy()}>
              <div class="mt-4">
                <AccessPolicyEditor
                  spaceId={props.spaceId()}
                  kind="entry"
                  resourceId={props.entryId?.() ?? ""}
                />
              </div>
            </Show>
          </>
        )}
      </Show>
    </div>
  );
  /* v8 ignore stop */
}
