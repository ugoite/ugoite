import { Show } from "solid-js";
import { t } from "~/lib/i18n";
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

export const serializeParameterInput = (
  definition: CompositionParameterDefinition,
  value: string,
): unknown | undefined => {
  if (value === "") return undefined;
  if (definition.type === "boolean") return value === "true";
  if (definition.type === "integer" || definition.type === "float") {
    const numeric = Number(value);
    return Number.isFinite(numeric) ? numeric : value;
  }
  return value;
};

/**
 * One layout-placed parameter control bound to transient Work state. The
 * control shows the declared human label, never the raw parameter id when
 * a label exists, and edits never touch stored state: the owner resolves
 * the new value and keeps query pages transient.
 */
export function ParameterControl(props: {
  definition: CompositionParameterDefinition;
  value: unknown;
  onChange: (value: unknown | undefined) => void;
  invalid?: boolean;
}) {
  const label = () => props.definition.label || props.definition.id;
  const value = () => parameterDisplayValue(props.definition, props.value);
  const handleInput = (raw: string) =>
    props.onChange(serializeParameterInput(props.definition, raw));

  return (
    <div class="ui-field">
      <label>
        <span>{label()}</span>
        <Show
          when={props.definition.type === "boolean"}
          fallback={
            <input
              class="ui-input"
              type={props.definition.type === "date"
                ? "date"
                : props.definition.type === "timestamp"
                ? "datetime-local"
                : props.definition.type === "integer" ||
                    props.definition.type === "float"
                ? "number"
                : "text"}
              step={props.definition.type === "integer"
                ? "1"
                : props.definition.type === "float"
                ? "any"
                : undefined}
              required={props.definition.required || undefined}
              value={value()}
              aria-invalid={props.invalid || undefined}
              onChange={(event) => handleInput(event.currentTarget.value)}
            />
          }
        >
          <select
            class="ui-input"
            required={props.definition.required || undefined}
            value={value()}
            aria-invalid={props.invalid || undefined}
            onChange={(event) => handleInput(event.currentTarget.value)}
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
