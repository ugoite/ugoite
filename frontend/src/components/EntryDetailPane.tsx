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
import { ConfirmDestructiveAction } from "~/components/ConfirmDestructiveAction";
import {
  createEntryFieldInputId,
  type EntryFieldDescriptor,
  EntryFields,
} from "~/components/EntryFields";
import { FieldInput } from "~/components/fields";
import { FieldValuesView } from "~/components/fields/FieldValue";
import { LocalBusyIndicator } from "~/components/LocalBusyIndicator";
import {
  type AssetFieldState,
  createAssetFieldState,
} from "~/lib/asset-field-state";
import { t } from "~/lib/i18n";
import { createResource } from "~/lib/recoverable-resource";
import {
  type DraftFields,
  draftValueToDisplayString,
  readAssetReferences,
  toTransportFields,
} from "~/lib/draft-values";
import { entryApi, RevisionConflictError } from "~/lib/ugoite-client";
import { UgoiteApiError } from "~/lib/ugoite-client/protocol";
import { validateEntryDraftViaWasm } from "~/lib/entry-validation";
import type { Entry, Form, FormField } from "~/lib/types";
import { isAssetReferenceListField } from "~/lib/asset-reference";
import { formatUserFacingError } from "~/lib/user-facing-error";
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

const BOOLEAN_VALUE_REGEX = /^(true|false|yes|no|on|off|1|0)$/i;
// Presentation hint only. The shared Rust boundary owns boolean coercion;
// this regex never decides saveability.

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

