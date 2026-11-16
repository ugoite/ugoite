import { For, onCleanup, onMount, Show } from "solid-js";
import { Portal } from "solid-js/web";
import { IconButton } from "~/components/IconButton";
import { UiIcon } from "~/components/UiIcon";
import {
  bindCompositionComponent,
  compositionNodeReturnsTable,
  setCompositionComponentVariable,
  type CompositionGraphDraft,
} from "~/lib/composition-variable-binding";
import { t } from "~/lib/i18n";

const focusableSelector =
  'button:not([disabled]), [tabindex]:not([tabindex="-1"])';

/** Select one SQL table value for a Table component. */
export function CompositionVariableBindingPicker(props: {
  draft: CompositionGraphDraft;
  componentId?: string;
  onCommit: (draft: CompositionGraphDraft) => void;
  onCreateQuery?: () => void;
  onClose: () => void;
}) {
  let dialog: HTMLDivElement | undefined;
  let opener: HTMLElement | null = null;

  onMount(() => {
    opener = document.activeElement instanceof HTMLElement
      ? document.activeElement
      : null;
    document.getElementById("app")?.setAttribute("inert", "");
    queueMicrotask(() => dialog?.querySelector<HTMLButtonElement>("button")
      ?.focus());
    onCleanup(() => {
      document.getElementById("app")?.removeAttribute("inert");
      const target = opener;
      opener = null;
      queueMicrotask(() => target?.isConnected && target.focus());
    });
  });

  const tables = () =>
    props.draft.variables.filter(compositionNodeReturnsTable);

  const commit = (variableId: string) => {
    const next = props.componentId
      ? setCompositionComponentVariable(
        props.draft,
        props.componentId,
        variableId,
      )
      : bindCompositionComponent(props.draft, "table", variableId);
    if (!next) return;
    props.onCommit(next);
    props.onClose();
  };

  return (
    <Portal>
      <div class="ui-dialog-backdrop" onClick={props.onClose}>
        <div
          ref={(element) => dialog = element}
          class="ui-dialog compositionBindingPicker"
          role="dialog"
          aria-modal="true"
          aria-labelledby="composition-table-picker-title"
          tabIndex={-1}
          onClick={(event) => event.stopPropagation()}
          onKeyDown={(event) => {
            if (event.key === "Escape") {
              event.preventDefault();
              props.onClose();
              return;
            }
            if (event.key !== "Tab") return;
            const controls = Array.from(
              (event.currentTarget as HTMLElement)
                .querySelectorAll<HTMLElement>(focusableSelector),
            ).filter((control) =>
              !control.closest("[hidden], [inert], [aria-hidden='true']")
            );
            if (controls.length === 0) {
              event.preventDefault();
              return;
            }
            const current = controls.indexOf(
              document.activeElement as HTMLElement,
            );
            const next = event.shiftKey
              ? current <= 0 ? controls.length - 1 : current - 1
              : current < 0 || current === controls.length - 1 ? 0 : current + 1;
            event.preventDefault();
            controls[next].focus();
          }}
        >
          <div class="compositionBindingPickerHeader">
            <h2 id="composition-table-picker-title">
              {t("composition.studioTable")}
            </h2>
            <button
              type="button"
              class="pill iconpill icononly"
              aria-label={t("common.close")}
              onClick={props.onClose}
            >
              <UiIcon name="close" />
              <span class="ui-sr-only">{t("common.close")}</span>
            </button>
          </div>
          <Show when={tables().length > 0}>
            <div class="compositionBindingTableList" role="list">
              <For each={tables()}>
                {(node) => (
                  <div role="listitem">
                    <button
                      type="button"
                      aria-label={node.id}
                      aria-pressed={props.draft.components.some((component) =>
                        component.id === props.componentId &&
                        component.kind === "table" &&
                        component.variable === node.id
                      )}
                      onClick={() => commit(node.id)}
                    >
                      <UiIcon name="canvas-table" />
                      <span>{node.id}</span>
                      <UiIcon name="chevron-right" />
                    </button>
                  </div>
                )}
              </For>
            </div>
          </Show>
          <Show when={props.onCreateQuery}>
            <IconButton
              icon="plus"
              label={t("composition.studioCreateTableQuery")}
              onClick={() => props.onCreateQuery?.()}
            />
          </Show>
        </div>
      </div>
    </Portal>
  );
}
