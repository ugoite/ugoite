import type { JSX } from "solid-js";

interface FieldStackProps {
  /** Accessible name for the field list (e.g. the columns heading). */
  label: string;
  children: JSX.Element;
  class?: string;
}

/**
 * Shared field-row stack for the form create/edit dialogs (POL-UI-003,
 * POL-UI-007). Plain one-column rows with list/listitem semantics: each
 * `FieldStackRow` carries one field editor plus its inline guidance, with
 * no card chrome. Rows use native labeled controls so keyboard focus and
 * the focus path come from the platform, not custom key handlers.
 */
export function FieldStack(props: FieldStackProps) {
  return (
    <div
      class={`fieldStack ui-stack-sm${props.class ? ` ${props.class}` : ""}`}
      role="list"
      aria-label={props.label}
    >
      {props.children}
    </div>
  );
}

interface FieldStackRowProps {
  children: JSX.Element;
  /** Extra classes appended to the row (e.g. edit-mode separators). */
  class?: string;
}

export function FieldStackRow(props: FieldStackRowProps) {
  return (
    <div
      class={`fieldStackRow${props.class ? ` ${props.class}` : ""}`}
      role="listitem"
    >
      {props.children}
    </div>
  );
}
