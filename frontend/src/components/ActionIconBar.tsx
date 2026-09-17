import { Index, Show } from "solid-js";
import type { Accessor, JSX } from "solid-js";
import { ButtonSpinner } from "~/components/ButtonSpinner";
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
  /**
   * Busy state (PR4): the icon slot swaps to a fixed-size spinner and the
   * control reports `aria-busy`. The visible label stays stable so the bar
   * never shifts layout mid-action.
   */
  busy?: boolean;
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
 *
 * Rows render through `Index` (positional, nodes never recreated): when an
 * action flips between weak/disabled and strong/enabled (PR4 save) or
 * busy/idle, the same button node updates in place, so focus is never
 * dropped and previously queried references stay attached.
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
      <Index each={list()}>
        {(action) => <ActionTile action={action} />}
      </Index>
    </div>
  );
}

function ActionTile(props: { action: Accessor<ActionItem> }) {
  const item = () => props.action();
  const accessibleName = () =>
    item().accessibleName ?? item().label;
  const cls = () =>
    ["tool", item().class ?? ""].filter(Boolean).join(" ");
  const busy = () => item().busy ?? false;
  const content = () => (
    <>
      <Show when={busy()} fallback={
        <Show when={item().icon}>
          {(icon) => <UiIcon name={icon()} />}
        </Show>
      }
      >
        <ButtonSpinner />
      </Show>
      <span class="toolLabel">{item().label}</span>
    </>
  );
  // Row kind (link vs button) is fixed when the row is created: Index keeps
  // each positional row's node across state flips so weak/disabled,
  // strong/enabled, and busy/idle all update the same node in place (no
  // focus loss, no detached references). All callers keep a static kind per
  // position (save/delete buttons; history/info links). A nav target that
  // is unavailable at creation is omitted (never a disabled `<a>`); a
  // disabled action renders a true disabled `<button>`.
  const initial = props.action();
  if (initial.href !== undefined) {
    if (initial.disabled) return null;
    return (
      <a
        class={cls()}
        classList={{ "tool-danger": item().danger }}
        href={item().href as string}
        title={accessibleName()}
        aria-label={accessibleName()}
        aria-busy={busy() || undefined}
        onClick={item().onClick as JSX.EventHandlerUnion<
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
      classList={{ "tool-danger": item().danger }}
      type="button"
      title={accessibleName()}
      aria-label={accessibleName()}
      aria-busy={busy() || undefined}
      disabled={item().disabled}
      onClick={item().onClick as JSX.EventHandlerUnion<
        HTMLButtonElement,
        MouseEvent
      >}
    >
      {content()}
    </button>
  );
}
