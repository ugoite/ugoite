import { useBeforeLeave } from "@solidjs/router";
import {
  createEffect,
  createMemo,
  createSignal,
  For,
  onCleanup,
  Show,
} from "solid-js";
import type { Accessor } from "solid-js";

import { AssetField } from "~/components/AssetField";
import { ActionIconBar } from "~/components/ActionIconBar";
import { BackLink } from "~/components/BackLink";
import { ListEditor } from "~/components/ListEditor";
import {
  createEntryFieldInputId,
  type EntryFieldDescriptor,
  EntryFields,
} from "~/components/EntryFields";
import { LocalBusyIndicator } from "~/components/LocalBusyIndicator";
import {
  type AssetFieldState,
  createAssetFieldState,
} from "~/lib/asset-field-state";
import { t } from "~/lib/i18n";
import { createResource } from "~/lib/recoverable-resource";
import { parseMarkdownH2Sections } from "~/lib/markdown";
import {
  buildEntryMarkdownFromFields,
  parseEntryMarkdownPresentation,
  readEntryTagsPresentation,
  updateEntryMarkdownPresentation,
} from "~/lib/entry-input";
import {
  type DraftFields,
  draftValueToDisplayString,
  isPlainStringListField,
  normalizeStringListValue,
  readAssetReferences,
  toTransportFields,
} from "~/lib/draft-values";
import {
  entryApi,
  RevisionConflictError,
  searchApi,
} from "~/lib/ugoite-client";
import { UgoiteApiError } from "~/lib/ugoite-client/protocol";
import { validateEntryDraftViaWasm } from "~/lib/entry-validation";
import {
  type CompatDraft,
  parseSourceToDraftViaWasm,
  renderDraftToSourceViaWasm,
} from "~/lib/entry-compat";
import type { Entry, Form, FormField } from "~/lib/types";
import type { MarkdownConversionDiagnostic } from "~/lib/ugoite-client/protocol";
import { isAssetReferenceListField } from "~/lib/asset-reference";
import {
  formatMarkdownConversionDiagnostic,
  formatUserFacingError,
} from "~/lib/user-facing-error";
import {
  clearCreateEntryDraftSession,
  createEntryDraftSessionKey,
  type CreateEntryDraftState,
  getCreateEntryDraftSession,
} from "~/lib/create-entry-draft-session";

export interface EntryDetailPaneProps {
  spaceId: Accessor<string>;
  entryId?: Accessor<string>;
  forms?: Accessor<Form[]>;
  /** A form turns the pane into the new-entry editor without creating a server record first. */
  createForm?: Accessor<Form | undefined>;
  onCreateFormChange?: (formName: string) => void;
  onDeleted: () => void;
  onCreated?: (result: { id: string; revision_id: string }) => void;
  onAfterSave?: () => void;
}

type RowReferenceOption = {
  id: string;
  title: string;
};

const BOOLEAN_VALUE_REGEX = /^(true|false|yes|no|on|off|1|0)$/i;
// Presentation hint only. The shared Rust boundary owns boolean coercion;
// this regex never decides saveability.
const NUMERIC_FIELD_TYPES = new Set([
  "integer",
  "long",
  "number",
  "double",
  "float",
]);
const ROW_REFERENCE_SUGGESTION_LIMIT = 8;

const isActiveRequiredField = (field: FormField) =>
  field.required && !field.deprecated;

function parseEntryValidationError(error: unknown) {
  if (!(error instanceof UgoiteApiError)) return null;
  const detail = error.detail && typeof error.detail === "object" &&
      !Array.isArray(error.detail)
    ? error.detail as Record<string, unknown>
    : null;
  if (error.code === "UNKNOWN_FORM_FIELDS") {
    const fields = Array.isArray(detail?.fields)
      ? detail.fields.filter((field): field is string =>
        typeof field === "string"
      )
      : [];
    return {
      title: t("entryDetail.unknownFormFields"),
      items: fields.length > 0 ? fields : [t("entryDetail.reviewRequirements")],
      fields,
    };
  }
  if (error.code === "FORM_VALIDATION_FAILED") {
    const warnings = Array.isArray(detail?.warnings) ? detail.warnings : [];
    const fields: string[] = [];
    const items = warnings
      .map((warning) => {
        if (!warning || typeof warning !== "object") return null;
        const item = warning as Record<string, unknown>;
        if (typeof item.field === "string" && item.field) {
          fields.push(item.field);
        }
        return typeof item.message === "string"
          ? item.message
          : typeof item.field === "string"
          ? item.field
          : null;
      })
      .filter((item): item is string => Boolean(item));
    return {
      title: t("entryDetail.validationFailed"),
      items: items.length > 0 ? items : [t("entryDetail.reviewRequirements")],
      fields,
    };
  }
  return null;
}

function normalizeFieldName(fieldName: string) {
  return fieldName.trim().toLowerCase();
}

function isMissingRequiredValue(fieldDef: FormField, content: string) {
  const value = content.trim();
  if (!value) return true;

  if (fieldDef.type === "object_list") {
    try {
      const parsed: unknown = JSON.parse(value);
      return Array.isArray(parsed) && parsed.length === 0;
    } catch {
      return false;
    }
  }

  if (fieldDef.type === "list" && !isAssetReferenceListField(fieldDef)) {
    const hasValue = value.split(/\r?\n/).some((line) => {
      const item = line.trim().replace(
        /^(?:[-*+](?:\s+\[[ xX]\])?\s*)/,
        "",
      );
      return item.length > 0;
    });
    return !hasValue;
  }

  if (isAssetReferenceListField(fieldDef)) {
    const result = readAssetReferences(value, true);
    return result.issue !== "invalid" && result.references.length === 0;
  }

  return false;
}

