import type { JSX } from "solid-js";
import { UiIcon, type UiIconName } from "~/components/UiIcon";

interface IconButtonBase {
  icon: UiIconName;
  /** Accessible name. Required so icon-only controls stay labelled. */
  label: string;
  disabled?: boolean;
  active?: boolean;
  class?: string;
}

type IconButtonProps =
  & IconButtonBase
  & (
    | {
      href: string;
      onClick?: JSX.EventHandlerUnion<HTMLAnchorElement, MouseEvent>;
      type?: undefined;
    }
    | {
      href?: undefined;
      onClick?: JSX.EventHandlerUnion<HTMLButtonElement, MouseEvent>;
      type?: "button" | "submit" | "reset";
    }
  );

/**
 * Shared 44px icon-only control. Renders an anchor when `href` is given,
 * otherwise a button. The visible content is the icon slot only; `label`
 * is exposed via `aria-label`.
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
  const content = () => (
    <>
      <UiIcon name={props.icon} />
      <span class="ui-sr-only">{props.label}</span>
    </>
  );
  if (props.href !== undefined) {
    const { icon: _icon, label, active: _active, class: _class, ...rest } =
      props as IconButtonBase & {
        href: string;
        onClick?: JSX.EventHandlerUnion<HTMLAnchorElement, MouseEvent>;
      };
    return (
      <a
        class={cls()}
        aria-label={label}
        aria-current={props.active ? "page" : undefined}
        {...rest}
      >
        {content()}
      </a>
    );
  }
  const {
    icon: _icon,
    label,
    active: _active,
    class: _class,
    href: _href,
    ...rest
  } = props as IconButtonBase & {
    onClick?: JSX.EventHandlerUnion<HTMLButtonElement, MouseEvent>;
    type?: "button" | "submit" | "reset";
  };
  return (
    <button
      class={cls()}
      type={props.type ?? "button"}
      aria-label={label}
      aria-pressed={props.active}
      disabled={props.disabled}
      {...rest}
    >
      {content()}
    </button>
  );
}
