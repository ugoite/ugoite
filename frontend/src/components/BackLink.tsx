import { A } from "@solidjs/router";
import { UiIcon } from "~/components/UiIcon";

interface BackLinkProps {
  /**
   * Immediate parent in the route hierarchy. Callers derive this from the
   * route params (e.g. the Entry detail path for Entry history), never a
   * cross-hierarchy shortcut: shell destinations stay in the shells.
   */
  href: string;
  /**
   * Full destination for assistive technology and the hover tooltip
   * (e.g. `t("entryHistory.backToEntry")`). The destination stays available
   * to the user while the shared control keeps only its positional icon visible.
   */
  label: string;
  class?: string;
}

/**
 * Shared positional back control (POL-UI-001, POL-UI-007). Each nested
 * surface renders exactly one `BackLink` to its hierarchical parent. Its
 * destination-specific label stays on the link for assistive technology and
 * hover discovery while the visible control uses the shared left chevron.
 */
export function BackLink(props: BackLinkProps) {
  const className = () =>
    ["pill", "iconpill", "icononly", "back-link", props.class ?? ""]
      .filter(Boolean)
      .join(" ");

  return (
    <A
      href={props.href}
      class={className()}
      aria-label={props.label}
      title={props.label}
    >
      <UiIcon name="chevron-left" />
      <span class="ui-sr-only">{props.label}</span>
    </A>
  );
}
