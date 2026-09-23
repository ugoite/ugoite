import {
  createEffect,
  createResource,
  createSignal,
  onCleanup,
  Show,
} from "solid-js";
import { Portal } from "solid-js/web";
import { EntryBrowser } from "~/components/EntryBrowser";
import {
  buildRowReferencePreview,
  rowReferenceTargetMatches,
} from "~/components/fields/row-reference";
import {
  createEntryQueryController,
  type EntryQueryCapabilities,
  type EntryQueryResult,
  systemEntryCapabilities,
} from "~/lib/entry-query";
import { entryApi } from "~/lib/ugoite-client";
import { t } from "~/lib/i18n";
import type { Entry, Form } from "~/lib/types";

export interface RowReferenceSelectProps {
  spaceId: string;
  targetForm: string;
  /** Stable entry id. The human-facing label is resolved from the Entry. */
  value: string;
  onChange: (id: string) => void;
  fieldId: string;
  invalid?: boolean;
  describedBy?: string;
  /** Loaded Form catalog. Stable Form ids enable the canonical EntryQuery path. */
  forms?: readonly Form[];
  /** Kept for form-level validation hooks; selection itself is confirmed in the dialog. */
  onPendingChange?: (pending: boolean) => void;
}

const targetFormDefinition = (props: RowReferenceSelectProps) => {
  const target = props.targetForm.trim();
  return props.forms?.find((form) =>
    form.id === target || form.name === target
  );
};

const canonicalCapabilities = (
  form: Form,
): EntryQueryCapabilities => {
  const scope = { kind: "form" as const, form_id: form.id! };
  const system = systemEntryCapabilities(scope);
  const fields = Object.values(form.fields)
    .map((field) => field.query_capability)
    .filter((field): field is NonNullable<typeof field> => field !== undefined);
  return { scope, fields: [...system.fields, ...fields] };
};

type ReferencedEntryState =
  | { kind: "empty" }
  | { kind: "loading" }
  | { kind: "ready"; preview: string }
  | { kind: "wrong-form"; actual: string }
  | { kind: "unavailable" };

/**
 * Canonical Row Reference picker.
 *
 * Existing-value display is a point read: the stored stable entry id is
 * resolved through an authorized Entry get, the returned Entry's Form is
 * checked against the target Form, and the human Preview is derived from
 * the Entry's structured fields. The displayed label never depends on the
 * candidate list page position.
 *
 * Candidate selection stays list-driven: the dialog owns a disposable
 * EntryQuery controller, selecting a row only updates dialog state, and
 * the field draft receives the stable entry id on confirm. Cancel never
 * mutates the parent form.
 *
 * Dialog behavior follows the shared confirmation pattern
 * (ConfirmDestructiveAction): Portal rendering, Escape dismissal to the
 * safe action, initial focus on Cancel, Tab cycling inside the dialog,
 * an inert background while open, and focus return to the trigger.
 */
