import { For } from "solid-js";
import type { JSX } from "solid-js";
import { UiIcon, type UiIconName } from "~/components/UiIcon";

export interface ActionItem {
  id: string;
  icon: UiIconName;
  /** Short fixed label only (e.g. 1-8 chars). Free-form sentences are rejected. */
  label: string;
  href?: string;
  disabled?: boolean;
  danger?: boolean;
  onClick?: JSX.EventHandlerUnion<
    HTMLButtonElement & HTMLAnchorElement,
    MouseEvent
  >;
}

const MAX_SHORT_LABEL = 12;

/**
 * Compact page-action grid. Labels must stay short so tiles keep a uniform
 * 44px touch target and wrap to a 2-row grid on narrow viewports.
 */
export function ActionIconBar(
  props: { actions: ActionItem[]; label?: string },
) {
  for (const action of props.actions) {
    if (action.label.length > MAX_SHORT_LABEL) {
      console.warn(
        `[ActionIconBar] label "${action.label}" exceeds ${MAX_SHORT_LABEL} chars; use a short fixed term.`,
      );
    }
  }
  return (
    <div
      class="actionbar compact-actions"
      role={props.label ? "toolbar" : undefined}
      aria-label={props.label}
    >
      <For each={props.actions}>
        {(action) => <ActionTile action={action} />}
      </For>
    </div>
  );
}

function ActionTile(props: { action: ActionItem }) {
  const cls = () =>
    ["tool", props.action.danger ? "tool-danger" : ""].filter(Boolean).join(
      " ",
    );
  const content = () => (
    <>
      <UiIcon name={props.action.icon} />
      <span class="toolLabel" aria-hidden="true">{props.action.label}</span>
      <span class="ui-sr-only">{props.action.label}</span>
    </>
  );
  if (props.action.href !== undefined) {
    return (
      <a
        class={cls()}
        href={props.action.href}
        aria-label={props.action.label}
        aria-disabled={props.action.disabled}
        onClick={props.action.onClick as JSX.EventHandlerUnion<
          HTMLAnchorElement,
          MouseEvent
        >}
      >
        {content()}
      </a>
    );
  }
  return (
    <button
      class={cls()}
      type="button"
      aria-label={props.action.label}
      disabled={props.action.disabled}
      onClick={props.action.onClick as JSX.EventHandlerUnion<
        HTMLButtonElement,
        MouseEvent
      >}
    >
      {content()}
    </button>
  );
}
