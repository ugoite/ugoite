import { createUniqueId, type JSX, Show } from "solid-js";
import { Portal } from "solid-js/web";
import { ButtonSpinner } from "~/components/ButtonSpinner";
import { handleDialogKeyDown, useDialogFocus } from "~/components/dialog-focus";
import { t } from "~/lib/i18n";

export interface ConfirmDestructiveActionProps {
  /** Dialog visibility. Mounts the modal only while true. */
  open: boolean;
  /** Dialog heading; also the accessible name. */
  title: string;
  /** Consequence statement; also the accessible description. */
  body: string;
  /** Destructive confirm label (e.g. "Delete entry"). */
  confirmLabel: string;
  /** Safe label. Defaults to Cancel and always receives initial focus. */
  cancelLabel?: string;
  /** While true, both actions disable and Escape/backdrop dismiss is held. */
  busy?: boolean;
  /** Server failure surfaced inside the dialog; the draft stays intact. */
  error?: string | null;
  /** Extra dialog content (e.g. an optional revert message field). */
  children?: JSX.Element;
  onConfirm: () => void;
  onClose: () => void;
}

/**
 * Shared destructive-action confirmation (POL-UI-007, POL-UI-008).
 *
 * Exactly one interactive confirmation pattern for Entry delete, Asset
 * delete, and restore/revert: the safe action (Cancel) receives initial
 * focus, Tab cycles inside the dialog, Escape dismisses to the safe action,
 * the background is inert while open, and focus returns to the invoking
 * control on dismiss. No `window.confirm`/`alert` on the product surface.
 */
export function ConfirmDestructiveAction(
  props: ConfirmDestructiveActionProps,
) {
  const titleId = `confirm-destructive-title-${createUniqueId()}`;
  const bodyId = `confirm-destructive-body-${createUniqueId()}`;
  let dialogRef: HTMLDivElement | undefined;
  let cancelRef: HTMLButtonElement | undefined;

  const busy = () => props.busy ?? false;

  const close = () => {
    if (busy()) return;
    props.onClose();
  };

  // Cancel is the default: focus lands on the safe action first.
  useDialogFocus(() => props.open, {
    dialog: () => dialogRef,
    initialFocus: () => cancelRef,
    onClose: close,
  });

  const handleKeyDown = (event: KeyboardEvent) =>
    handleDialogKeyDown(event, dialogRef, close);

  return (
    <Portal>
      <Show when={props.open}>
        <div
          class="ui-backdrop"
          onClick={(event) => {
            if (event.target === event.currentTarget) close();
          }}
        >
          <div
            ref={dialogRef}
            class="ui-dialog ui-confirm-destructive-dialog"
            role="dialog"
            aria-modal="true"
            aria-labelledby={titleId}
            aria-describedby={bodyId}
            onKeyDown={handleKeyDown}
          >
            <h2 id={titleId} class="ui-dialog-title">
              {props.title}
            </h2>
            <p id={bodyId} class="ui-confirm-destructive-body">
              {props.body}
            </p>
            {props.children}
            <div class="ui-dialog-actions">
              <button
                ref={cancelRef}
                type="button"
                class="ui-button ui-button-secondary"
                disabled={busy()}
                onClick={close}
              >
                {props.cancelLabel ?? t("common.cancel")}
              </button>
              <button
                type="button"
                class="ui-button ui-button-primary ui-button-danger"
                aria-busy={busy() || undefined}
                disabled={busy()}
                onClick={props.onConfirm}
              >
                <Show when={busy()}>
                  <ButtonSpinner />
                </Show>
                {props.confirmLabel}
              </button>
            </div>
            <Show when={props.error}>
              <p class="ui-alert ui-alert-error mt-3" role="alert">
                {props.error}
              </p>
            </Show>
          </div>
        </div>
      </Show>
    </Portal>
  );
}
