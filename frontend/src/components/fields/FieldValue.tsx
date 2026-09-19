import { For, Match, Show, Switch } from "solid-js";

export interface FieldValueViewProps {
  name: string;
  value: unknown;
}

const isBlank = (value: unknown) =>
  value === null ||
  value === undefined ||
  (typeof value === "string" && value.trim() === "");

const toText = (value: unknown): string => {
  if (typeof value === "string") return value;
  if (typeof value === "boolean" || typeof value === "number") {
    return String(value);
  }
  try {
    return JSON.stringify(value);
  } catch {
    return "";
  }
};

/**
 * Read-only value renderer for revision review.
 *
 * Plain text output only — never disabled inputs. Editing controls belong
 * to the shared FieldInput family; review surfaces render values as text
 * so there is exactly one interactive field implementation.
 */
export function FieldValueView(props: FieldValueViewProps) {
  return (
    <Switch
      fallback={
        <p class="ui-field-value-text" data-field-value={props.name}>
          {toText(props.value)}
        </p>
      }
    >
      <Match when={isBlank(props.value)}>
        <p class="ui-field-value-empty ui-muted" data-field-value={props.name}>
          —
        </p>
      </Match>
      <Match
        when={Array.isArray(props.value) &&
          (props.value as unknown[]).every((item) =>
            typeof item === "string" || typeof item === "number" ||
            typeof item === "boolean"
          )}
      >
        <ul class="ui-field-value-list" data-field-value={props.name}>
          <For each={props.value as Array<string | number | boolean>}>
            {(item) => <li>{String(item)}</li>}
          </For>
        </ul>
      </Match>
      <Match when={Array.isArray(props.value)}>
        <ul class="ui-field-value-list" data-field-value={props.name}>
          <For each={props.value as unknown[]}>
            {(item) => <li>{toText(item)}</li>}
          </For>
        </ul>
      </Match>
      <Match
        when={typeof props.value === "object" && props.value !== null}
      >
        <dl class="ui-field-value-object" data-field-value={props.name}>
          <For each={Object.entries(props.value as Record<string, unknown>)}>
            {([key, entryValue]) => (
              <div class="ui-field-value-property">
                <dt>{key}</dt>
                <dd>{toText(entryValue)}</dd>
              </div>
            )}
          </For>
        </dl>
      </Match>
    </Switch>
  );
}

export interface FieldValuesViewProps {
  fields: Array<{ name: string }>;
  getValue: (name: string) => unknown;
}

/**
 * Read-only field list for revision review. Mirrors the EntryFields row
 * structure (name heading + value) with zero interactive controls.
 */
export function FieldValuesView(props: FieldValuesViewProps) {
  return (
    <div class="form entry-field-values readonly">
      <div class="ui-entry-field-list">
        <For each={props.fields}>
          {(field) => (
            <div class="field ui-entry-field">
              <div class="ui-entry-field-heading">
                <div class="min-w-0">
                  <p class="ui-label">{field.name}</p>
                </div>
              </div>
              <FieldValueView
                name={field.name}
                value={props.getValue(field.name)}
              />
            </div>
          )}
        </For>
      </div>
    </div>
  );
}

/** Back-compat aliases for review call sites. */
export const FieldValue = FieldValueView;
export const FieldValues = FieldValuesView;
