import type { CompositionParameterDefinition } from "~/lib/composition-api";

export const parameterDisplayValue = (
  definition: CompositionParameterDefinition,
  value: unknown,
): string => {
  const current = value === undefined ? definition.default : value;
  if (current === null || current === undefined) return "";
  if (typeof current === "string") return current;
  if (typeof current === "number" || typeof current === "boolean") {
    return String(current);
  }
  return "";
};

/**
 * Read-only view of an Input's value until the shared Rust/WASM typed parser
 * is available to the browser. No browser-side parsing or query mutation is
 * permitted at this boundary.
 */
export function ParameterControl(props: {
  definition: CompositionParameterDefinition;
  displayName: string;
  value: unknown;
  invalid?: boolean;
}) {
  const label = () => props.displayName;
  const value = () => parameterDisplayValue(props.definition, props.value);

  return (
    <div class="ui-field">
      <label>
        <span>{label()}</span>
        <input
          class="ui-input"
          type="text"
          value={value()}
          readOnly
          aria-readonly="true"
          aria-required={props.definition.required || undefined}
          aria-invalid={props.invalid || undefined}
        />
      </label>
    </div>
  );
}
