import {
  createEffect,
  createUniqueId,
  type JSX,
  onCleanup,
  Show,
} from "solid-js";
import { ButtonSpinner } from "~/components/ButtonSpinner";
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
  let opener: Element | null = null;

  const busy = () => props.busy ?? false;

  createEffect(() => {
    if (!props.open) return;
    opener = document.activeElement instanceof Element
      ? document.activeElement
      : null;
    // Cancel is the default: focus lands on the safe action first.
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

  const close = () => {
    if (busy()) return;
    props.onClose();
  };

  const handleKeyDown = (event: KeyboardEvent) => {
    if (event.key === "Escape") {
      event.preventDefault();
      close();
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
  );
}