function buildEditorGuidance(form: Form | null, markdown: string) {
  // Presentation hints only. Saveability and canonical errors come from the
  // shared Rust boundary (`entry.validate_draft` + server mutation). When a
  // hint conflicts with Rust, the Rust result wins.
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
      if (!isActiveRequiredField(fieldDef)) return false;
      const section = sectionMap.get(normalizeFieldName(fieldName));
      return isMissingRequiredValue(fieldDef, section?.content ?? "");
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
    if (
      fieldDef.type === "asset_reference" &&
      readAssetReferences(value, false).issue === "invalid"
    ) {
      typeIssues.push(`${fieldName}: ${t("assetField.error.invalid")}`);
    }
    if (isAssetReferenceListField(fieldDef)) {
      const result = readAssetReferences(value, true);
      if (result.issue === "invalid") {
        typeIssues.push(`${fieldName}: ${t("assetField.error.invalid")}`);
      } else if (result.issue === "duplicate") {
        typeIssues.push(`${fieldName}: ${t("assetField.error.duplicate")}`);
      }
    }
    /* v8 ignore stop */
  }

  return { missingRequired, unknownSections, typeIssues };
}

function resolveInputMode(field: FormField): "decimal" | undefined {
  return NUMERIC_FIELD_TYPES.has(field.type) ? "decimal" : undefined;
}

function resolveInputType(field: FormField): "date" | "text" {
  return field.type === "date" ? "date" : "text";
}

class EntryLoadTimeoutError extends Error {
  constructor() {
    super("entry load timed out");
    this.name = "EntryLoadTimeoutError";
  }
}

