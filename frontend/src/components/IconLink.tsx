import type { JSX } from "solid-js";
import { UiIcon, type UiIconName } from "~/components/UiIcon";

interface IconLinkProps {
  icon: UiIconName;
  /** Accessible name. Required so icon-only controls stay labelled. */
  label: string;
  href: string;
  active?: boolean;
  class?: string;
  onClick?: JSX.EventHandlerUnion<HTMLAnchorElement, MouseEvent>;
}

/**
 * Shared 44px icon-only link. Link-only: actions use `IconButton`.
 * If the nav target is unavailable, callers omit this element entirely
 * (never a disabled `<a>`). The accessible name lives on the link itself.
 */
export function IconLink(props: IconLinkProps) {
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
    <a
      class={cls()}
      href={props.href}
      aria-label={props.label}
      aria-current={props.active ? "page" : undefined}
      onClick={props.onClick}
    >
      <UiIcon name={props.icon} />
      <span class="ui-sr-only">{props.label}</span>
    </a>
  );
}
