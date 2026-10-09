import { createSignal, createUniqueId, For, Show } from "solid-js";
import { handleDialogKeyDown, useDialogFocus } from "~/components/dialog-focus";
import { t } from "~/lib/i18n";
import { UiIcon } from "./UiIcon";

export interface EntryTableColumnOption {
  key: string;
  label: string;
  selected: boolean;
  disabled?: boolean;
  disabledReason?: string;
  countsTowardSelectionLimit?: boolean;
}

export interface EntryTableColumnPickerProps {
  options: () => readonly EntryTableColumnOption[];
  canApply?: (selectedKeys: readonly string[]) => boolean;
  selectionLimit?: {
    maximum: number;
    reason: string;
    countSelected?: (selectedKeys: readonly string[]) => number;
  };
  onApply: (selectedKeys: string[]) => void;
}

/** Shared, compact column chooser for Form-backed tables. */
export function EntryTableColumnPicker(
  props: EntryTableColumnPickerProps,
) {
  const [open, setOpen] = createSignal(false);
  const [draft, setDraft] = createSignal<string[]>([]);
  const headingId = `entry-table-column-picker-${createUniqueId()}`;
  let trigger: HTMLButtonElement | undefined;
  let dialog: HTMLDivElement | undefined;

  const openDialog = () => {
    setDraft(
      props.options().filter((option) => option.selected).map((o) => o.key),
    );
    setOpen(true);
  };
  const closeDialog = () => {
    setOpen(false);
    trigger?.focus();
  };
  const toggle = (key: string, checked: boolean) =>
    setDraft((current) =>
      checked
        ? current.includes(key) ? current : [...current, key]
        : current.filter((candidate) => candidate !== key)
    );
  const apply = () => {
    if (props.canApply?.(draft()) === false) return;
    const available = new Set(props.options().map((option) => option.key));
    props.onApply(draft().filter((key) => available.has(key)));
    closeDialog();
  };
  const disabled = (option: EntryTableColumnOption) =>
    option.disabled || (
      !draft().includes(option.key) &&
      option.countsTowardSelectionLimit !== false &&
      props.selectionLimit !== undefined &&
      (props.selectionLimit.countSelected?.(draft()) ?? draft().length) >=
        props.selectionLimit.maximum
    );
  const disabledReason = (option: EntryTableColumnOption) =>
    option.disabledReason ?? (
      !draft().includes(option.key) &&
        option.countsTowardSelectionLimit !== false &&
        props.selectionLimit !== undefined &&
        (props.selectionLimit.countSelected?.(draft()) ?? draft().length) >=
          props.selectionLimit.maximum
        ? props.selectionLimit.reason
        : undefined
    );

  useDialogFocus(() => open(), {
    dialog: () => dialog,
    returnFocus: () => trigger ?? null,
    inert: false,
    onClose: closeDialog,
  });

  return (
    <>
      <button
        ref={(element) => trigger = element}
        type="button"
        class="ui-button ui-button-secondary entry-browser-display-button"
        aria-label={t("entryBrowser.columns")}
        aria-haspopup="dialog"
        aria-expanded={open()}
        title={t("entryBrowser.columns")}
        onClick={openDialog}
      >
        <UiIcon name="columns" />
      </button>
      <Show when={open()}>
        <div
          class="ui-backdrop entry-browser-dialog-backdrop"
          onClick={(event) => {
            if (event.target === event.currentTarget) closeDialog();
          }}
        >
          <div
            ref={(element) => dialog = element}
            class="ui-dialog entry-browser-display-dialog"
            role="dialog"
            aria-modal="true"
            aria-labelledby={headingId}
            tabIndex={-1}
            onKeyDown={(event) =>
              handleDialogKeyDown(event, dialog, closeDialog)}
          >
            <header class="ui-dialog-header">
              <h2
                id={headingId}
                class="ui-dialog-title"
              >
                {t("entryBrowser.columns")}
              </h2>
              <button
                type="button"
                class="ui-button ui-button-secondary entry-browser-dialog-close"
                aria-label={t("entryBrowser.closeDialog")}
                onClick={closeDialog}
              >
                <UiIcon name="close" />
              </button>
            </header>
            <div class="entry-browser-dialog-content">
              <ul class="ui-stack-sm">
                <For each={props.options()}>
                  {(option) => (
                    <li>
                      <label class="entry-browser-dialog-option">
                        <input
                          type="checkbox"
                          checked={draft().includes(option.key)}
                          disabled={disabled(option)}
                          title={disabledReason(option)}
                          onChange={(event) =>
                            toggle(option.key, event.currentTarget.checked)}
                        />
                        {option.label}
                      </label>
                    </li>
                  )}
                </For>
              </ul>
            </div>
            <footer class="ui-dialog-actions flex justify-end gap-2">
              <button
                type="button"
                class="ui-button ui-button-secondary"
                onClick={closeDialog}
              >
                {t("common.cancel")}
              </button>
              <button
                type="button"
                class="ui-button ui-button-primary"
                disabled={props.canApply?.(draft()) === false}
                onClick={apply}
              >
                {t("entryBrowser.apply")}
              </button>
            </footer>
          </div>
        </div>
      </Show>
    </>
  );
}
