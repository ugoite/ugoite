import type { JSX } from "solid-js";
import { UiIcon, type UiIconName } from "~/components/UiIcon";

interface IconButtonProps {
  icon: UiIconName;
  /** Accessible name. Required so icon-only controls stay labelled. */
  label: string;
  /** Optional hover/AT description, e.g. a blocked-action reason. */
  title?: string;
  disabled?: boolean;
  active?: boolean;
  class?: string;
  ref?: (element: HTMLButtonElement) => void;
  onClick?: JSX.EventHandlerUnion<HTMLButtonElement, MouseEvent>;
  type?: "button" | "submit" | "reset";
}

/**
 * Shared 44px icon-only button. Button-only: navigation uses `IconLink`.
 * The visible content is the icon slot only; `label` is exposed via
 * `aria-label` on the button itself (no wrapper aria-label).
 */
export function IconButton(props: IconButtonProps) {
  const cls = () =>
    [
      "pill",
      "iconpill",
      "icononly",
      props.active ? "active" : "",
      props.class ?? "",
    ]
      .filter(Boolean)
      .join(" ");
  return (
    <button
      class={cls()}
      type={props.type ?? "button"}
      ref={props.ref}
      aria-label={props.label}
      aria-pressed={props.active}
      title={props.title}
      disabled={props.disabled}
      onClick={props.onClick}
    >
      <UiIcon name={props.icon} />
      <span class="ui-sr-only">{props.label}</span>
    </button>
  );
}