function buildEditorGuidance(form: Form | null, fields: DraftFields) {
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

  /* v8 ignore start */
  const formFields = Object.entries(form.fields || {});
  /* v8 ignore stop */

  const missingRequired = formFields
    .filter(([fieldName, fieldDef]) => {
      if (!isActiveRequiredField(fieldDef)) return false;
      return isMissingRequiredValue(
        fieldDef,
        draftValueToDisplayString(fields[fieldName]),
      );
    })
    .map(([fieldName]) => fieldName);

  const typeIssues: string[] = [];
  for (const [fieldName, fieldDef] of formFields) {
    const value = draftValueToDisplayString(fields[fieldName]).trim();
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

  return { missingRequired, unknownSections: [], typeIssues };
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

export function EntryDetailPane(props: EntryDetailPaneProps) {
  const [editorContent, setEditorContent] = createSignal("");
  // Structured draft is the only mutation authority in this pane. The stored
  // representation is retained only for read-only compatibility and asset
  // previews; it is never edited or sent back as a mutation payload.
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
  // 409 recovery state: the user draft is never touched on conflict. The
  // server revision id is shown, the latest saved version can be fetched for
  // side-by-side review (read-only latest via FieldValuesView), and the next
  // save re-runs explicitly against the adopted base. No auto-merge.
  const [serverRevisionId, setServerRevisionId] = createSignal<string | null>(
    null,
  );
  const [latestEntry, setLatestEntry] = createSignal<Entry | null>(null);
  const [showLatest, setShowLatest] = createSignal(false);
  const [latestLoading, setLatestLoading] = createSignal(false);
  // Destructive delete runs behind the shared confirmation dialog: the
  // failure stays inside the dialog and the draft stays intact.
  const [deleteConfirmOpen, setDeleteConfirmOpen] = createSignal(false);
  const [isDeleting, setIsDeleting] = createSignal(false);
  const [deleteError, setDeleteError] = createSignal<string | null>(null);
  // Unsaved-work navigation guard: the pending router retry runs only after
  // explicit confirmation in the shared dialog (no window.confirm).
  const [leaveConfirmOpen, setLeaveConfirmOpen] = createSignal(false);
  let pendingLeaveRetry: (() => void) | null = null;
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
  const [assetEditorGeneration, setAssetEditorGeneration] = createSignal(0);
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

  // Asset upload/read state belongs to this Entry draft. Keeping this map
  // here preserves provisional Files and read state across field remounts.
  const assetFieldStates = new Map<string, AssetFieldState>();

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
  const saveReady = () => isDirty() && !isSaving();
  const isAuthoringSession = createMemo(() =>
    !draftSessionFinished() &&
    (Boolean(props.createForm?.()) || Boolean(createdEntry()))
  );
  const draftEntry = createMemo<Entry | null>(() => {
    const created = createdEntry();
    if (created) {
      return {
        id: created.id,
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
      form: form.name,
      content: "",
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
      fields: draftFields(),
      tags: draftTags(),
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
    if (leaveConfirmOpen()) return;
    pendingLeaveRetry = () => event.retry(true);
    setLeaveConfirmOpen(true);
  });

  const confirmLeave = () => {
    clearCreateDraft();
    setLeaveConfirmOpen(false);
    const retry = pendingLeaveRetry;
    pendingLeaveRetry = null;
    retry?.();
  };

  const cancelLeave = () => {
    pendingLeaveRetry = null;
    setLeaveConfirmOpen(false);
  };

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

  const editorGuidance = createMemo(() =>
    buildEditorGuidance(currentForm(), draftFields())
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
    setServerRevisionId(null);
    setLatestEntry(null);
    setShowLatest(false);
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

  const persistedFieldValue = (fieldName: string) =>
    entry()?.sections?.[fieldName] ?? "";

  const fieldIssue = (fieldName: string) =>
    editorGuidance().typeIssues.find((issue) =>
      issue.startsWith(`${fieldName}:`)
    );

  // Shared EntryFields descriptors: all Form fields share one
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
    const entryId = loadedEntry.id;
    const revisionId = loadedEntry.revision_id;
    const saved = isCreateMode()
      ? draftSession.restore(loadedEntry.form ?? "")
      : undefined;
    const content = loadedEntry.content ?? "";
    const draft = saved
      ? { fields: saved.fields }
      : { fields: loadedEntry.sections ?? {} };
    const tags = saved?.tags ?? loadedEntry.tags ?? [];
    setLastLoadedEntryId(entryId);
    setLastLoadedResourceRevisionId(revisionId);
    setCurrentRevisionId(
      createdEntry()?.revision_id ?? (isCreateMode() ? null : revisionId),
    );
    setAssetEditorGeneration((generation) => generation + 1);
    setDraftFields(draft.fields);
    setDraftTags(tags);
    setEditorContent(content);
    setLastSavedContent(isCreateMode() ? "" : content);
    setHasUserEdited(saved?.dirty ?? false);
    setIsDirty(isCreateMode() ? true : false);
    setConflictMessage(null);
    setServerRevisionId(null);
    setLatestEntry(null);
    setShowLatest(false);
    setValidationError(null);
    setInvalidFields([]);
  });

  const handleFieldChange = (fieldName: string, value: unknown) => {
    setHasUserEdited(true);
    const next = { ...draftFields(), [fieldName]: value };
    setDraftFields(next);
    setIsDirty(true);
    setConflictMessage(null);
    setValidationError(null);
    setInvalidFields([]);
    clearSaveNotice();
    persistCreateDraft();
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
      // The user draft stays exactly as typed: only the recovery state is
      // recorded. Re-save runs explicitly after the author reviews.
      setServerRevisionId(
        typeof error.currentRevisionId === "string" &&
          error.currentRevisionId.trim()
          ? error.currentRevisionId
          : null,
      );
      setLatestEntry(null);
      setShowLatest(false);
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
    // (required guidance, boolean/list formatting) never block a save.
    // Lock before async validation so rapid saves still yield one revision.
    setIsSaving(true);
    clearSaveNotice();
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

    // Structured wire authority: Entry mutations always carry the Form and
    // field map. Markdown Form fields remain ordinary field values.
    const formDef = currentForm() ?? props.createForm?.();
    const formName = formDef?.name;
    if (!formDef || !formName) {
      setIsSaving(false);
      setConflictMessage(t("entryDetail.savePrerequisite"));
      return;
    }
    const fields: Record<string, unknown> = toTransportFields(
      formDef,
      draftFields(),
    );

    // Pre-save Rust validation: same classification as the server mutation.
    // On conflict the Rust result wins over any TypeScript hint. Bridge
    // failures must never leave the save lock stuck.
    if (formDef && formName) {
      let precheck;
      try {
        precheck = await validateEntryDraftViaWasm(formDef, {
          tags: draftTags(),
          fields,
        }, props.forms?.() ?? []);
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
      fields: JSON.stringify(fields),
      tags: JSON.stringify(draftTags()),
    };
    const currentSnapshot = () => ({
      fields: JSON.stringify(
        formDef ? toTransportFields(formDef, draftFields()) : fields,
      ),
      tags: JSON.stringify(draftTags()),
    });
    try {
      const result = context.create
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
        });
      setCurrentRevisionId(result.revision_id);
      setServerRevisionId(null);
      setLatestEntry(null);
      setShowLatest(false);
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

  const openDeleteConfirm = () => {
    if (isDeleting()) return;
    setDeleteError(null);
    setDeleteConfirmOpen(true);
  };

  const closeDeleteConfirm = () => {
    if (isDeleting()) return;
    setDeleteConfirmOpen(false);
  };

  const handleDelete = async () => {
    const wsId = props.spaceId();
    const entryId = props.entryId?.() ?? "";
    /* v8 ignore start */
    if (!wsId || !entryId || isDeleting()) return;
    /* v8 ignore stop */

    setIsDeleting(true);
    setDeleteError(null);
    try {
      await entryApi.delete(wsId, entryId);
      setDeleteConfirmOpen(false);
      props.onDeleted();
    } catch (error) {
      // The failure stays inside the dialog; the draft stays intact and the
      // dialog stays open for retry or safe dismiss.
      setDeleteError(
        formatUserFacingError(
          error,
          "entryDetail.deleteFailed",
          "entry.delete",
        ),
      );
    } finally {
      setIsDeleting(false);
    }
  };

  // 409 recovery: fetch the latest saved version for side-by-side review.
  // The local draft is never overwritten; adopting the base only re-points
  // the next save.
  const loadLatestForConflict = async () => {
    const wsId = props.spaceId();
    const entryId = createdEntry()?.id ?? props.entryId?.() ?? "";
    if (!wsId || !entryId || latestLoading()) return;
    setLatestLoading(true);
    try {
      setLatestEntry(await entryApi.get(wsId, entryId));
      setShowLatest(true);
    } catch (error) {
      setConflictMessage(
        formatUserFacingError(error, "entryDetail.saveFailed", "entry.get"),
      );
    } finally {
      setLatestLoading(false);
    }
  };

  const adoptLatestRevisionBase = () => {
    const serverRevision = serverRevisionId();
    if (!serverRevision) return;
    // Keep every keystroke; only the optimistic base moves forward so the
    // next explicit save records the draft on top of the latest revision.
    setCurrentRevisionId(serverRevision);
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

    // One component family shared with the create dialog: string/markdown,
    // number, boolean, date, list, object_list, row_reference, and
    // list<row_reference> render through FieldInput. Frontend owns
    // display/interaction only; validity authority stays in Rust. Asset kinds
    // stay a call-site override above (AssetField); this call never filters
    // values alone.
    const resolveTargetFormName = (target: string | undefined) => {
      const trimmed = target?.trim() ?? "";
      if (!trimmed) return target;
      // The options lookup filters by Form name while stored targets may be
      // opaque Form ids; resolve through the loaded catalog. Unresolvable
      // values pass through untouched for the Rust boundary to diagnose.
      return props.forms?.().find((form) => form.id === trimmed)?.name ??
        target;
    };
    const resolvedField: FormField = {
      ...fieldDef,
      target_form: resolveTargetFormName(fieldDef.target_form),
      items: fieldDef.items
        ? {
          ...fieldDef.items,
          target_form: resolveTargetFormName(fieldDef.items.target_form),
        }
        : fieldDef.items,
    };
    return (
      <FieldInput
        field={resolvedField}
        value={draftFields()[fieldName]}
        onChange={(nextValue) => handleFieldChange(fieldName, nextValue)}
        fieldId={fieldId}
        fieldName={fieldName}
        spaceId={props.spaceId()}
        multiline={fieldDef.type === "markdown"}
        invalid={invalid()}
        describedBy={invalid() ? describedBy() : undefined}
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
                    {currentEntry().id}
                  </h1>
                  <Show
                    when={currentEntry().form &&
                      !(isCreateMode() && props.forms &&
                        props.onCreateFormChange)}
                  >
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
              {
                /*
                Single one-line action bar (PR4): save rides with
                history/info/delete instead of a separate header save area.
                Save is filled/strong only when dirty and unblocked, weak and
                disabled when clean. The permanent saved chip is gone; success
                announces once through the transient toast below.
              */
              }
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
                    href: `/spaces/${
                      encodeURIComponent(props.spaceId())
                    }/entries/${
                      encodeURIComponent(props.entryId?.() ?? "")
                    }/history`,
                  },
                  {
                    key: "info",
                    label: t("entryDetail.action.infoShort"),
                    accessibleName: t("entryDetail.info"),
                    icon: "info",
                    class: "ui-entry-tool",
                    href: `/spaces/${
                      encodeURIComponent(props.spaceId())
                    }/entries/${
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
                    onClick: openDeleteConfirm,
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
              {
                /*
                Same interaction pattern as the detail view: the header
                carries Back navigation, this bar carries Save. Save is
                strong only for valid unsaved changes; no separate
                unsaved-changes badge. Saving state shows on the Save
                action itself (creation navigates away on success, so no
                success toast is needed here).
              */
              }
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
              <div
                class="ui-alert ui-alert-error text-sm ui-entry-conflict"
                role="alert"
              >
                <Show
                  when={serverRevisionId()}
                  fallback={conflictMessage()}
                >
                  {(serverRevision) => (
                    <div class="ui-stack-sm">
                      <p class="font-semibold">
                        {t("entryDetail.conflictHeading")}
                      </p>
                      <p>{t("entryDetail.conflictBody")}</p>
                      <p>
                        {t("entryDetail.conflictServerRevision", {
                          revision: serverRevision(),
                        })}
                      </p>
                      <p class="ui-muted">{conflictMessage()}</p>
                      <div class="flex flex-wrap gap-2">
                        <button
                          type="button"
                          class="ui-button ui-button-secondary ui-button-sm text-xs"
                          disabled={latestLoading()}
                          onClick={() => void loadLatestForConflict()}
                          aria-expanded={showLatest()}
                        >
                          {showLatest()
                            ? t("entryDetail.conflictHideLatest")
                            : t("entryDetail.conflictShowLatest")}
                        </button>
                        <button
                          type="button"
                          class="ui-button ui-button-secondary ui-button-sm text-xs"
                          onClick={adoptLatestRevisionBase}
                        >
                          {t("entryDetail.conflictAdoptBase")}
                        </button>
                      </div>
                      <Show when={latestLoading()}>
                        <LocalBusyIndicator
                          size="sm"
                          label={t("entryDetail.loading")}
                        />
                      </Show>
                      <Show when={showLatest() && latestEntry()}>
                        {(latest) => (
                          <div class="ui-entry-conflict-compare">
                            <section
                              aria-label={t(
                                "entryDetail.conflictLocalHeading",
                              )}
                            >
                              <h3 class="text-sm font-semibold">
                                {t("entryDetail.conflictLocalHeading")}
                              </h3>
                              <Show
                                when={currentForm()}
                                fallback={
                                  <p class="text-sm whitespace-pre-wrap">
                                    {editorContent()}
                                  </p>
                                }
                              >
                                {(entryForm) => (
                                  <FieldValuesView
                                    fields={Object.keys(
                                      entryForm().fields || {},
                                    ).map((name) => ({ name }))}
                                    getValue={(name) =>
                                      draftValueToDisplayString(
                                        draftFields()[name],
                                      )}
                                  />
                                )}
                              </Show>
                            </section>
                            <section
                              aria-label={t(
                                "entryDetail.conflictLatestHeading",
                              )}
                            >
                              <h3 class="text-sm font-semibold">
                                {t("entryDetail.conflictLatestHeading")}
                              </h3>
                              <FieldValuesView
                                fields={Object.keys(
                                  latest().sections ?? {},
                                ).map((name) => ({ name }))}
                                getValue={(name) =>
                                  latest().sections?.[name] ?? ""}
                              />
                            </section>
                          </div>
                        )}
                      </Show>
                    </div>
                  )}
                </Show>
              </div>
            </Show>

            <div class="ui-entry-workspace">
              <main class="ui-entry-main">
                <Show
                  when={currentForm()}
                  fallback={
                    <div class="ui-card">
                      <p class="text-sm ui-muted">
                        {t("entryDetail.noFields")}
                      </p>
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
                        </div>
                      </Show>
                    </div>
                  )}
                </Show>
              </main>
            </div>
          </>
        )}
      </Show>

      <ConfirmDestructiveAction
        open={deleteConfirmOpen()}
        title={t("entryDetail.deleteTitle")}
        body={t("entryDetail.confirmDelete")}
        confirmLabel={t("entryDetail.delete")}
        busy={isDeleting()}
        error={deleteError()}
        onConfirm={() => void handleDelete()}
        onClose={closeDeleteConfirm}
      />
      <ConfirmDestructiveAction
        open={leaveConfirmOpen()}
        title={t("entryDetail.leaveTitle")}
        body={t("entryDetail.confirmLeave")}
        confirmLabel={t("entryDetail.discard")}
        onConfirm={confirmLeave}
        onClose={cancelLeave}
      />
    </div>
  );
  /* v8 ignore stop */
}