function CanonicalRowReferenceSelect(props: RowReferenceSelectProps) {
  const form = () => targetFormDefinition(props)!;
  const scope = () => ({ kind: "form" as const, form_id: form().id! });
  const controller = createEntryQueryController(
    () => props.spaceId,
    { scope: scope(), filters: [], sort: [] },
  );
  const capabilities = () => canonicalCapabilities(form());
  const [open, setOpen] = createSignal(false);
  const [pending, setPending] = createSignal<EntryQueryResult | null>(null);
  const [selectedRow, setSelectedRow] = createSignal<EntryQueryResult | null>(
    null,
  );
  let triggerRef: HTMLButtonElement | undefined;
  let dialogRef: HTMLDivElement | undefined;
  let cancelRef: HTMLButtonElement | undefined;
  let opener: HTMLElement | null = null;

  const storedId = () => props.value.trim();

  const [referencedEntry] = createResource(
    () => storedId() ? `${props.spaceId}::${storedId()}` : null,
    async (): Promise<Entry> => await entryApi.get(props.spaceId, storedId()),
  );

  const referencedState = (): ReferencedEntryState => {
    if (!storedId()) return { kind: "empty" };
    if (referencedEntry.loading) return { kind: "loading" };
    // Read .error before the value accessor: Solid re-throws a rejected
    // resource's error from the value read, so the value is only safe to
    // touch when no error is present.
    if (referencedEntry.error) return { kind: "unavailable" };
    const entry = referencedEntry();
    if (!entry) return { kind: "unavailable" };
    const target = form();
    if (!rowReferenceTargetMatches(entry.form, target)) {
      return {
        kind: "wrong-form",
        actual: typeof entry.form === "string" && entry.form.trim() !== ""
          ? entry.form.trim()
          : target.name,
      };
    }
    return {
      kind: "ready",
      preview: buildRowReferencePreview(entry.fields ?? {}, target),
    };
  };

  /** Optimistic label from the just-confirmed row; ids must match. */
  const optimisticLabel = () => {
    const row = selectedRow();
    if (row && row.id === storedId() && row.preview?.trim()) {
      return row.preview.trim();
    }
    return "";
  };

  /**
   * Human display text for the stored value. Raw entry ids never surface:
   * point-read Preview first, then the just-confirmed row Preview, then a
   * safe generic or unavailable state.
   */
  const displayText = () => {
    const state = referencedState();
    if (state.kind === "wrong-form") {
      return t("createDialog.entry.rowReference.wrongFormReference", {
        actual: state.actual,
        form: form().name,
      });
    }
    if (state.kind === "unavailable") {
      return t("createDialog.entry.rowReference.unavailableReference");
    }
    const optimistic = optimisticLabel();
    if (optimistic) return optimistic;
    if (state.kind === "ready") {
      if (state.preview.trim()) return state.preview.trim();
      return t("createDialog.entry.rowReference.selected");
    }
    if (state.kind === "loading") {
      return t("createDialog.entry.rowReference.loadingReference");
    }
    return t("createDialog.entry.rowReference.noneSelected");
  };

  const openPicker = () => {
    opener = triggerRef ?? null;
    setPending(null);
    setOpen(true);
    void controller.invalidate().then(() => {
      if (!props.value) return;
      const existing = controller.rows().find((row) => row.id === props.value);
      if (existing) setSelectedRow(existing);
    });
  };

  const closePicker = () => {
    setPending(null);
    setOpen(false);
    props.onPendingChange?.(false);
  };

  const cancel = () => {
    closePicker();
  };

  const confirm = () => {
    const row = pending();
    if (!row) return;
    setSelectedRow(row);
    props.onChange(row.id);
    props.onPendingChange?.(false);
    setPending(null);
    setOpen(false);
  };

  const clear = () => {
    setSelectedRow(null);
    props.onChange("");
    props.onPendingChange?.(false);
  };

  const selectPending = (row: EntryQueryResult) => {
    setPending(row);
    props.onPendingChange?.(true);
  };

  onCleanup(() => {
    props.onPendingChange?.(false);
  });

  createEffect(() => {
    if (!open()) return;
    // Cancel is the safe action: focus lands there first, Tab cycles
    // inside the dialog, and the background stays inert while open.
    queueMicrotask(() => cancelRef?.focus());
    const appRoot = document.getElementById("app");
    appRoot?.setAttribute("inert", "");
    onCleanup(() => {
      appRoot?.removeAttribute("inert");
      const target = opener instanceof HTMLElement ? opener : null;
      opener = null;
      // Return focus to the invoking control on dismiss.
      queueMicrotask(() => {
        if (target?.isConnected) target.focus();
      });
    });
  });

  const handleDialogKeyDown = (event: KeyboardEvent) => {
    if (event.key === "Escape") {
      event.preventDefault();
      cancel();
      return;
    }
    if (event.key !== "Tab" || !dialogRef) return;
    const focusable = Array.from(
      dialogRef.querySelectorAll<HTMLElement>(
        'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])',
      ),
    );
    if (focusable.length === 0) {
      event.preventDefault();
      return;
    }
    const currentIndex = focusable.indexOf(
      document.activeElement as HTMLElement,
    );
    const nextIndex = event.shiftKey
      ? currentIndex <= 0 ? focusable.length - 1 : currentIndex - 1
      : currentIndex < 0 || currentIndex === focusable.length - 1
      ? 0
      : currentIndex + 1;
    event.preventDefault();
    focusable[nextIndex].focus();
  };

  return (
    <>
      <div
        class="ui-stack-sm"
        data-row-reference-picker="entry-query"
        data-testid="row-reference-picker"
        data-target-form={form().id}
      >
        <div class="flex items-center gap-2">
          <span class="ui-input flex-1" aria-live="polite">
            {displayText()}
          </span>
          <button
            ref={(element) => {
              triggerRef = element;
            }}
            type="button"
            class="ui-button ui-button-secondary"
            onClick={openPicker}
            aria-haspopup="dialog"
          >
            {t("createDialog.entry.rowReference.select")}
          </button>
          <Show when={props.value}>
            <button
              type="button"
              class="ui-button ui-button-secondary"
              onClick={clear}
            >
              {t("createDialog.entry.rowReference.clear")}
            </button>
          </Show>
        </div>
      </div>
      <Portal>
        <Show when={open()}>
          <div
            class="ui-backdrop"
            role="presentation"
            onClick={(event) => {
              if (event.target === event.currentTarget) cancel();
            }}
          >
            <div
              ref={(element) => {
                dialogRef = element;
              }}
              class="ui-dialog ui-row-reference-dialog ui-stack"
              role="dialog"
              aria-modal="true"
              aria-labelledby={`${props.fieldId}-picker-title`}
              onKeyDown={handleDialogKeyDown}
            >
              <div class="flex items-center justify-between gap-2">
                <h2 id={`${props.fieldId}-picker-title`}>
                  {t("createDialog.entry.rowReference.selectFor", {
                    form: form().name,
                  })}
                </h2>
                <button
                  ref={(element) => {
                    cancelRef = element;
                  }}
                  type="button"
                  class="ui-button ui-button-secondary"
                  onClick={cancel}
                >
                  {t("common.cancel")}
                </button>
              </div>
              <EntryBrowser
                mode="select_one"
                controller={controller}
                capabilities={capabilities()}
                formLabels={{ [form().id!]: form().name }}
                onSelect={selectPending}
              />
              <div class="flex justify-end gap-2">
                <button
                  type="button"
                  class="ui-button ui-button-secondary"
                  onClick={cancel}
                >
                  {t("common.cancel")}
                </button>
                <button
                  type="button"
                  class="ui-button ui-button-primary"
                  disabled={!pending()}
                  onClick={confirm}
                >
                  {t("common.confirm")}
                </button>
              </div>
            </div>
          </div>
        </Show>
      </Portal>
    </>
  );
}

function UnavailableRowReferenceSelect(props: RowReferenceSelectProps) {
  return (
    <div class="ui-stack-sm" data-row-reference-picker="unavailable">
      <span
        id={props.fieldId}
        class="ui-input"
        aria-live="polite"
        aria-invalid={props.invalid ? "true" : undefined}
        aria-describedby={props.describedBy}
      >
        {props.value
          ? t("createDialog.entry.rowReference.selected")
          : t("createDialog.entry.rowReference.noneSelected")}
      </span>
      <p class="text-xs ui-text-danger" role="alert">
        {t("createDialog.entry.rowReference.targetUnavailable", {
          form: props.targetForm.trim(),
        })}
      </p>
    </div>
  );
}

/** Shared row-reference control for create and edit. */
export function RowReferenceSelect(props: RowReferenceSelectProps) {
  const canonical = () => Boolean(targetFormDefinition(props)?.id);
  return (
    <Show
      when={canonical()}
      fallback={<UnavailableRowReferenceSelect {...props} />}
    >
      <CanonicalRowReferenceSelect {...props} />
    </Show>
  );
}
