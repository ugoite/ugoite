import { createSignal, Show } from "solid-js";
import { t } from "~/lib/i18n";
import type { CompositionParameterDefinition } from "~/lib/composition-api";
import { parseCompositionVariableInput } from "~/lib/ugoite-client/protocol";

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
 * One layout-placed Input bound to transient Work state. Values are sent to
 * the existing resolver on edit and never written to the saved revision.
 */
export function ParameterControl(props: {
  definition: CompositionParameterDefinition;
  displayName: string;
  value: unknown;
  onChange: (value: unknown | undefined) => void;
  invalid?: boolean;
}) {
  const [rawValue, setRawValue] = createSignal<string | undefined>();
  const [parseInvalid, setParseInvalid] = createSignal(false);
  let inputGeneration = 0;
  const label = () => props.displayName;
  const value = () =>
    rawValue() ??
      parameterDisplayValue(props.definition, props.value);
  const handleInput = async (raw: string) => {
    setRawValue(raw);
    setParseInvalid(false);
    const generation = ++inputGeneration;
    try {
      const parsed = await parseCompositionVariableInput(
        props.definition.type,
        raw,
      );
      if (generation !== inputGeneration) return;

      if (parsed.status === "valid") {
        props.onChange(parsed.value);
        setRawValue(undefined);
        return;
      }
      if (parsed.status === "unset") {
        props.onChange(undefined);
        setRawValue(undefined);
        return;
      }

      // Keep malformed raw text visible and let Core reject its untyped value.
      // In particular, never guess a number using JavaScript's Number().
      setParseInvalid(true);
      props.onChange(raw);
    } catch {
      if (generation !== inputGeneration) return;
      // If WASM is unavailable, the raw string remains untyped and Core still
      // validates it against the declared variable type.
      setParseInvalid(true);
      props.onChange(raw);
    }
  };

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
              aria-invalid={props.invalid || parseInvalid() || undefined}
              onInput={(event) => void handleInput(event.currentTarget.value)}
            />
          }
        >
          <select
            class="ui-input"
            required={props.definition.required || undefined}
            value={value()}
            aria-required={props.definition.required || undefined}
            aria-invalid={props.invalid || parseInvalid() || undefined}
            onInput={(event) => void handleInput(event.currentTarget.value)}
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
