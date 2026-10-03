import { createUniqueId, onCleanup, onMount, Show } from "solid-js";
import { Portal } from "solid-js/web";
import { ButtonSpinner } from "~/components/ButtonSpinner";
import { t } from "~/lib/i18n";

export interface SaveAsToolDialogProps {
  initialName: string;
  busy: boolean;
  retryAvailable: boolean;
  error: string | null;
  onSave: (name: string) => void;
  onRetry: () => void;
  onClose: () => void;
}

export function SaveAsToolDialog(props: SaveAsToolDialogProps) {
  const titleId = `save-as-tool-title-${createUniqueId()}`;
  const nameId = `save-as-tool-name-${createUniqueId()}`;
  let dialog: HTMLDivElement | undefined;
  let nameInput: HTMLInputElement | undefined;
  let opener: HTMLElement | null = null;

  onMount(() => {
    opener = document.activeElement instanceof HTMLElement
      ? document.activeElement
      : null;
    const appRoot = document.getElementById("app");
    appRoot?.setAttribute("inert", "");
    queueMicrotask(() => nameInput?.focus());
    onCleanup(() => {
      appRoot?.removeAttribute("inert");
      const target = opener;
      opener = null;
      queueMicrotask(() => {
        if (target?.isConnected) target.focus();
      });
    });
  });

  const locked = () => props.busy || props.retryAvailable;
  const close = () => {
    if (!locked()) props.onClose();
  };

  const handleKeyDown = (event: KeyboardEvent) => {
    if (event.key === "Escape") {
      event.preventDefault();
      close();
      return;
    }
    if (event.key !== "Tab" || !dialog) return;
    const focusable = Array.from(
      dialog.querySelectorAll<HTMLElement>(
        "button:not([disabled]), input:not([disabled])",
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

  const handleSubmit = (event: SubmitEvent) => {
    event.preventDefault();
    if (props.busy) return;
    if (props.retryAvailable) {
      props.onRetry();
      return;
    }
    const form = event.currentTarget as HTMLFormElement;
    if (!form.reportValidity()) return;
    props.onSave(nameInput?.value ?? "");
  };

  return (
    <Portal>
      <div
        class="ui-backdrop"
        onClick={(event) => {
          if (event.target === event.currentTarget) close();
        }}
      >
        <div
          ref={dialog}
          class="ui-dialog composition-save-dialog"
          role="dialog"
          aria-modal="true"
          aria-labelledby={titleId}
          onKeyDown={handleKeyDown}
        >
          <h2 id={titleId} class="ui-dialog-title">
            {t("composition.saveAsTool")}
          </h2>
          <form class="ui-stack-sm" onSubmit={handleSubmit}>
            <label class="ui-label" for={nameId}>{t("composition.name")}</label>
            <input
              ref={nameInput}
              id={nameId}
              class="ui-input"
              name="name"
              value={props.initialName}
              required
              disabled={locked()}
            />
            <Show when={props.error}>
              <p class="ui-alert ui-alert-error" role="alert">
                {props.error}
              </p>
            </Show>
            <div class="ui-dialog-actions">
              <Show when={!props.retryAvailable}>
                <button
                  type="button"
                  class="ui-button ui-button-secondary"
                  disabled={props.busy}
                  onClick={close}
                >
                  {t("common.cancel")}
                </button>
              </Show>
              <button
                type="submit"
                class="ui-button ui-button-primary"
                aria-busy={props.busy || undefined}
                disabled={props.busy}
              >
                <Show when={props.busy}>
                  <ButtonSpinner />
                </Show>
                {props.retryAvailable
                  ? t("composition.retrySave")
                  : t("composition.save")}
              </button>
            </div>
          </form>
        </div>
      </div>
    </Portal>
  );
}
