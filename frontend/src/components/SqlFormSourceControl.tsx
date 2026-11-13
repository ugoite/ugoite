import { createSignal, createUniqueId, For, onCleanup, Show } from "solid-js";
import { IconButton } from "~/components/IconButton";
import { handleDialogKeyDown, useDialogFocus } from "~/components/dialog-focus";
import { isReservedMetadataForm } from "~/lib/metadata-forms";
import { compositionFormQuerySql } from "~/lib/ugoite-client";
import { UgoiteApiError } from "~/lib/ugoite-client/protocol";
import type { Form } from "~/lib/types";
import { t } from "~/lib/i18n";

interface SqlFormSourceControlProps {
  sql: () => string;
  scope?: () => string;
  forms: () => readonly Form[];
  formBindingGuards?: () => readonly { name: string; formId: string }[];
  onFormBindingSelected: (
    selection: { sql: string; name: string; form_id: string },
  ) => void;
  disabled?: () => boolean;
}

/** Explicit Form starter for a blank SQL draft; Rust owns its SQL projection. */
export function SqlFormSourceControl(props: SqlFormSourceControlProps) {
  const [open, setOpen] = createSignal(false);
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string>();
  const [returnFocus, setReturnFocus] = createSignal<HTMLElement | null>(null);
  const dialogTitleId = `sql-form-source-title-${createUniqueId()}`;
  let dialog: HTMLDivElement | undefined;
  let generation = 0;
  onCleanup(() => generation++);

  const forms = () => {
    const seen = new Set<string>();
    const guards = props.formBindingGuards?.() ?? [];
    return props.forms().flatMap((form) => {
      const relation = form.name.trim();
      const stableRelation = form.sql_relation?.trim();
      const guard = guards.find((candidate) => candidate.name === relation);
      if (
        !form.id || !stableRelation || !relation ||
        isReservedMetadataForm(relation) || seen.has(relation) ||
        guard && guard.formId !== form.id
      ) {
        return [];
      }
      seen.add(relation);
      return [{
        relation,
        formId: form.id,
        columns: Object.entries(form.fields ?? {}).flatMap(([name, field]) => {
          const column = field.sql_column?.trim();
          if (
            !column || column.startsWith("_ugoite_") ||
            field.query_capability?.projectable !== true
          ) return [];
          return [{ column, label: field.label?.trim() || name }];
        }),
      }];
    });
  };
  const disabled = () => Boolean(
    props.disabled?.() || props.sql().trim() !== "SELECT 1" ||
      forms().length === 0,
  );
  const close = () => {
    generation++;
    setBusy(false);
    setOpen(false);
  };
  const selectForm = async (form: ReturnType<typeof forms>[number]) => {
    const source = props.sql();
    const scope = props.scope?.();
    if (busy() || disabled()) return;
    const request = ++generation;
    setBusy(true);
    setError(undefined);
    try {
      const { sql } = await compositionFormQuerySql(form.relation, form.columns);
      if (
        request !== generation || props.sql() !== source ||
        props.scope?.() !== scope || disabled()
      ) return;
      props.onFormBindingSelected({
        sql,
        name: form.relation,
        form_id: form.formId,
      });
      close();
    } catch (cause) {
      if (request !== generation) return;
      setError(
        cause instanceof UgoiteApiError &&
            cause.code === "form_no_projectable_fields"
          ? t("composition.formNoProjectableFields")
          : t("sqlPage.structure.documentEditUnavailable"),
      );
    } finally {
      if (request === generation) setBusy(false);
    }
  };
  useDialogFocus(() => open(), {
    dialog: () => dialog,
    returnFocus,
    inert: false,
    onClose: close,
  });

  return (
    <>
      <div class="sql-workspace__action-bar">
        <div
          class="sql-workspace__actions"
          role="group"
          aria-label={t("sqlPage.sql")}
        >
          <IconButton
            icon="forms"
            label={t("sqlPage.structure.form")}
            title={t("sqlPage.structure.form")}
            disabled={disabled()}
            onClick={(event) => {
              setReturnFocus(event.currentTarget);
              setError(undefined);
              setOpen(true);
            }}
          />
        </div>
      </div>
      <Show when={open()}>
        <div
          class="ui-backdrop sql-structure-backdrop"
          onClick={(event) => {
            if (event.target === event.currentTarget) close();
          }}
        >
          <div
            ref={(element) => dialog = element}
            class="ui-dialog sql-structure-dialog"
            role="dialog"
            aria-modal="true"
            aria-labelledby={dialogTitleId}
            tabIndex={-1}
            onKeyDown={(event) => handleDialogKeyDown(event, dialog, close)}
          >
            <header class="ui-dialog-header">
              <h2 id={dialogTitleId} class="ui-dialog-title">
                {t("sqlPage.structure.form")}
              </h2>
              <IconButton
                icon="close"
                label={t("common.close")}
                onClick={close}
              />
            </header>
            <div class="sql-structure-dialog-content">
              <div
                class="sql-form-picker__list"
                role="group"
                aria-label={t("sqlPage.structure.availableForms")}
              >
                <For each={forms()}>
                  {(form) => (
                    <button
                      type="button"
                      class="sql-form-picker__option ui-button ui-button-secondary"
                      disabled={busy() || props.disabled?.()}
                      onClick={() => void selectForm(form)}
                    >
                      {form.relation}
                    </button>
                  )}
                </For>
              </div>
              <Show when={error()}>
                <p role="alert" class="ui-text-danger">{error()}</p>
              </Show>
            </div>
          </div>
        </div>
      </Show>
    </>
  );
}
