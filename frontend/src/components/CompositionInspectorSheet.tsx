import { onCleanup, onMount, Show } from "solid-js";
import { Portal } from "solid-js/web";
import {
  CompositionInspector,
  type CompositionInspectorDataJump,
} from "~/components/CompositionInspector";
import { IconButton } from "~/components/IconButton";
import type { CompositionFieldNames } from "~/components/CompositionRenderer";
import type { CompositionDraft } from "~/lib/composition-draft";
import { t } from "~/lib/i18n";

export interface CompositionInspectorSheetProps {
  draft: CompositionDraft;
  /** Transient canvas selection; the sheet renders the block only. */
  selectedId: string | null;
  fieldNames?: CompositionFieldNames;
  fieldProjectable?: (formId: string, fieldId: number) => boolean | undefined;
  /** Single draft mutation channel shared with the canvas. */
  onDraftChange: (draft: CompositionDraft) => void;
  onDataJump: (jump: CompositionInspectorDataJump) => void;
  onClose: () => void;
}

const FOCUSABLE_SELECTOR =
  'button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), a[href], [tabindex]:not([tabindex="-1"])';

/**
 * Narrow-viewport bottom sheet for the selected canvas block. The sheet
 * reuses the existing inspector content component without duplicating
 * editors: the same selection identity renders inline on wide viewports and
 * here on narrow ones, never both. Focus moves to the sheet Close control
 * on open; the caller returns focus to the invoking block on close.
 * Backdrop, Escape, and Close all dismiss through onClose, which owns the
 * dismissal state; the sheet itself never mutates draft or selection.
 */
export function CompositionInspectorSheet(
  props: CompositionInspectorSheetProps,
) {
  let dialog: HTMLDivElement | undefined;

  onMount(() => {
    const appRoot = document.getElementById("app");
    appRoot?.setAttribute("inert", "");
    queueMicrotask(() => dialog?.querySelector("button")?.focus());
    onCleanup(() => {
      appRoot?.removeAttribute("inert");
    });
  });

  const handleKeyDown = (event: KeyboardEvent) => {
    if (event.key === "Escape") {
      event.preventDefault();
      props.onClose();
      return;
    }
    if (event.key === "Tab") {
      const container = event.currentTarget as HTMLElement;
      const focusable = Array.from(
        container.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR),
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
    }
  };

  return (
    <Portal>
      <div
        class="ui-backdrop studioSheetBackdrop"
        onClick={(event) => {
          if (event.target === event.currentTarget) props.onClose();
        }}
      >
        <div
          ref={dialog}
          class="ui-dialog studioInspectorSheet"
          role="dialog"
          aria-modal="true"
          aria-labelledby="studio-inspector-heading"
          onKeyDown={handleKeyDown}
        >
          <div class="studioSheetClose">
            <IconButton
              icon="close"
              label={t("common.close")}
              onClick={() => props.onClose()}
            />
          </div>
          <Show when={props.selectedId}>
            <CompositionInspector
              draft={props.draft}
              selectedId={props.selectedId}
              fieldNames={props.fieldNames}
              fieldProjectable={props.fieldProjectable}
              onDraftChange={props.onDraftChange}
              onDataJump={props.onDataJump}
            />
          </Show>
        </div>
      </div>
    </Portal>
  );
}