async function fetchWithTimeout<T>(
  promise: Promise<T>,
  ms = 10000,
  error: Error = new EntryLoadTimeoutError(),
): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const timeout = new Promise<never>((_, reject) => {
    /* v8 ignore start */
    timer = setTimeout(() => reject(error), ms);
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
  invalid?: boolean;
  describedBy?: string;
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
        aria-invalid={props.invalid ? "true" : undefined}
        aria-describedby={props.describedBy}
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
        <LocalBusyIndicator
          label={t("createDialog.entry.rowReference.loading", {
            form: props.targetForm,
          })}
        />
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
  // Structured draft is the single authority in this pane. Fields view edits
  // it directly; source view is compatibility ingress that parses back into
  // it; preview and save derive from it. `editorContent` remains as the
  // source textarea buffer kept in sync.
  const [draftTitle, setDraftTitle] = createSignal("");
  const [draftFields, setDraftFields] = createSignal<DraftFields>({});
  const [draftTags, setDraftTags] = createSignal<string[]>([]);
  const [lastSavedContent, setLastSavedContent] = createSignal("");
  const [isDirty, setIsDirty] = createSignal(false);
  const [isSaving, setIsSaving] = createSignal(false);
  // Transient save confirmation for the detail view (PR4): the permanent
  // saved chip is gone; success surfaces once as a toast announcement and
  // then clears. The create flow keeps its inline save area instead.
  const [saveNotice, setSaveNotice] = createSignal<string | null>(null);
  let saveNoticeTimer: ReturnType<typeof setTimeout> | undefined;
  const clearSaveNotice = () => {
    if (saveNoticeTimer) {
      clearTimeout(saveNoticeTimer);
      saveNoticeTimer = undefined;
    }
    setSaveNotice(null);
  };
  const flashSaveNotice = (message: string) => {
    clearSaveNotice();
    setSaveNotice(message);
    if (typeof window !== "undefined") {
      saveNoticeTimer = setTimeout(() => {
        saveNoticeTimer = undefined;
        setSaveNotice(null);
      }, 4000);
    }
  };
  const [conflictMessage, setConflictMessage] = createSignal<string | null>(
    null,
  );
  const [validationError, setValidationError] = createSignal<
    {
      title: string;
      items: string[];
    } | null
  >(null);
  // Canonical invalid fields from the shared Rust boundary (WASM pre-check
  // or server mutation). Presentation hints never write here; Rust wins.
  const [invalidFields, setInvalidFields] = createSignal<string[]>([]);
  const [currentRevisionId, setCurrentRevisionId] = createSignal<string | null>(
    null,
  );
  const [lastLoadedEntryId, setLastLoadedEntryId] = createSignal<string | null>(
    null,
  );
  const [lastLoadedResourceRevisionId, setLastLoadedResourceRevisionId] =
    createSignal<string | null>(null);
  const [entryError, setEntryError] = createSignal<string | null>(null);
  const [compatibilityDiagnostics, setCompatibilityDiagnostics] = createSignal<
    MarkdownConversionDiagnostic[]
  >([]);
  const [pendingCanonicalDraft, setPendingCanonicalDraft] = createSignal<
    CompatDraft | null
  >(null);
  const [assetEditorGeneration, setAssetEditorGeneration] = createSignal(0);
  const [showAdvancedSource, setShowAdvancedSource] = createSignal(false);
  // Compat diagnostics block saving, so auto-open the Advanced source
  // disclosure when they appear: the blocking Markdown stays visible for
  // review. Latch semantics: auto-open on the empty->non-empty transition,
  // remain open when the diagnostic clears, close only on user close, and
  // auto-open again on a later diagnostic episode if closed.
  let hadCompatDiagnostics = false;
  createEffect(() => {
    const has = compatibilityDiagnostics().length > 0;
    if (has && !hadCompatDiagnostics) {
      setShowAdvancedSource(true);
    }
    hadCompatDiagnostics = has;
  });
  const [hasUserEdited, setHasUserEdited] = createSignal(false);
  const [createdEntry, setCreatedEntry] = createSignal<
    {
      id: string;
      revision_id: string;
    } | null
  >(null);
  const [draftSessionFinished, setDraftSessionFinished] = createSignal(false);

  const draftSessionKey = createMemo(() =>
    createEntryDraftSessionKey(props.spaceId())
  );
  const draftSession = getCreateEntryDraftSession(draftSessionKey());

  // Asset upload/read state belongs to this Entry draft. The fields view and
  // the advanced source disclosure are separate conditional subtrees, so
  // keeping this map in either child would lose provisional Files and read
  // state when a subtree unmounts.
  const assetFieldStates = new Map<string, AssetFieldState>();

  // Pending Rust compat reconciliations (source<->draft). Saves settle them
  // first so a rapid source-type + Ctrl+S can never persist TS-only semantics.
  const pendingCompat = new Set<Promise<void>>();
  const trackCompat = (promise: Promise<void>) => {
    pendingCompat.add(promise);
    void promise.finally(() => {
      pendingCompat.delete(promise);
    });
  };
  const settleCompat = async () => {
    const pending = [...pendingCompat];
    if (pending.length === 0) return;
    await Promise.allSettled(pending);
  };

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
          new EntryLoadTimeoutError(),
        );
      } catch (error) {
        /* v8 ignore start */
        setEntryError(
          error instanceof EntryLoadTimeoutError
            ? t("entryDetail.loadTimedOut")
            : formatUserFacingError(
              error,
              "entryDetail.loadFailed",
              "entry.get",
            ),
        );
        /* v8 ignore stop */
        return null;
      }
    },
  );

  const isCreateMode = createMemo(() =>
    !createdEntry() && Boolean(props.createForm?.())
  );
  // Save is strong only when there is dirty work and nothing blocks it;
  // a clean editor shows a weak disabled tool instead of a saved chip.
  const saveReady = () =>
    isDirty() && !isSaving() && compatibilityDiagnostics().length === 0;
  const isAuthoringSession = createMemo(() =>
    !draftSessionFinished() &&
    (Boolean(props.createForm?.()) || Boolean(createdEntry()))
  );
  const draftEntry = createMemo<Entry | null>(() => {
    const created = createdEntry();
    if (created) {
      return {
        id: created.id,
        title: draftTitle(),
        form: props.createForm?.()?.name,
        content: editorContent(),
        revision_id: created.revision_id,
        created_at: "",
        updated_at: "",
      };
    }
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
  const entry = createMemo(() =>
    createdEntry() || isCreateMode() ? draftEntry() : remoteEntry()
  );
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

  createEffect(() => {
    const generation = assetEditorGeneration();
    for (const state of assetFieldStates.values()) {
      state.resetForGeneration(generation);
    }
  });

  onCleanup(() => {
    clearSaveNotice();
    persistCreateDraft();
    for (const state of assetFieldStates.values()) state.dispose();
  });

  const currentDraftState = (): CreateEntryDraftState | null => {
    const formName = currentForm()?.name ?? props.createForm?.()?.name ?? "";
    if (!formName) return null;
    return {
      title: draftTitle(),
      fields: draftFields(),
      tags: draftTags(),
      source: editorContent(),
      assetFields: Object.fromEntries(
        Object.entries(draftFields()).filter(([name]) => name.startsWith("__")),
      ),
      dirty: isDirty() && hasUserEdited(),
    };
  };

  function persistCreateDraft(): void {
    if (!isAuthoringSession()) return;
    const formName = props.createForm?.()?.name ?? "";
    const state = currentDraftState();
    if (formName && state) draftSession.save(formName, state);
  }

  const clearCreateDraft = () => {
    clearCreateEntryDraftSession(draftSessionKey());
  };

  // The router guard covers SPA navigation. The browser event covers closing,
  // reload, and navigation to a non-SPA destination.
  useBeforeLeave?.((event) => {
    persistCreateDraft();
    if (!isAuthoringSession() || !draftSession.hasDirtyWork()) return;
    if (event.defaultPrevented) return;
    event.preventDefault();
    if (
      typeof window === "undefined" ||
      window.confirm(t("entryDetail.confirmLeave"))
    ) {
      clearCreateDraft();
      event.retry(true);
    }
  });

  createEffect(() => {
    if (typeof window === "undefined" || !isAuthoringSession()) return;
    const handleBeforeUnload = (event: BeforeUnloadEvent) => {
      persistCreateDraft();
      if (!draftSession.hasDirtyWork()) return;
      event.preventDefault();
      event.returnValue = "";
    };
    window.addEventListener("beforeunload", handleBeforeUnload);
    onCleanup(() =>
      window.removeEventListener("beforeunload", handleBeforeUnload)
    );
  });
  const formWorkspaceHref = createMemo(() => {
    const formName = entry()?.form?.trim();
    const base = `/spaces/${encodeURIComponent(props.spaceId())}/forms`;
    return formName ? `${base}?form=${encodeURIComponent(formName)}` : base;
  });

  const loadedForms = () => props.forms?.() ?? [];

  // Structured draft is the authority; empty stays empty so the heading
  // falls back to the stable entry ID (never a synthesized "Untitled").
  const editorTitle = createMemo(() => draftTitle());
  const editorGuidance = createMemo(() =>
    buildEditorGuidance(currentForm(), editorContent())
  );

  const requiredFieldErrorId = (fieldId: string) => `${fieldId}-required`;

  const focusFirstMissingRequiredField = () => {
    if (typeof document === "undefined") return;
    // Rust diagnostics drive aria-invalid; focus the first invalid field.
    document.querySelector<HTMLElement>('[aria-invalid="true"]')?.focus();
  };

  const showRustValidationFailure = (
    title: string,
    items: string[],
    fields: string[],
  ) => {
    setConflictMessage(null);
    setInvalidFields(fields);
    setValidationError({ title, items });
    if (fields.length > 0) {
      focusFirstMissingRequiredField();
    } else if (typeof document !== "undefined") {
      // No field to focus (e.g. pending uploads): focus the summary alert.
      queueMicrotask(() => {
        document.querySelector<HTMLElement>("#entry-detail-validation")
          ?.focus();
      });
    }
  };

  const fieldValue = (fieldName: string): string =>
    draftValueToDisplayString(draftFields()[fieldName]);

  const persistedFieldValue = (fieldName: string) => {
    const sections = new Map<string, string>();
    for (const section of parseMarkdownH2Sections(lastSavedContent())) {
      sections.set(normalizeFieldName(section.title), section.content);
    }
    return sections.get(normalizeFieldName(fieldName)) ?? "";
  };

  const fieldIssue = (fieldName: string) =>
    editorGuidance().typeIssues.find((issue) =>
      issue.startsWith(`${fieldName}:`)
    );

  // Shared EntryFields descriptors: title/body/other fields share one
  // spacing contract. Structure only, memoized per form so row identities
  // stay stable across keystrokes (values flow through live bindings, never
  // snapshots); a new form still rebuilds rows for its own fields.
  const entryFieldDescriptors = createMemo((): EntryFieldDescriptor[] => {
    const form = currentForm() ?? props.createForm?.();
    if (!form) return [];
    return Object.entries(form.fields || {}).map(
      ([fieldName, fieldDef], index) => ({
        name: fieldName,
        type: fieldDef.type,
        targetForm: fieldDef.target_form,
        required: isActiveRequiredField(fieldDef),
        fieldId: createEntryFieldInputId(fieldName, index),
      }),
    );
  });

  createEffect(() => {
    const loadedEntry = entry();
    if (!loadedEntry) return;
    // A create that completed while the author kept typing is already bound
    // to the durable identity. Hydrating the synthetic entry here would make
    // the unsaved post-request Work look saved.
    if (createdEntry()) return;
    if (
      loadedEntry.id === lastLoadedEntryId() &&
      loadedEntry.revision_id === lastLoadedResourceRevisionId()
    ) {
      return;
    }
    const defaultContent = loadedEntry.content ?? "";
    const entryId = loadedEntry.id;
    const revisionId = loadedEntry.revision_id;
    const loadedTitle = loadedEntry.title || "";
    const saved = isCreateMode()
      ? draftSession.restore(loadedEntry.form ?? "")
      : undefined;
    const content = saved?.source ?? defaultContent;
    const draft = saved
      ? { title: saved.title, fields: saved.fields }
      : parseEntryMarkdownPresentation(content);
    const tags = saved?.tags ?? readEntryTagsPresentation(content) ?? [];
    setLastLoadedEntryId(entryId);
    setLastLoadedResourceRevisionId(revisionId);
    setCurrentRevisionId(
      createdEntry()?.revision_id ?? (isCreateMode() ? null : revisionId),
    );
    setAssetEditorGeneration((generation) => generation + 1);
    setDraftTitle(draft.title || loadedTitle);
    setDraftFields(draft.fields);
    setDraftTags(tags);
    setEditorContent(content);
    setLastSavedContent(isCreateMode() ? "" : content);
    setHasUserEdited(saved?.dirty ?? false);
    setIsDirty(isCreateMode() ? true : false);
    setConflictMessage(null);
    setValidationError(null);
    setInvalidFields([]);
    setCompatibilityDiagnostics([]);
    setPendingCanonicalDraft(null);
    // Reconcile the immediate parse through the Rust bridge (authority).
    // Guard on the loaded buffer so late reconciliation never wipes user
    // edits made after load.
    trackCompat(
      parseSourceToDraftViaWasm(content, loadedTitle).then(
        (canonical) => {
          if (
            lastLoadedEntryId() !== entryId ||
            lastLoadedResourceRevisionId() !== revisionId
          ) {
            return;
          }
          if (editorContent() !== content) return;
          if (draftTitle() !== (draft.title || loadedTitle)) return;
          if (
            canonical.diagnostics.length > 0 &&
            (currentForm() || props.createForm?.())
          ) {
            setCompatibilityDiagnostics(canonical.diagnostics);
            setPendingCanonicalDraft(canonical);
            return;
          }
          setDraftTitle(canonical.title || loadedTitle);
          setDraftFields(canonical.fields);
          setDraftTags(canonical.tags);
          persistCreateDraft();
        },
        () => {},
      ),
    );
  });

  const syncEditorFromDraft = (title: string, fields: DraftFields) => {
    // Immediate TypeScript compatibility render keeps field editing and the
    // source textarea responsive. The Rust compatibility bridge then
    // reconciles to the canonical 0.1 representation (authority): when both
    // agree nothing changes; when they disagree the Rust output wins.
    const content = updateEntryMarkdownPresentation(
      editorContent(),
      title,
      Object.fromEntries(
        Object.entries(fields).map(([name, value]) => [
          name,
          draftValueToDisplayString(value),
        ]),
      ),
    );
    setEditorContent(content);
    setIsDirty(content !== lastSavedContent());
    setConflictMessage(null);
    setValidationError(null);
    setInvalidFields([]);
    clearSaveNotice();
    persistCreateDraft();

    const formDef = currentForm() ?? props.createForm?.();
    if (!formDef) return;
    const requestTitle = title;
    const requestFields = { ...fields };
    const requestTags = [...draftTags()];
    const requestBaseline = content;
    trackCompat(
      renderDraftToSourceViaWasm(
        formDef,
        requestTitle,
        requestTags,
        requestFields,
        loadedForms(),
      ).then(
        (canonical) => {
          if (editorContent() !== requestBaseline) return;
          if (draftTitle() !== requestTitle) return;
          const current = draftFields();
          for (const [key, value] of Object.entries(requestFields)) {
            if (current[key] !== value) return;
          }
          if (canonical === requestBaseline) return;
          setEditorContent(canonical);
          setIsDirty(canonical !== lastSavedContent());
          persistCreateDraft();
        },
        () => {},
      ),
    );
  };

  const handleContentChange = (content: string) => {
    // Immediate TypeScript compatibility parse keeps the textarea responsive.
    // The Rust bridge then reconciles to the canonical draft (authority).
    // TS failures never block the bridge: they fall back to preserving the
    // previous draft until the canonical parse lands.
    const fallbackTitle = entry()?.title || "";
    try {
      const draft = parseEntryMarkdownPresentation(content);
      const tags = readEntryTagsPresentation(content);
      setDraftTitle(draft.title || fallbackTitle);
      setDraftFields(draft.fields);
      if (tags !== null) setDraftTags(tags);
    } catch {
      // Ignore; the Rust reconciliation below remains authoritative.
    }
    setEditorContent(content);
    setHasUserEdited(true);
    setIsDirty(content !== lastSavedContent());
    setConflictMessage(null);
    setValidationError(null);
    setInvalidFields([]);
    setCompatibilityDiagnostics([]);
    setPendingCanonicalDraft(null);
    clearSaveNotice();
    persistCreateDraft();
    trackCompat(
      parseSourceToDraftViaWasm(content, fallbackTitle).then(
        (canonical) => {
          if (editorContent() !== content) return;
          if (
            canonical.diagnostics.length > 0 &&
            (currentForm() || props.createForm?.())
          ) {
            setCompatibilityDiagnostics(canonical.diagnostics);
            setPendingCanonicalDraft(canonical);
            return;
          }
          setDraftTitle(canonical.title || fallbackTitle);
          setDraftFields(canonical.fields);
          setDraftTags(canonical.tags);
          persistCreateDraft();
        },
        () => {},
      ),
    );
  };

  const acceptCanonicalDraft = () => {
    const canonical = pendingCanonicalDraft();
    if (!canonical) return;
    const title = canonical.title || entry()?.title || "";
    const formDef = currentForm() ?? props.createForm?.();
    const render = formDef
      ? renderDraftToSourceViaWasm(
        formDef,
        title,
        canonical.tags,
        canonical.fields,
        loadedForms(),
      )
      : Promise.resolve(
        [
          canonical.tags.length > 0
            ? `---\ntags:\n${
              canonical.tags.map((tag) => `  - ${tag}`).join("\n")
            }\n---\n`
            : "",
          `# ${title}`,
          ...Object.entries(canonical.fields).flatMap(([name, value]) => [
            `## ${name}`,
            value,
          ]),
        ].join("\n\n").trim(),
      );

    trackCompat(
      render.then(
        (source) => {
          if (pendingCanonicalDraft() !== canonical) return;
          setDraftTitle(title);
          setDraftFields(canonical.fields);
          setDraftTags(canonical.tags);
          setEditorContent(source);
          setCompatibilityDiagnostics([]);
          setPendingCanonicalDraft(null);
          setHasUserEdited(true);
          setIsDirty(true);
          persistCreateDraft();
        },
        (error) => {
          setConflictMessage(
            formatUserFacingError(
              error,
              "entryDetail.saveFailed",
              "entry.update",
            ),
          );
        },
      ),
    );
  };

  const handleFieldChange = (fieldName: string, value: unknown) => {
    setHasUserEdited(true);
    const next = { ...draftFields(), [fieldName]: value };
    setDraftFields(next);
    syncEditorFromDraft(draftTitle(), next);
  };

  const assetFieldState = (fieldName: string, multiple: boolean) => {
    const formName = currentForm()?.name ?? props.createForm?.()?.name ?? "";
    const key = `${formName}\u0000${fieldName}`;
    let state = assetFieldStates.get(key);
    if (!state) {
      state = createAssetFieldState();
      assetFieldStates.set(key, state);
    }
    // The binding belongs to the Entry draft, not either conditionally
    // mounted AssetField view. Upload completion therefore always resolves
    // against the latest Markdown draft.
    state.bindDraft({
      multiple,
      getValue: () => draftFields()[fieldName],
      setValue: (value) => handleFieldChange(fieldName, value),
    });
    return state;
  };

  const validateAssetFields = async (): Promise<string[]> => {
    // Asset type/coercion/duplicate/required decisions belong to the shared
    // Rust boundary (`entry.validate_draft` + server mutation). TypeScript
    // only blocks on upload state, which Rust cannot observe.
    const hasPendingUpload = Array.from(assetFieldStates.values()).some(
      (state) => state.pendingUploads().length > 0,
    );
    if (hasPendingUpload) {
      return [t("entryDetail.validation.assetUploadPending")];
    }
    return [];
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
    const entryId = createdEntry()?.id ?? props.entryId?.() ?? "";
    if (isCreateMode()) {
      if (!wsId) {
        return { ok: false, reason: t("entryDetail.savePrerequisite") };
      }
      return { ok: true, wsId, create: true };
    }
    /* v8 ignore start */
    const revisionId = currentRevisionId() || createdEntry()?.revision_id ||
      entry()?.revision_id;
    if (!wsId || !entryId || !revisionId) {
      return {
        ok: false,
        reason: t("entryDetail.savePrerequisite"),
      };
    }
    /* v8 ignore stop */
    return { ok: true, wsId, entryId, revisionId, create: false };
  };

  const handleSaveError = (error: unknown) => {
    const parsed = parseEntryValidationError(error);
    if (parsed) {
      showRustValidationFailure(parsed.title, parsed.items, parsed.fields);
      return;
    } else if (error instanceof RevisionConflictError) {
      setConflictMessage(
        error.apiError
          ? formatUserFacingError(
            error.apiError,
            "entryDetail.saveFailed",
            "entry.update",
          )
          : t("errors.code.revisionConflict"),
      );
    } else {
      setConflictMessage(
        formatUserFacingError(error, "entryDetail.saveFailed", "entry.update"),
      );
    }
  };

  const handleSave = async () => {
    if (isSaving()) return;
    const context = resolveSaveContext();
    /* v8 ignore start */
    if (!context.ok) {
      setConflictMessage(context.reason);
      return;
    }
    /* v8 ignore stop */

    // Saveability is decided by the shared Rust boundary. TypeScript hints
    // (required guidance, boolean/list formatting) never block a save;
    // loss-producing compatibility conversion is an explicit exception.
    // Lock before async validation so rapid saves still yield one revision.
    setIsSaving(true);
    clearSaveNotice();
    // Settle pending source<->draft reconciliations first so a rapid
    // source-type + save can never persist TS-only semantics.
    await settleCompat();
    if (compatibilityDiagnostics().length > 0) {
      setIsSaving(false);
      setConflictMessage(t("entryDetail.compatibilityLossSaveBlocked"));
      return;
    }
    const assetIssues = await validateAssetFields();
    if (assetIssues.length > 0) {
      setIsSaving(false);
      showRustValidationFailure(
        t("entryDetail.validation.title"),
        assetIssues,
        [],
      );
      return;
    }

    // Structured wire authority when the Form is known; formless notes keep
    // the Markdown compatibility path. Field values share the webform builder
    // so trimming and zoned-timestamp normalization agree.
    const formDef = currentForm() ?? props.createForm?.();
    const formName = formDef?.name;
    const title = draftTitle();
    const fields: Record<string, unknown> = formDef
      ? toTransportFields(formDef, draftFields())
      : Object.fromEntries(
        Object.entries(draftFields()).filter(([name, value]) => {
          if (name.startsWith("__")) return false;
          const text = draftValueToDisplayString(value);
          return text.trim().length > 0;
        }).map(([name, value]) => [
          name,
          draftValueToDisplayString(value).trim(),
        ]),
      );

    // Pre-save Rust validation: same classification as the server mutation.
    // On conflict the Rust result wins over any TypeScript hint. Bridge
    // failures must never leave the save lock stuck.
    if (formDef && formName) {
      let precheck;
      try {
        precheck = await validateEntryDraftViaWasm(formDef, {
          title,
          tags: draftTags(),
          fields,
        }, loadedForms());
      } catch (error) {
        setIsSaving(false);
        showRustValidationFailure(
          t("entryDetail.saveFailed"),
          [formatUserFacingError(
            error,
            "entryDetail.saveFailed",
            "entry.create",
          )],
          [],
        );
        return;
      }
      if (!precheck.ok) {
        setIsSaving(false);
        const parsed = parseEntryValidationError(precheck.error);
        if (parsed) {
          showRustValidationFailure(parsed.title, parsed.items, parsed.fields);
        } else {
          showRustValidationFailure(
            t("entryDetail.validationFailed"),
            [precheck.message],
            precheck.invalidFields,
          );
        }
        return;
      }
    }

    setConflictMessage(null);
    setValidationError(null);
    setInvalidFields([]);
    // This is the exact request-start snapshot. Any edits after this point
    // remain Work and must not trigger a route transition on create.
    const requestSnapshot = {
      title: draftTitle(),
      fields: JSON.stringify(fields),
      tags: JSON.stringify(draftTags()),
      source: editorContent(),
    };
    const contentToSave = requestSnapshot.source;
    const currentSnapshot = () => ({
      title: draftTitle(),
      fields: JSON.stringify(
        formDef ? toTransportFields(formDef, draftFields()) : fields,
      ),
      tags: JSON.stringify(draftTags()),
      source: editorContent(),
    });
    try {
      // Structured wire authority when the Form is known; the same
      // `entry.create`/`entry.update` operations carry either shape.
      const result = formName
        ? context.create
          ? await entryApi.create(context.wsId, {
            form: formName,
            tags: draftTags(),
            fields,
          })
          : await entryApi.update(context.wsId, context.entryId!, {
            form: formName,
            tags: draftTags(),
            fields,
            parent_revision_id: context.revisionId!,
          })
        : context.create
        ? await entryApi.create(context.wsId, { markdown: contentToSave })
        : await entryApi.update(context.wsId, context.entryId!, {
          markdown: contentToSave,
          parent_revision_id: context.revisionId!,
        });
      setCurrentRevisionId(result.revision_id);
      setLastSavedContent(contentToSave);
      const unchanged = JSON.stringify(currentSnapshot()) ===
        JSON.stringify(requestSnapshot);
      setIsDirty(!unchanged);
      if (!context.create && unchanged) {
        // Transient confirmation only: neither view keeps a permanent
        // saved chip. The create flow announces through the same toast.
        flashSaveNotice(t("entryDetail.saved"));
      }
      if (!unchanged && context.create) {
        // Bind the durable identity without replacing the active draft. The
        // next save is an optimistic update against this revision.
        setCreatedEntry(result);
        setHasUserEdited(true);
        persistCreateDraft();
      }
      if (!context.create && createdEntry() && unchanged) {
        clearCreateDraft();
        setDraftSessionFinished(true);
        props.onCreated?.(result);
      }
      props.onAfterSave?.();
      if (context.create && unchanged) {
        setDraftSessionFinished(true);
        clearCreateDraft();
        props.onCreated?.(result);
      }
    } catch (error) {
      handleSaveError(error);
    } finally {
      setIsSaving(false);
    }
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
      alert(
        formatUserFacingError(
          error,
          "entryDetail.deleteFailed",
          "entry.delete",
        ),
      );
      /* v8 ignore stop */
    }
  };

  const handleCreateFormChange = (formName: string) => {
    // Capture before the parent changes the Form accessor. The next load
    // effect restores the state belonging to the selected Form.
    persistCreateDraft();
    props.onCreateFormChange?.(formName);
  };

  const renderFieldControl = (
    fieldName: string,
    fieldDef: FormField,
    fieldId: string,
    invalid: () => boolean,
    describedBy: () => string | undefined,
  ) => {
    const value = () => fieldValue(fieldName);

    if (fieldDef.type === "row_reference" && fieldDef.target_form?.trim()) {
      const targetForm = fieldDef.target_form.trim();
      const targetFormName = props.forms?.().find((form) =>
        form.id === targetForm
      )?.name ?? targetForm;
      return (
        <EntryRowReferenceField
          spaceId={props.spaceId()}
          fieldId={fieldId}
          targetForm={targetFormName}
          value={value()}
          invalid={invalid()}
          describedBy={invalid() ? describedBy() : undefined}
          onChange={(nextValue) => handleFieldChange(fieldName, nextValue)}
        />
      );
    }

    if (
      fieldDef.type === "asset_reference" || isAssetReferenceListField(fieldDef)
    ) {
      return (
        <AssetField
          fieldId={fieldId}
          fieldName={fieldName}
          value={draftFields()[fieldName]}
          persistedValue={persistedFieldValue(fieldName)}
          multiple={isAssetReferenceListField(fieldDef)}
          spaceId={props.spaceId()}
          state={assetFieldState(
            fieldName,
            isAssetReferenceListField(fieldDef),
          )}
          invalid={invalid()}
          describedBy={invalid() ? describedBy() : undefined}
          formName={entry()?.form ?? currentForm()?.name}
          entryId={isCreateMode() ? undefined : entry()?.id}
          generation={assetEditorGeneration()}
          onChange={(nextValue) => handleFieldChange(fieldName, nextValue)}
        />
      );
    }

    if (
      fieldDef.type === "list" &&
      !isAssetReferenceListField(fieldDef) &&
      isPlainStringListField(fieldDef)
    ) {
      // Repeated typed controls instead of newline conventions: each item
      // is a text box, adding/removing marks the entry dirty, and the user
      // never types list syntax.
      const items = () => normalizeStringListValue(draftFields()[fieldName]);
      return (
        <ListEditor
          values={items()}
          onChange={(next) => handleFieldChange(fieldName, next)}
          createItem={() => ""}
          addLabel={t("entryDetail.listAddItem")}
          renderItem={(item, index, helpers) => (
            <div class="flex items-center gap-2">
              <input
                id={index === 0 ? fieldId : `${fieldId}-${index}`}
                class="ui-input"
                value={item}
                aria-label={t("entryDetail.listItemLabel", {
                  field: fieldName,
                  index: index + 1,
                })}
                aria-invalid={invalid() ? "true" : undefined}
                aria-describedby={invalid() ? describedBy() : undefined}
                placeholder={t("entryDetail.fieldPlaceholder")}
                onInput={(event) =>
                  handleFieldChange(
                    fieldName,
                    items().map((entry, position) =>
                      position === index ? event.currentTarget.value : entry
                    ),
                  )}
              />
              <button
                type="button"
                class="ui-button ui-button-secondary ui-button-sm text-sm"
                aria-label={t("entryDetail.listRemoveItem", {
                  field: fieldName,
                  index: index + 1,
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

    if (
      fieldDef.type === "markdown" ||
      (fieldDef.type === "list" && !isAssetReferenceListField(fieldDef)) ||
      fieldDef.type === "object_list"
    ) {
      return (
        <textarea
          id={fieldId}
          class="ui-input ui-textarea"
          value={value()}
          aria-invalid={invalid() ? "true" : undefined}
          aria-describedby={invalid() ? describedBy() : undefined}
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
        inputmode={resolveInputMode(fieldDef)}
        value={value()}
        aria-invalid={invalid() ? "true" : undefined}
        aria-describedby={invalid() ? describedBy() : undefined}
        placeholder={t("entryDetail.fieldPlaceholder")}
        onInput={(event) =>
          handleFieldChange(fieldName, event.currentTarget.value)}
      />
    );
  };

  /* v8 ignore start */
  return (
    <div class="ui-entry-page" aria-busy={entryLoading() || undefined}>
      {
        /* Panel-local spinner only: fields stay mounted and visible during
          refetch. No full-screen overlay, no visible loading text. */
      }
      <Show when={entryLoading() && !entry()}>
        <div class="ui-entry-loading">
          <LocalBusyIndicator label={t("entryDetail.loading")} />
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
                  {t("entryDetail.spaceId")}: {props.spaceId()} / {t(
                    "entryDetail.entryId",
                  )}: {props.entryId?.() ?? ""}
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
                <BackLink
                  href={formWorkspaceHref()}
                  label={t("entryDetail.back")}
                />
                <Show when={isCreateMode()}>
                  <h2 class="ui-page-subtitle mt-2">
                    {t("createDialog.entry.heading")}
                  </h2>
                </Show>
                <div class="mt-2 flex flex-wrap items-center gap-2">
                  <h1 class="ui-page-title truncate">
                    {editorTitle().trim() || currentEntry().id}
                  </h1>
                  <Show when={currentEntry().form && !(isCreateMode() && props.forms && props.onCreateFormChange)}>
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
                      disabled={isSaving()}
                      onChange={(event) =>
                        handleCreateFormChange(event.currentTarget.value)}
                    >
                      <For each={props.forms?.() ?? []}>
                        {(form) => (
                          <option value={form.name}>{form.name}</option>
                        )}
                      </For>
                    </select>
                  </Show>
                </div>
              </div>
            </header>

            <Show when={!isCreateMode()}>
              {/*
                Single one-line action bar (PR4): save rides with
                history/info/delete instead of a separate header save area.
                Save is filled/strong only when dirty and unblocked, weak and
                disabled when clean. The permanent saved chip is gone; success
                announces once through the transient toast below.
              */}
              <ActionIconBar
                label={t("entryDetail.actionBarLabel")}
                class="ui-entry-action-bar"
                items={[
                  {
                    key: "save",
                    label: t("entryDetail.save"),
                    accessibleName: t("entryDetail.save"),
                    icon: "save",
                    class: saveReady()
                      ? "ui-entry-tool ui-entry-tool-primary"
                      : "ui-entry-tool",
                    disabled: !saveReady(),
                    busy: isSaving(),
                    onClick: () => {
                      void handleSave();
                    },
                  },
                  {
                    key: "history",
                    label: t("entryDetail.action.historyShort"),
                    accessibleName: t("entryDetail.history"),
                    icon: "history",
                    class: "ui-entry-tool",
                    href: `/spaces/${encodeURIComponent(props.spaceId())}/entries/${
                      encodeURIComponent(props.entryId?.() ?? "")
                    }/history`,
                  },
                  {
                    key: "info",
                    label: t("entryDetail.action.infoShort"),
                    accessibleName: t("entryDetail.info"),
                    icon: "info",
                    class: "ui-entry-tool",
                    href: `/spaces/${encodeURIComponent(props.spaceId())}/entries/${
                      encodeURIComponent(props.entryId?.() ?? "")
                    }/info`,
                  },
                  {
                    key: "delete",
                    label: t("entryDetail.action.deleteShort"),
                    accessibleName: t("entryDetail.delete"),
                    icon: "trash",
                    danger: true,
                    class: "ui-entry-tool ui-entry-tool-danger",
                    onClick: handleDelete,
                  },
                ]}
              />
              <Show when={isSaving() || saveNotice()}>
                <p class="ui-save-toast" role="status">
                  {isSaving() ? t("entryDetail.saving") : saveNotice()}
                </p>
              </Show>
            </Show>
            <Show when={isCreateMode()}>
              {/*
                Same interaction pattern as the detail view: the header
                carries Back navigation, this bar carries Save. Save is
                strong only for valid unsaved changes; no separate
                unsaved-changes badge. Saving state shows on the Save
                action itself (creation navigates away on success, so no
                success toast is needed here).
              */}
              <ActionIconBar
                label={t("entryDetail.actionBarLabel")}
                class="ui-entry-action-bar"
                items={[
                  {
                    key: "save",
                    label: t("entryDetail.save"),
                    accessibleName: t("entryDetail.save"),
                    icon: "save",
                    class: saveReady()
                      ? "ui-entry-tool ui-entry-tool-primary"
                      : "ui-entry-tool",
                    disabled: !saveReady(),
                    busy: isSaving(),
                    onClick: () => {
                      void handleSave();
                    },
                  },
                ]}
              />
              <Show when={isSaving() || saveNotice()}>
                <p class="ui-save-toast" role="status">
                  {isSaving() ? t("entryDetail.saving") : saveNotice()}
                </p>
              </Show>
            </Show>

            <Show when={validationError()}>
              {(error) => (
                <div
                  id="entry-detail-validation"
                  class="ui-alert ui-alert-warning text-sm"
                  role="alert"
                  tabindex="-1"
                >
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
              <main class="ui-entry-main">
                <Show
                  when={currentForm()}
                  fallback={
                    <div
                      id="entry-source-panel"
                      class="ui-entry-source-body"
                    >
                      <Show when={compatibilityDiagnostics().length > 0}>
                        <div
                          class="ui-alert ui-alert-warning text-sm mb-3"
                          role="alert"
                        >
                          <p class="font-semibold">
                            {t("entryDetail.compatibilityLossTitle")}
                          </p>
                          <ul class="mt-2 list-disc pl-5 space-y-1">
                            <For each={compatibilityDiagnostics()}>
                              {(diagnostic) => (
                                <li>
                                  {formatMarkdownConversionDiagnostic(
                                    diagnostic,
                                  )}
                                </li>
                              )}
                            </For>
                          </ul>
                          <p class="mt-2">
                            {t("entryDetail.compatibilityLossDescription")}
                          </p>
                          <Show when={pendingCanonicalDraft()}>
                            <button
                              type="button"
                              class="ui-button ui-button-secondary mt-3"
                              onClick={acceptCanonicalDraft}
                            >
                              {t("entryDetail.acceptCanonicalVersion")}
                            </button>
                          </Show>
                        </div>
                      </Show>
                      <textarea
                        class="ui-editor ui-entry-source-editor"
                        value={editorContent()}
                        onInput={(event) =>
                          handleContentChange(event.currentTarget.value)}
                        onKeyDown={handleEditorKeyDown}
                        aria-label={t("entryDetail.sourcePlaceholder")}
                        placeholder={t("entryDetail.sourcePlaceholder")}
                        spellcheck={false}
                      />
                    </div>
                  }
                >
                  {(entryForm) => (
                    <div
                      id="entry-fields-panel"
                      class="ui-entry-form-body"
                    >
                      <EntryFields
                        fields={entryFieldDescriptors()}
                        getValue={fieldValue}
                        isInvalid={(fieldName) =>
                          invalidFields().includes(fieldName)}
                        describedBy={requiredFieldErrorId}
                        renderControl={(field, getHelpers) => {
                          const fieldDef =
                            (entryForm().fields || {})[field.name];
                          // Descriptors always derive from this form, so this
                          // guard is dead code for type narrowing only.
                          if (!fieldDef) return null;
                          // Helpers stay lazy accessors: reading them here
                          // would subscribe this field expression to
                          // validation state and re-create the control on
                          // every keystroke.
                          return renderFieldControl(
                            field.name,
                            fieldDef,
                            field.fieldId,
                            () => getHelpers().invalid,
                            () => getHelpers().describedBy ??
                              requiredFieldErrorId(field.fieldId),
                          );
                        }}
                        belowField={(below) => (
                          <>
                            <Show
                              when={!invalidFields().includes(below.name) &&
                                editorGuidance().missingRequired.includes(
                                  below.name,
                                )}
                            >
                              <p class="text-xs ui-muted">
                                {t("entryDetail.requiredMessage")}
                              </p>
                            </Show>
                            <Show
                              when={invalidFields().includes(below.name)}
                            >
                              <p
                                class="text-xs ui-text-danger"
                                role="alert"
                              >
                                {validationError()?.items.find((item) =>
                                  item.startsWith(`${below.name}:`) ||
                                  item.includes(below.name)
                                ) ?? t("entryDetail.requiredMessage")}
                              </p>
                            </Show>
                            <Show when={fieldIssue(below.name)}>
                              {(issue) => (
                                <p class="text-xs ui-muted">
                                  {issue()}
                                </p>
                              )}
                            </Show>
                          </>
                        )}
                      />

                      <Show
                        when={Object.keys(entryForm().fields || {}).length ===
                          0}
                      >
                        <div class="ui-entry-no-fields">
                          <p class="font-medium">
                            {t("entryDetail.noFields")}
                          </p>
                          <button
                            type="button"
                            class="ui-button ui-button-secondary mt-4"
                            onClick={() =>
                              setShowAdvancedSource((value) => !value)}
                            aria-expanded={showAdvancedSource()}
                            aria-controls="entry-source-panel-advanced"
                          >
                            {t("entryDetail.openSource")}
                          </button>
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
                            onClick={() =>
                              setShowAdvancedSource((value) => !value)}
                            aria-expanded={showAdvancedSource()}
                            aria-controls="entry-source-panel-advanced"
                          >
                            {t("entryDetail.reviewSource")}
                          </button>
                        </div>
                      </Show>
                      <details
                        class="ui-entry-source-disclosure"
                        open={showAdvancedSource()}
                        onToggle={(event) =>
                          setShowAdvancedSource(event.currentTarget.open)}
                      >
                        <summary>{t("entryDetail.advanced")}</summary>
                        <div
                          id="entry-source-panel-advanced"
                          class="ui-entry-source-body"
                        >
                          <Show when={compatibilityDiagnostics().length > 0}>
                            <div
                              class="ui-alert ui-alert-warning text-sm mb-3"
                              role="alert"
                            >
                              <p class="font-semibold">
                                {t("entryDetail.compatibilityLossTitle")}
                              </p>
                              <ul class="mt-2 list-disc pl-5 space-y-1">
                                <For each={compatibilityDiagnostics()}>
                                  {(diagnostic) => (
                                    <li>
                                      {formatMarkdownConversionDiagnostic(
                                        diagnostic,
                                      )}
                                    </li>
                                  )}
                                </For>
                              </ul>
                              <p class="mt-2">
                                {t("entryDetail.compatibilityLossDescription")}
                              </p>
                              <Show when={pendingCanonicalDraft()}>
                                <button
                                  type="button"
                                  class="ui-button ui-button-secondary mt-3"
                                  onClick={acceptCanonicalDraft}
                                >
                                  {t("entryDetail.acceptCanonicalVersion")}
                                </button>
                              </Show>
                            </div>
                          </Show>
                          <textarea
                            class="ui-editor ui-entry-source-editor"
                            value={editorContent()}
                            onInput={(event) =>
                              handleContentChange(event.currentTarget.value)}
                            onKeyDown={handleEditorKeyDown}
                            aria-label={t(
                              "entryDetail.advancedSourcePlaceholder",
                            )}
                            placeholder={t("entryDetail.sourcePlaceholder")}
                            spellcheck={false}
                          />
                        </div>
                      </details>
                    </div>
                  )}
                </Show>
              </main>
            </div>
          </>
        )}
      </Show>
    </div>
  );
  /* v8 ignore stop */
}
