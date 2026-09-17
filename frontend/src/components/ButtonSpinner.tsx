/**
 * Small inline spinner for busy buttons. The button label stays stable
 * (Save/Run/Restore/Create/Upload/Search); the spinner is the only busy
 * signal alongside `disabled` + `aria-busy`. No "Saving..." text swaps.
 */
export function ButtonSpinner() {
  return <span class="btnSpinner" aria-hidden="true" />;
}
