import { createSignal, Show } from "solid-js";
import { EntryBrowser } from "~/components/EntryBrowser";
import {
  createEntryQueryController,
  type EntryQueryCapabilities,
  type EntryQueryResult,
  systemEntryCapabilities,
} from "~/lib/entry-query";
import { t } from "~/lib/i18n";
import type { Form } from "~/lib/types";

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

/**
 * Canonical Row Reference picker.
 *
 * The dialog owns a disposable EntryQuery controller. Selecting a row only
 * updates dialog state; the field draft receives the stable entry id on
 * confirm. Cancel never mutates the parent form.
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

  const openPicker = () => {
    setPending(null);
    setOpen(true);
    void controller.invalidate();
  };

  const cancel = () => {
    setPending(null);
    setOpen(false);
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

  const humanLabel = () => {
    const row = selectedRow();
    if (row?.preview?.trim()) return row.preview.trim();
    return props.value ? t("createDialog.entry.rowReference.selected") : "";
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
            {humanLabel() || t("createDialog.entry.rowReference.noneSelected")}
          </span>
          <button
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
      <Show when={open()}>
        <div
          class="ui-backdrop"
          role="presentation"
          onClick={(event) => {
            if (event.target === event.currentTarget) cancel();
          }}
        >
          <div
            class="ui-dialog ui-row-reference-dialog ui-stack"
            role="dialog"
            aria-modal="true"
            aria-labelledby={`${props.fieldId}-picker-title`}
          >
            <div class="flex items-center justify-between gap-2">
              <h2 id={`${props.fieldId}-picker-title`}>
                {t("createDialog.entry.rowReference.selectFor", {
                  form: form().name,
                })}
              </h2>
              <button
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
              onSelect={setPending}
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
