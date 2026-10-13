import { Show } from "solid-js";
import { t } from "~/lib/i18n";
import type { CompositionVariableDefinition } from "~/lib/composition-api";

export const parameterDisplayValue = (
  definition: CompositionVariableDefinition,
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
 * One layout-placed Input bound to transient Work state. Values are sent to
 * the existing resolver on edit and never written to the saved revision.
 */
export function ParameterControl(props: {
  definition: CompositionVariableDefinition;
  displayName: string;
  value: unknown;
  rawValue?: string;
  onChange: (raw: string) => void;
  invalid?: boolean;
}) {
  const label = () => props.displayName;
  const value = () =>
    props.rawValue ??
      parameterDisplayValue(props.definition, props.value);

  return (
    <div class="ui-field">
      <label>
        <span>{label()}</span>
        <Show
          when={props.definition.type === "boolean"}
          fallback={
            <input
              class="ui-input"
              type="text"
              required={props.definition.required || undefined}
              value={value()}
              aria-required={props.definition.required || undefined}
              aria-invalid={props.invalid || undefined}
              onInput={(event) => props.onChange(event.currentTarget.value)}
            />
          }
        >
          <select
            class="ui-input"
            required={props.definition.required || undefined}
            value={value()}
            aria-required={props.definition.required || undefined}
            aria-invalid={props.invalid || undefined}
            onInput={(event) => props.onChange(event.currentTarget.value)}
          >
            <option value="">—</option>
            <option value="true">{t("composition.booleanTrue")}</option>
            <option value="false">{t("composition.booleanFalse")}</option>
          </select>
        </Show>
      </label>
    </div>
  );
}
