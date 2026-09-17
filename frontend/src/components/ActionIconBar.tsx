import { For, Show } from "solid-js";
import type { JSX } from "solid-js";
import { UiIcon, type UiIconName } from "~/components/UiIcon";

export interface ActionItem {
  /** Stable identity. `id` (PR2) and `key` (PR3) are aliases. */
  id?: string;
  key?: string;
  icon?: UiIconName;
  /** Short visible label. */
  label: string;
  /** Long accessible name (existing i18n string). Falls back to `label`. */
  accessibleName?: string;
  href?: string;
  disabled?: boolean;
  danger?: boolean;
  /** Extra classes appended to the rendered tool (legacy hooks). */
  class?: string;
  onClick?:
    | JSX.EventHandlerUnion<
      HTMLButtonElement & HTMLAnchorElement,
      MouseEvent
    >
    | (() => void);
}

/** PR3 alias kept for callers written against the stash version. */
export type ActionBarItem = ActionItem;

export interface ActionIconBarProps {
  /** Accessible name for the toolbar (existing i18n string). */
  label?: string;
  /** PR2 prop name. */
  actions?: ActionItem[];
  /** PR3 prop name (alias for `actions`). */
  items?: ActionItem[];
  /** Extra classes appended to the bar (legacy hooks). */
  class?: string;
}

/**
 * Shared compact action strip. Primary actions stay in the page header;
 * secondary actions live here with short visible labels, 44px targets, and
 * long accessible names. The bar never wraps; only table wrappers scroll.
 * A nav target that is unavailable is omitted (never a disabled `<a>`);
 * a disabled action renders a true disabled `<button>`.
 */
export function ActionIconBar(props: ActionIconBarProps) {
  const list = () => props.items ?? props.actions ?? [];
  const barClass = () =>
    `actionbar compact-actions${props.class ? ` ${props.class}` : ""}`;
  return (
    <div
      class={barClass()}
      role={props.label ? "toolbar" : undefined}
      aria-label={props.label}
    >
      <For each={list()}>
        {(action) => <ActionTile action={action} />}
      </For>
    </div>
  );
}

function ActionTile(props: { action: ActionItem }) {
  const accessibleName = () =>
    props.action.accessibleName ?? props.action.label;
  const cls = () =>
    ["tool", props.action.class ?? ""].filter(Boolean).join(" ");
  const content = () => (
    <>
      <Show when={props.action.icon}>
        {(icon) => <UiIcon name={icon()} />}
      </Show>
      <span class="toolLabel">{props.action.label}</span>
    </>
  );
  if (props.action.href !== undefined) {
    // Nav target unavailable: omit rather than rendering a disabled link.
    if (props.action.disabled) return null;
    return (
      <a
        class={cls()}
        classList={{ "tool-danger": props.action.danger }}
        href={props.action.href}
        title={accessibleName()}
        aria-label={accessibleName()}
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
      classList={{ "tool-danger": props.action.danger }}
      type="button"
      title={accessibleName()}
      aria-label={accessibleName()}
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
