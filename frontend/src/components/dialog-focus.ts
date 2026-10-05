import { createEffect, onCleanup } from "solid-js";

/**
 * Shared dialog focus primitive (extracted from ConfirmDestructiveAction).
 *
 * One focus-trap/inert/focus-return pattern for every dialog: the safe
 * action receives initial focus, Tab cycles inside the dialog, Escape
 * dismisses, the background is inert while open (Portal dialogs), and focus
 * returns to the invoking control on dismiss.
 */

export const DIALOG_FOCUSABLE_SELECTOR =
  'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

export function focusableDialogItems(root: HTMLElement): HTMLElement[] {
  return Array.from(
    root.querySelectorAll<HTMLElement>(DIALOG_FOCUSABLE_SELECTOR),
  );
}

/**
 * Cycle Tab inside the dialog; Escape invokes `onClose`. Callers keep their
 * own guards (e.g. holding Escape/backdrop dismiss while busy) inside
 * `onClose`.
 */
export function handleDialogKeyDown(
  event: KeyboardEvent,
  root: HTMLElement | undefined,
  onClose: () => void,
): void {
  if (event.key === "Escape") {
    event.preventDefault();
    onClose();
    return;
  }
  if (event.key !== "Tab" || !root) return;
  const focusable = focusableDialogItems(root);
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
}

export interface DialogFocusOptions {
  /** Dialog root for Tab cycling. */
  dialog: () => HTMLElement | undefined;
  /** Element receiving initial focus; defaults to the first focusable. */
  initialFocus?: () => HTMLElement | null | undefined;
  /**
   * Explicit focus-return target. Defaults to the opener captured from
   * `document.activeElement` when the dialog opens.
   */
  returnFocus?: () => HTMLElement | null | undefined;
  /**
   * Mark the app root inert while open. Portal dialogs leave this true;
   * inline dialogs rendered inside the app root must pass false.
   */
  inert?: boolean;
  onClose: () => void;
}

/** Run the shared focus lifecycle while `open()` is true. */
export function useDialogFocus(
  open: () => boolean,
  options: DialogFocusOptions,
): void {
  createEffect(() => {
    if (!open()) return;
    const opener = document.activeElement instanceof Element
      ? document.activeElement
      : null;
    const root = options.dialog();
    const initial = options.initialFocus?.() ??
      (root ? focusableDialogItems(root)[0] : undefined);
    queueMicrotask(() => initial?.focus());
    let appRoot: HTMLElement | null = null;
    if (options.inert !== false) {
      appRoot = document.getElementById("app");
      appRoot?.setAttribute("inert", "");
    }
    onCleanup(() => {
      appRoot?.removeAttribute("inert");
      const fallback = options.returnFocus?.();
      const target = fallback ??
        (opener instanceof HTMLElement ? opener : null);
      // Return focus to the invoking control on dismiss.
      queueMicrotask(() => {
        if (target?.isConnected) target.focus();
      });
    });
  });
}
