import { A } from "@solidjs/router";
import { t } from "~/lib/i18n";

interface BackLinkProps {
  /**
   * Immediate parent in the route hierarchy. Callers derive this from the
   * route params (e.g. the Entry detail path for Entry history), never a
   * cross-hierarchy shortcut: shell destinations stay in the shells.
   */
  href: string;
  /**
   * Full destination for assistive technology and the hover tooltip
   * (e.g. `t("entryHistory.backToEntry")`). The visible label stays the
   * positional short form so the hierarchy — already shown by the shell and
   * the page title — is not repeated as prose.
   */
  label: string;
  class?: string;
}

/**
 * Shared positional back control (POL-UI-001, POL-UI-007). Each nested
 * surface renders exactly one `BackLink` to its hierarchical parent with a
 * short visible label; the destination sentence lives only in the accessible
 * name and tooltip. The visible label uses the generic `common.back` key:
 * Entry-namespaced strings stay on Entry surfaces.
 */
export function BackLink(props: BackLinkProps) {
  return (
    <A
      href={props.href}
      class={props.class ?? "btn"}
      aria-label={props.label}
      title={props.label}
    >
      <span aria-hidden="true">{"← "}</span>
      {t("common.back")}
    </A>
  );
}
