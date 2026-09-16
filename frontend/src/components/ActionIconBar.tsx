import { For, Show } from "solid-js";
import type { JSX } from "solid-js";
import { UiIcon, type UiIconName } from "~/components/UiIcon";

export interface ActionItem {
  /** Stable identity. `id` (PR2) and `key` (PR3) are aliases. */
  id?: string;
  key?: string;
  icon?: UiIconName;
  /** Short visible label only (e.g. 1-8 chars). Free-form sentences are rejected. */
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

const MAX_SHORT_LABEL = 12;

/**
 * Shared compact action strip (PR2 contract, PR3 capabilities).
 * Primary actions stay in the page header; secondary actions live here with
 * short visible labels, 44px targets, and long accessible names. Desktop is
 * governed by `.actionbar.compact-actions` in app.css; mobile collapses to
 * an even 2-column grid.
 */
export function ActionIconBar(props: ActionIconBarProps) {
  const list = () => props.items ?? props.actions ?? [];
  for (const action of list()) {
    if (action.label.length > MAX_SHORT_LABEL) {
      console.warn(
        `[ActionIconBar] label "${action.label}" exceeds ${MAX_SHORT_LABEL} chars; use a short fixed term.`,
      );
    }
  }
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
    return (
      <a
        class={cls()}
        classList={{ "tool-danger": props.action.danger }}
        href={props.action.href}
        title={accessibleName()}
        aria-label={accessibleName()}
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
