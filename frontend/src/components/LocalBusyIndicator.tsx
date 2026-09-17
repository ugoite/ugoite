interface LocalBusyIndicatorProps {
  /** Accessible label announced to assistive technology (sr-only). */
  label: string;
  size?: "sm" | "md";
}

/**
 * Panel-local busy indicator: spinner only, no visible loading text.
 * The label is exposed to assistive technology as the status content in a
 * visually-hidden span (no aria-label, so labelled-control queries keep
 * resolving to the real control). Use this for local fetches/refetches;
 * route/auth/bootstrap pending UI covers first paint only.
 */
export function LocalBusyIndicator(props: LocalBusyIndicatorProps) {
  const sizeClass = () => props.size === "sm" ? "localpending-sm" : "";
  return (
    <span class={`localpending ${sizeClass()}`} role="status">
      <span class="localspinner" aria-hidden="true" />
      <span class="ui-sr-only">{props.label}</span>
    </span>
  );
}
